// Boot + state machine. Owns the collapsed⇄expanded lifecycle, connection
// state, per-widget glance polling, and the chat streaming wiring. All network
// I/O lives in the Rust core behind api.ts; this file only orchestrates the
// view (see docs/DESIGN.md for every motion/interaction number below).

import * as api from "./api";
import type {
  Card,
  ConnState,
  Settings,
  Urgency,
  WidgetSpec,
} from "./types";
import { renderStrip, type StripData } from "./views/strip";
import { createPanel, type PanelData, type PanelHandle } from "./views/panel";
import type { SettingsContext } from "./views/settings";

const DOCS_URL =
  "https://github.com/czekaj/hermes-notch/blob/main/docs/WIDGET_SPEC.md";
const URGENCY_RANK: Record<Urgency, number> = {
  normal: 0,
  attention: 1,
  urgent: 2,
};

const reduceMotion = window.matchMedia(
  "(prefers-reduced-motion: reduce)",
).matches;
const COLLAPSE_SHRINK_MS = reduceMotion ? 90 : 210;
const GRACE_MS = 350;

interface AppState {
  conn: ConnState;
  settings: Settings;
  widgets: WidgetSpec[];
  cards: Record<string, Card>;
  activeId: string | null;
  expanded: boolean;
  showSettings: boolean;
  forcedSettings: boolean;
  chatBodyLive: boolean;
}

const state: AppState = {
  conn: "disconnected",
  settings: {
    host: "",
    port: 9119,
    username: "",
    password: "",
    token: "",
    autostart: false,
  },
  widgets: [],
  cards: {},
  activeId: null,
  expanded: false,
  showSettings: false,
  forcedSettings: false,
  chatBodyLive: false,
};

const root = document.getElementById("app") as HTMLElement;
const hud = document.createElement("div");
hud.className = "hud is-collapsed";
root.append(hud);

let panel: PanelHandle;
let stripEl: HTMLElement | null = null;
let graceTimer = 0;
const pollers = new Map<string, number>();

// --- derived helpers -----------------------------------------------------
function activeSpec(): WidgetSpec | null {
  return state.widgets.find((w) => w.id === state.activeId) ?? null;
}

function isChat(spec: WidgetSpec | null): boolean {
  if (!spec) return false;
  if (spec.source.type === "chat") return true;
  return Boolean(state.cards[spec.id]?.chat);
}

function urgencyOf(id: string): Urgency {
  return state.cards[id]?.glance?.urgency ?? "normal";
}

function urgencyMap(): Record<string, Urgency> {
  const map: Record<string, Urgency> = {};
  for (const w of state.widgets) map[w.id] = urgencyOf(w.id);
  return map;
}

// Highest-urgency widget wins the collapsed strip (ties → first).
function pickShownId(): string | null {
  let best: string | null = null;
  let bestRank = -1;
  for (const w of state.widgets) {
    const rank = URGENCY_RANK[urgencyOf(w.id)];
    if (rank > bestRank) {
      bestRank = rank;
      best = w.id;
    }
  }
  return best;
}

function hostLabel(): string {
  const s = state.settings;
  return s.host ? (s.port ? `${s.host}:${s.port}` : s.host) : "hermes host";
}

// --- painting ------------------------------------------------------------
function stripData(): StripData {
  return {
    conn: state.conn,
    hostLabel: hostLabel(),
    widgets: state.widgets,
    glances: state.cards,
    shownId: state.activeId,
  };
}

function paintStrip(): void {
  const next = renderStrip(stripData());
  if (stripEl) stripEl.replaceWith(next);
  else hud.insertBefore(next, panel.el);
  stripEl = next;
  syncFrame();
}

function panelData(): PanelData {
  const id = state.activeId;
  return {
    spec: activeSpec(),
    card: id ? state.cards[id] ?? null : null,
    widgets: state.widgets,
    activeId: id,
    urgencies: urgencyMap(),
    docsUrl: DOCS_URL,
  };
}

function settingsCtx(): SettingsContext {
  const spec = activeSpec();
  const chatActive = isChat(spec) && state.conn === "connected";
  return {
    canBack: !state.forcedSettings,
    onBack: () => {
      state.showSettings = false;
      panel.showSettings(false);
    },
    onConnect: (values) => {
      void connectWith(values);
    },
    ...(chatActive && spec
      ? {
          resetLabel: `Reset ${spec.name} session`,
          onReset: () => {
            void api
              .chatReset(spec.id)
              .then(() => {
                state.chatBodyLive = false;
                state.showSettings = false;
                paintPanel();
              })
              .catch((e) => panel.showError(String(e)));
          },
        }
      : {}),
  };
}

function paintPanel(): void {
  panel.update(panelData());
  panel.setSettings(state.settings, settingsCtx());
  panel.showSettings(state.showSettings || state.forcedSettings);
  syncFrame();
}

