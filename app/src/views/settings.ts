// Settings is a card like any other: host, port, username, password, token,
// autostart — quiet underlined fields, one column, a single Connect primary
// button. No modal, no separate preferences window. The gear flips to this;
// the back arrow flips back.

import type { Settings } from "../types";
import { el } from "../render/dom";

const BACK_GLYPH = "‹"; // ‹

export interface SettingsContext {
  onConnect: (values: Settings) => void;
  onBack: () => void;
  canBack: boolean;
  /** Present when the active widget is chat-backed: label + reset handler. */
  resetLabel?: string;
  onReset?: () => void;
}

export function renderSettings(
  settings: Settings,
  ctx: SettingsContext,
): HTMLElement {
  const root = el("div", "settings");

  // Header: optional back arrow + title.
  const header = el("div", "settings-header");
  if (ctx.canBack) {
    const back = el("button", "iconbtn back");
    back.type = "button";
    back.textContent = BACK_GLYPH;
    back.setAttribute("aria-label", "Back");
    back.addEventListener("click", ctx.onBack);
    header.append(back);
  }
  header.append(el("div", "settings-title", "Connection"));
  root.append(header);

  const host = textField("Host", settings.host, "text", "127.0.0.1");
  const port = textField("Port", String(settings.port ?? 9119), "number", "9119");
  const username = textField("Username", settings.username, "text", "");
  const password = textField("Password", settings.password, "password", "");
  const token = textField("Session token (dev)", settings.token, "text", "loopback only");

  root.append(host.field, port.field, username.field, password.field, token.field);

  // Autostart toggle.
  const toggleRow = el("label", "field toggle-row");
  toggleRow.append(el("span", "field-label", "Launch at login"));
  const toggle = el("input");
  toggle.type = "checkbox";
  toggle.className = "toggle";
  toggle.checked = settings.autostart === true;
  toggleRow.append(toggle);
  const track = el("span", "toggle-track");
  toggleRow.append(track);
  root.append(toggleRow);

  // Connect.
  const connect = el("button", "btn primary connect");
  connect.type = "button";
  connect.textContent = "Connect";
  const submit = () => {
    const portNum = parseInt(port.input.value, 10);
    ctx.onConnect({
      host: host.input.value.trim(),
      port: Number.isFinite(portNum) ? portNum : 9119,
      username: username.input.value,
      password: password.input.value,
      token: token.input.value.trim(),
      autostart: toggle.checked,
    });
  };
  connect.addEventListener("click", submit);
  root.append(connect);

  // Danger-styled plain-text reset for the active chat widget's session —
  // destructive actions never get a filled button in a HUD (DESIGN §Buttons).
  if (ctx.onReset && ctx.resetLabel) {
    const reset = el("button", "btn danger reset-session");
    reset.type = "button";
    reset.textContent = ctx.resetLabel;
    reset.addEventListener("click", ctx.onReset);
    root.append(reset);
  }

  // Enter in any field submits.
  root.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      submit();
    }
  });

  return root;
}

function textField(
  label: string,
  value: string,
  type: "text" | "password" | "number",
  placeholder: string,
): { field: HTMLElement; input: HTMLInputElement } {
  const field = el("label", "field");
  field.append(el("span", "field-label", label));
  const input = el("input", "field-input");
  input.type = type;
  input.value = value ?? "";
  input.placeholder = placeholder;
  input.autocomplete = "off";
  input.spellcheck = false;
  field.append(input);
  return { field, input };
}
