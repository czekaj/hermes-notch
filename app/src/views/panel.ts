// Expanded panel: the active widget's Card rendered with native fidelity —
// header (title + widget-switcher dots), scrolling body, action buttons,
// footer (free-text input + gear). Also owns the settings flip (back face).
//
// Streaming (chat) replies are driven imperatively via showWorking /
// appendDelta / showComplete / showError so input focus and the rest of the
// panel survive mid-stream. main.ts guarantees update() is not called against
// a chat widget while its streamed body is live.

import type {
  Card,
  Settings,
  Urgency,
  WidgetAction,
  WidgetSpec,
} from "../types";
import { el, type RenderCtx } from "../render/dom";
import { renderBlocks } from "../render/card";
import { renderMarkdown } from "../render/markdown";
import { renderSettings, type SettingsContext } from "./settings";

const GEAR_GLYPH = "⚙"; // ⚙

export interface PanelContext {
  copy: (text: string) => void;
  openLink: (url: string) => void;
  onAction: (actionId: string) => void;
  onSubmit: (text: string) => void;
  onSelectWidget: (id: string) => void;
  onGear: () => void;
}

export interface PanelData {
  spec: WidgetSpec | null;
  card: Card | null;
  widgets: WidgetSpec[];
  activeId: string | null;
  urgencies: Record<string, Urgency>;
  docsUrl: string;
}

export interface PanelHandle {
  el: HTMLElement;
  update: (data: PanelData) => void;
  playEntrance: () => void;
  setSettings: (settings: Settings, sctx: SettingsContext) => void;
  showSettings: (show: boolean) => void;
  showWorking: () => void;
  appendDelta: (text: string) => void;
  showComplete: (markdown: string) => void;
  showError: (message: string) => void;
  isInputFocused: () => boolean;
  focusInput: () => void;
  hasInput: () => boolean;
}