// --- window/shape sync -----------------------------------------------------
// The native window must match the visible CSS shape exactly: the vibrancy
// layer fills the whole window, so any excess shows as a bare glass rectangle
// (and a too-small window clips the shape). Measure with offset* metrics —
// they ignore the entrance transform — and report to the Rust side.
let frameSyncQueued = false;
function syncFrame(): void {
  if (frameSyncQueued) return;
  frameSyncQueued = true;
  requestAnimationFrame(() => {
    frameSyncQueued = false;
    const el = state.expanded ? panel.el : stripEl;
    if (!el) return;
    const w = Math.max(el.offsetWidth, el.scrollWidth);
    const h = el.offsetTop + Math.max(el.offsetHeight, el.scrollHeight);
    if (w < 40 || h < 20) return; // not laid out yet
    void api.setExpanded(state.expanded, Math.ceil(w) + 2, Math.ceil(h) + 2);
  });
}

// --- connection ----------------------------------------------------------
function setConn(next: ConnState): void {
  state.conn = next;
}

async function connectWith(values: Settings): Promise<void> {
  state.settings = await api.setSettings(values);
  state.forcedSettings = false;
  await startConnect();
  if (state.conn === "connected") {
    state.showSettings = false;
    paintPanel();
  }
}

async function startConnect(): Promise<void> {
  setConn("connecting");
  paintStrip();
  try {
    const info = await api.connect();
    state.widgets = info.widgets ?? [];
    setConn("connected");
    await pollAllGlances();
    // Never steal the active widget mid-view, but always seed it on first
    // connect — the user may already be hovering while we connect.
    if (!state.expanded || state.activeId == null) state.activeId = pickShownId();
    paintStrip();
    paintPanel();
    // If the user was already hovering while we connected, the expand path
    // ran before any widget existed — pull the full card/chat body now.
    if (state.expanded && !state.showSettings) void refreshActive();
    restartPollers();
  } catch {
    setConn("error");
    forceSettings();
  }
}

function forceSettings(): void {
  state.forcedSettings = true;
  state.showSettings = true;
  state.expanded = true;
  hud.classList.remove("is-collapsed");
  hud.classList.add("is-expanded");
  paintStrip();
  paintPanel(); // syncFrame() inside sizes the window to the settings card
  panel.playEntrance();
}

// --- glance polling ------------------------------------------------------
async function pollAllGlances(): Promise<void> {
  await Promise.all(
    state.widgets.map(async (w) => {
      try {
        state.cards[w.id] = await api.getGlance(w.id);
      } catch {
        /* leave any prior card in place */
      }
    }),
  );
}

async function pollOne(id: string): Promise<void> {
  try {
    state.cards[id] = await api.getGlance(id);
  } catch {
    return;
  }
  if (!state.expanded || state.activeId == null) state.activeId = pickShownId();
  paintStrip();
  const spec = state.widgets.find((w) => w.id === id) ?? null;
  if (
    state.expanded &&
    id === state.activeId &&
    !isChat(spec) &&
    !state.chatBodyLive
  ) {
    paintPanel();
  }
}

function intervalMs(spec: WidgetSpec): number {
  const r = spec.refresh ?? {};
  const secs = state.expanded
    ? r.while_visible ?? r.interval ?? 30
    : r.interval ?? 30;
  return Math.max(3, secs) * 1000;
}

function restartPollers(): void {
  for (const handle of pollers.values()) window.clearInterval(handle);
  pollers.clear();
  for (const w of state.widgets) {
    const handle = window.setInterval(() => void pollOne(w.id), intervalMs(w));
    pollers.set(w.id, handle);
  }
}

// --- expand / collapse ---------------------------------------------------
function clearGrace(): void {
  if (graceTimer) {
    window.clearTimeout(graceTimer);
    graceTimer = 0;
  }
}

async function expand(): Promise<void> {
  if (state.expanded) return;
  state.expanded = true;
  clearGrace();
  hud.classList.remove("is-collapsed");
  hud.classList.add("is-expanded");
  paintPanel(); // paints, then syncFrame() sizes the window to the panel
  panel.playEntrance();
  void refreshActive();
  // Insurance re-measures: after the entrance animation settles and after
  // fonts finish loading — both can grow the layout past the first measure.
  window.setTimeout(syncFrame, 400);
  void document.fonts.ready.then(() => syncFrame());
  restartPollers();
}

function collapse(): void {
  if (!state.expanded || state.forcedSettings) return;
  state.expanded = false;
  state.showSettings = false;
  state.chatBodyLive = false;
  clearGrace();
  hud.classList.remove("is-expanded");
  hud.classList.add("is-collapsed");
  panel.showSettings(false);
  state.activeId = pickShownId();
  paintStrip();
  // Shrink the window only after the collapse animation has played out.
  window.setTimeout(() => syncFrame(), COLLAPSE_SHRINK_MS);
  restartPollers();
}

