// Dev-only mock backend. Imported dynamically by api.ts and guarded by
// import.meta.env.DEV, so it is tree-shaken out of the production bundle.
//
// It fakes the whole Tauri surface so `vite dev` renders the real UI in a
// plain browser: mock settings (host pre-configured so we land on the
// connected view), one adhd-focus widget whose Card is the verbatim output of
// widgets/adhd-focus/state.py, a rich chat reply to exercise the markdown
// renderer, simulated streaming, and a hover simulator wired to the strip.

import type {
  Card,
  ChatStatus,
  HostInfo,
  PanelInfo,
  Settings,
  WidgetSpec,
} from "./types";

type Handler = (payload: unknown) => void;

// --- fixtures ------------------------------------------------------------
const MOCK_SETTINGS: Settings = {
  host: "127.0.0.1",
  port: 9119,
  username: "demo",
  password: "",
  token: "dev-loopback-token",
  autostart: true,
};

const ADHD_SPEC: WidgetSpec = {
  spec: 1,
  id: "adhd-focus",
  name: "Focus",
  icon: "🎯",
  description: "One step at a time over the personal task universe.",
  source: {
    type: "chat",
    on_start: "/adhd",
    glance: { type: "script", command: ["python3", "state.py", "--glance"] },
  },
  refresh: { interval: 60, while_visible: 15 },
  actions: [
    { id: "done", label: "Done", icon: "✓", style: "primary", effect: { type: "chat", text: "done" } },
    { id: "skip", label: "Skip", icon: "→", effect: { type: "chat", text: "skip" } },
    { id: "smaller", label: "Smaller", icon: "✂", effect: { type: "chat", text: "smaller" } },
    { id: "why", label: "Why", icon: "?", effect: { type: "chat", text: "why" } },
    { id: "pause", label: "Pause", icon: "⏸", style: "danger", effect: { type: "chat", text: "pause" } },
  ],
  input: { placeholder: "veto / park / anything…", effect: { type: "chat" } },
};

// Same shape as `python3 widgets/adhd-focus/state.py` output (fictional data).
const ADHD_CARD: Card = {
  card: 1,
  glance: {
    text: "Renew passport: book appointment",
    detail: "~5 min · step 2 of 4",
    urgency: "attention",
  },
  title: "Step 2 of 4",
  body: [
    { type: "text", text: "Book the passport renewal appointment — the form takes ~3 min." },
    { type: "kv", items: [["Source", "todoist"], ["Est", "~5 min"]] },
    { type: "progress", value: 1, max: 4, label: "batch" },
    { type: "divider" },
    { type: "md", text: "**Next:** Reply to the venue email · Submit expense report (+4 more)" },
    { type: "kv", items: [["Queue", "6 next · 1 parked · 2 triage"]] },
  ],
  status: { state: "ok", detail: "" },
  ts: "2026-07-22T20:16:06+00:00",
  chat: { on_start: "/adhd", session_tag: "notch:adhd-focus" },
};

const CHAT_HISTORY = [
  "**Step 2 of 4** — Book the passport renewal appointment (~5 min).",
  "",
  "Everything you need is below. Book the earliest morning slot, then run the confirm command once you have the reference number.",
  "",
  "- Open the appointment portal",
  "- Pick the earliest morning slot",
  "- Copy the reference number",
  "",
  "```",
  "remctl done 42",
  "```",
  "",
  "[Passport appointment portal](https://example.gov/appointments)",
].join("\n");

const STREAM_REPLY =
  "On it. Marking this step **done** and pulling the next one from the queue — " +
  "*Reply to the venue email*. Give me a second…";