export function createPanel(ctx: PanelContext): PanelHandle {
  const renderCtx: RenderCtx = { copy: ctx.copy, openLink: ctx.openLink };

  const root = el("div", "panel");
  const flip = el("div", "panel-flip");
  const front = el("div", "panel-face front");
  const back = el("div", "panel-face back");
  flip.append(front, back);
  root.append(flip);

  // Persistent front-face regions (rebuilt on update).
  const header = el("div", "panel-header");
  const body = el("div", "body");
  const actions = el("div", "actions");
  const footer = el("div", "footer");
  const footerLeft = el("div", "footer-left");
  const gear = el("button", "iconbtn gear");
  gear.type = "button";
  gear.textContent = GEAR_GLYPH;
  gear.setAttribute("aria-label", "Settings");
  gear.addEventListener("click", ctx.onGear);
  footer.append(footerLeft, gear);
  front.append(header, body, actions, footer);

  let inputEl: HTMLInputElement | null = null;
  let placeholder = "";
  let streamRegion: HTMLElement | null = null;

  // --- header (title + widget switcher) ---------------------------------
  function buildHeader(data: PanelData): void {
    header.textContent = "";
    const title = data.card?.title ?? data.spec?.name ?? "Hermes";
    header.append(el("div", "panel-title", title));

    if (data.widgets.length > 1) {
      const switcher = el("div", "switcher");
      for (const w of data.widgets) {
        const dot = el("button", "switch-dot");
        dot.type = "button";
        dot.setAttribute("aria-label", w.name);
        if (w.id === data.activeId) dot.classList.add("active");
        if ((data.urgencies[w.id] ?? "normal") === "urgent") {
          dot.classList.add("urgent");
        }
        dot.addEventListener("click", () => ctx.onSelectWidget(w.id));
        switcher.append(dot);
      }
      header.append(switcher);
    }
  }

  // --- body -------------------------------------------------------------
  function buildBody(data: PanelData): void {
    body.textContent = "";
    streamRegion = null;

    if (!data.spec) {
      // Zero widgets installed.
      const hint = el(
        "div",
        "hint",
        "No widgets installed. Drop one in ~/.hermes/notch-widgets/ and rescan.",
      );
      body.append(hint);
      const row = el("button", "link-row");
      row.type = "button";
      row.append(el("span", "link-ico", "↗"));
      row.append(el("span", "link-label", "How to build a widget"));
      row.addEventListener("click", () => ctx.openLink(data.docsUrl));
      body.append(row);
      return;
    }

    const card = data.card;
    if (card && (card.body?.length ?? 0) > 0) {
      body.append(renderBlocks(card, renderCtx));
    } else if (card?.status?.state === "error") {
      body.append(el("div", "error-line", card.status.detail || "Something broke."));
    } else {
      body.append(el("div", "hint", card?.glance?.text ?? "Nothing to show."));
    }
  }

  // --- actions ----------------------------------------------------------
  function buildActions(data: PanelData): void {
    actions.textContent = "";
    const spec = data.spec;
    if (!spec?.actions || spec.actions.length === 0) {
      actions.classList.add("empty");
      return;
    }
    const enabled = data.card?.actions_enabled;
    const list = enabled
      ? spec.actions.filter((a) => enabled.includes(a.id))
      : spec.actions;
    if (list.length === 0) {
      actions.classList.add("empty");
      return;
    }
    actions.classList.remove("empty");
    for (const action of list) actions.append(buildButton(action));
  }

  function buildButton(action: WidgetAction): HTMLButtonElement {
    const style = action.style ?? "default";
    const btn = el("button", `btn ${style}`);
    btn.type = "button";
    if (action.icon) btn.append(el("span", "btn-ico", action.icon));
    btn.append(el("span", "btn-label", action.label));
    btn.addEventListener("click", () => ctx.onAction(action.id));
    return btn;
  }

  // --- footer input -----------------------------------------------------
  function buildInputLine(): void {
    footerLeft.textContent = "";
    inputEl = null;
    if (!placeholder) return;
    const input = el("input", "input");
    input.type = "text";
    input.placeholder = placeholder;
    input.autocomplete = "off";
    input.spellcheck = false;
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        const value = input.value.trim();
        if (value) {
          input.value = "";
          ctx.onSubmit(value);
        }
      }
    });
    const line = el("div", "input-line");
    line.append(input);
    footerLeft.append(line);
    inputEl = input;
  }

  function buildWorking(): void {
    footerLeft.textContent = "";
    inputEl = null;
    const working = el("div", "working");
    working.append(el("span", "wd"), el("span", "wd"), el("span", "wd"));
    footerLeft.append(working);
  }

  // --- public API -------------------------------------------------------
  function update(data: PanelData): void {
    placeholder = data.spec?.input?.placeholder ?? "";
    buildHeader(data);
    buildBody(data);
    buildActions(data);
    buildInputLine();
  }

  function playEntrance(): void {
    front.classList.remove("entering");
    // reflow to restart the animation
    void front.offsetWidth;
    front.classList.add("entering");
    window.setTimeout(() => front.classList.remove("entering"), 500);
  }

  function setSettings(settings: Settings, sctx: SettingsContext): void {
    back.textContent = "";
    back.append(renderSettings(settings, sctx));
  }

  function showSettings(show: boolean): void {
    root.classList.toggle("flipped", show);
  }

  function showWorking(): void {
    buildWorking();
  }

  function appendDelta(text: string): void {
    if (!streamRegion) {
      body.textContent = "";
      streamRegion = el("div", "stream");
      body.append(streamRegion);
    }
    streamRegion.append(document.createTextNode(text));
    body.scrollTop = body.scrollHeight;
  }

  function showComplete(markdown: string): void {
    body.textContent = "";
    streamRegion = null;
    if (markdown.trim()) {
      body.append(renderMarkdown(markdown, renderCtx));
    } else {
      body.append(el("div", "hint", "Done."));
    }
    buildInputLine();
  }

  function showError(message: string): void {
    streamRegion = null;
    // Replace any working indicator with the quiet input line again.
    buildInputLine();
    const line = el("div", "error-line", message);
    body.append(line);
  }

  return {
    el: root,
    update,
    playEntrance,
    setSettings,
    showSettings,
    showWorking,
    appendDelta,
    showComplete,
    showError,
    isInputFocused: () => inputEl !== null && document.activeElement === inputEl,
    focusInput: () => inputEl?.focus(),
    hasInput: () => placeholder !== "",
  };
}