function scheduleCollapse(): void {
  if (state.forcedSettings) return;
  clearGrace();
  graceTimer = window.setTimeout(() => {
    if (!panel.isInputFocused()) collapse();
  }, GRACE_MS);
}

function toggle(): void {
  if (state.expanded) collapse();
  else void expand();
}

// Pull the full Card (and chat history) for the active widget on expand/switch.
async function refreshActive(): Promise<void> {
  const spec = activeSpec();
  if (!spec) {
    paintPanel();
    return;
  }
  state.chatBodyLive = false;
  try {
    state.cards[spec.id] = await api.getState(spec.id);
  } catch {
    /* keep glance card */
  }
  paintPanel();
  if (isChat(spec)) {
    try {
      await api.chatEnsure(spec.id);
      const hist = await api.chatHistory(spec.id);
      if (hist.trim()) {
        panel.showComplete(hist);
        state.chatBodyLive = true;
      }
    } catch {
      /* history is best-effort */
    }
  }
}

// --- action / input handlers ---------------------------------------------
function handleAction(actionId: string): void {
  const spec = activeSpec();
  if (!spec) return;
  const action = spec.actions?.find((a) => a.id === actionId);
  if (!action) return;

  if (action.effect.type === "script") {
    void api
      .runAction(spec.id, actionId)
      .then((card) => {
        if (card) {
          state.cards[spec.id] = card;
          state.chatBodyLive = false;
          paintPanel();
        }
      })
      .catch(() => panel.showError("That action failed."));
    return;
  }

  // chat effect: inject verbatim text; the reply streams back as events.
  const text = action.effect.text ?? "";
  panel.showWorking();
  void api.chatSend(spec.id, text).catch((e) => panel.showError(String(e)));
}

function handleSubmit(text: string): void {
  const spec = activeSpec();
  if (!spec) return;
  panel.showWorking();
  void api.chatSend(spec.id, text).catch((e) => panel.showError(String(e)));
}

function handleSelectWidget(id: string): void {
  if (id === state.activeId) return;
  state.activeId = id;
  state.chatBodyLive = false;
  paintStrip();
  paintPanel();
  void refreshActive();
}

// --- events --------------------------------------------------------------
function wireEvents(): void {
  void api.onHover(({ entered }) => {
    if (entered) {
      clearGrace();
      void expand();
    } else {
      scheduleCollapse();
    }
  });

  void api.onShortcut(() => toggle());

  void api.onConnStatus(({ state: s }) => {
    setConn(s);
    if (s === "error") forceSettings();
    paintStrip();
  });

  void api.onChatEvent((e) => {
    if (e.widgetId !== state.activeId) return;
    switch (e.kind) {
      case "start":
        panel.showWorking();
        break;
      case "status":
        panel.showWorking();
        break;
      case "delta":
        panel.appendDelta(e.text ?? "");
        break;
      case "complete":
        panel.showComplete(e.text ?? "");
        state.chatBodyLive = true;
        break;
      case "error":
        panel.showError(e.text ?? "Something went wrong.");
        break;
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") collapse();
  });
}

// --- boot ----------------------------------------------------------------
async function boot(): Promise<void> {
  panel = createPanel({
    copy: (text) => void api.copyText(text),
    openLink: (url) => {
      void api.openUrl(url);
      collapse();
    },
    onAction: handleAction,
    onSubmit: handleSubmit,
    onSelectWidget: handleSelectWidget,
    onGear: () => {
      state.showSettings = true;
      panel.setSettings(state.settings, settingsCtx());
      panel.showSettings(true);
    },
  });
  hud.append(panel.el);
  paintStrip();

  // Content-driven resizes (chat streaming, settings flip) → window follows.
  new ResizeObserver(() => syncFrame()).observe(panel.el);

  // Notch geometry → CSS vars so the strip hugs the island exactly.
  try {
    const pi = await api.panelInfo();
    document.documentElement.style.setProperty(
      "--notch-w",
      `${pi.notch_width}px`,
    );
    document.documentElement.style.setProperty(
      "--notch-h",
      `${pi.has_notch ? pi.notch_height : 0}px`,
    );
    document.body.classList.toggle("has-notch", pi.has_notch);
    document.body.classList.toggle("no-notch", !pi.has_notch);
  } catch {
    document.body.classList.add("no-notch");
  }

  wireEvents();

  try {
    state.settings = await api.getSettings();
  } catch {
    /* fall through to settings */
  }

  paintPanel();

  if (state.settings.host && state.settings.host.trim()) {
    await startConnect();
  } else {
    forceSettings();
  }
}

void boot();