// --- backend -------------------------------------------------------------
export function createMockBackend(): {
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  listen: <T>(event: string, cb: (payload: T) => void) => Promise<() => void>;
} {
  // ?theme=dark|light forces a theme for screenshots/dev (system wins otherwise).
  const forcedTheme = new URLSearchParams(location.search).get("theme");
  if (forcedTheme === "dark" || forcedTheme === "light") {
    document.documentElement.dataset.theme = forcedTheme;
  }

  const listeners = new Map<string, Set<Handler>>();
  let settings: Settings = { ...MOCK_SETTINGS };
  let hovering = false;

  // Dev-only: a fake desktop behind the transparent window so the vibrancy
  // panel and black strip are visible when previewing in a plain browser.
  document.body.classList.add("dev");
  if (!document.querySelector(".dev-backdrop")) {
    const backdrop = document.createElement("div");
    backdrop.className = "dev-backdrop";
    document.body.prepend(backdrop);
  }

  const emit = (event: string, payload: unknown): void => {
    const set = listeners.get(event);
    if (!set) return;
    for (const cb of set) cb(payload);
  };

  const emitConn = (state: string, detail?: string): void =>
    emit("conn:status", detail ? { state, detail } : { state });

  const emitChat = (kind: string, text?: string): void =>
    emit("chat:event", { widgetId: ADHD_SPEC.id, kind, ...(text ? { text } : {}) });

  // Hover simulator: treat the whole HUD like the native tracking area.
  const inHud = (node: EventTarget | null): boolean =>
    node instanceof Element && node.closest(".hud") !== null;
  document.addEventListener("mouseover", (e) => {
    if (!hovering && inHud(e.target)) {
      hovering = true;
      emit("notch:hover", { entered: true });
    }
  });
  document.addEventListener("mouseout", (e) => {
    if (hovering && !inHud(e.relatedTarget)) {
      hovering = false;
      emit("notch:hover", { entered: false });
    }
  });
  // Keyboard: ⌥Space fakes the global shortcut for browser testing.
  document.addEventListener("keydown", (e) => {
    if (e.altKey && e.code === "Space") {
      e.preventDefault();
      emit("notch:shortcut", {});
    }
  });

  const simulateStream = (): void => {
    emitChat("start");
    const words = STREAM_REPLY.split(" ");
    let idx = 0;
    const tick = (): void => {
      if (idx >= words.length) {
        emitChat("complete", STREAM_REPLY);
        return;
      }
      emitChat("delta", (idx === 0 ? "" : " ") + words[idx]);
      idx++;
      window.setTimeout(tick, 55);
    };
    window.setTimeout(tick, 260);
  };

  const invoke = async <T>(
    cmd: string,
    args?: Record<string, unknown>,
  ): Promise<T> => {
    const out = (v: unknown): T => v as T;
    switch (cmd) {
      case "get_settings":
        return out(settings);
      case "set_settings":
        settings = { ...settings, ...(args?.patch as Partial<Settings>) };
        return out(settings);
      case "connect": {
        emitConn("connecting");
        await delay(240);
        emitConn("connected");
        const info: HostInfo = {
          ok: true,
          host_version: "mock-0.1.0",
          widgets: [ADHD_SPEC],
        };
        return out(info);
      }
      case "disconnect":
        emitConn("disconnected");
        return out(undefined);
      case "get_glance":
      case "get_state":
        return out(ADHD_CARD);
      case "run_action":
        return out(ADHD_CARD);
      case "chat_ensure":
        return out({ session_id: "mock-session", fresh: false } as ChatStatus);
      case "chat_send":
        simulateStream();
        return out(undefined);
      case "chat_history":
        return out(CHAT_HISTORY);
      case "chat_interrupt":
        return out(undefined);
      case "set_expanded":
        return out(undefined);
      case "panel_info":
        return out({
          has_notch: true,
          notch_width: 220,
          notch_height: 34,
          scale: 2,
        } as PanelInfo);
      case "open_url":
        window.open(String(args?.url ?? ""), "_blank");
        return out(undefined);
      case "copy_text":
        void navigator.clipboard?.writeText(String(args?.text ?? ""));
        return out(undefined);
      default:
        return out(undefined);
    }
  };

  const listen = async <T>(
    event: string,
    cb: (payload: T) => void,
  ): Promise<() => void> => {
    const set = listeners.get(event) ?? new Set<Handler>();
    const handler: Handler = (p) => cb(p as T);
    set.add(handler);
    listeners.set(event, set);
    return () => set.delete(handler);
  };

  return { invoke, listen };
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}
