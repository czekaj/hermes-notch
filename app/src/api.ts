// Typed wrappers over the Tauri command/event surface (PROTOCOL.md §3).
//
// Command names are snake_case exactly as documented. Argument keys are
// camelCase — Tauri v2 converts them to the Rust snake_case parameter names.
//
// The webview never touches the network itself; every call here crosses into
// the Rust core. In a plain browser (no `window.__TAURI_INTERNALS__`) we fall
// back to the dev mock (src/devmock.ts), which is dynamically imported so the
// import.meta.env.DEV guard tree-shakes it out of the production bundle.

import type {
  Card,
  ChatStatus,
  ConnStatusEvent,
  ChatEvent,
  HostInfo,
  HoverEvent,
  PanelInfo,
  Settings,
} from "./types";

type UnlistenFn = () => void;
type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type ListenFn = <T>(event: string, cb: (payload: T) => void) => Promise<UnlistenFn>;

const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let _invoke: InvokeFn;
let _listen: ListenFn;
const ready: Promise<void> = (async () => {
  if (IS_TAURI) {
    const core = await import("@tauri-apps/api/core");
    const evt = await import("@tauri-apps/api/event");
    _invoke = (cmd, args) => core.invoke(cmd, args);
    _listen = async (event, cb) =>
      evt.listen(event, (e) => cb(e.payload as never));
    return;
  }
  if (import.meta.env.DEV) {
    const mock = await import("./devmock");
    const backend = mock.createMockBackend();
    _invoke = backend.invoke;
    _listen = backend.listen;
    return;
  }
  // Production build with no Tauri host: fail loudly rather than silently.
  _invoke = () => Promise.reject(new Error("Tauri backend unavailable"));
  _listen = () => Promise.reject(new Error("Tauri backend unavailable"));
})();

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await ready;
  return _invoke<T>(cmd, args);
}

async function subscribe<T>(
  event: string,
  cb: (payload: T) => void,
): Promise<UnlistenFn> {
  await ready;
  return _listen<T>(event, cb);
}

// --- settings ------------------------------------------------------------
export const getSettings = (): Promise<Settings> => call("get_settings");
export const setSettings = (patch: Partial<Settings>): Promise<Settings> =>
  call("set_settings", { patch });

// --- connection lifecycle ------------------------------------------------
export const connect = (): Promise<HostInfo> => call("connect");
export const disconnect = (): Promise<void> => call("disconnect");

// --- cards ---------------------------------------------------------------
export const getGlance = (widgetId: string): Promise<Card> =>
  call("get_glance", { widgetId });
export const getState = (widgetId: string): Promise<Card> =>
  call("get_state", { widgetId });
export const runAction = (
  widgetId: string,
  actionId: string,
): Promise<Card | null> => call("run_action", { widgetId, actionId });

// --- chat ----------------------------------------------------------------
export const chatEnsure = (widgetId: string): Promise<ChatStatus> =>
  call("chat_ensure", { widgetId });
export const chatSend = (widgetId: string, text: string): Promise<void> =>
  call("chat_send", { widgetId, text });
export const chatHistory = (widgetId: string): Promise<string> =>
  call("chat_history", { widgetId });
export const chatInterrupt = (widgetId: string): Promise<void> =>
  call("chat_interrupt", { widgetId });
export const chatReset = (widgetId: string): Promise<void> =>
  call("chat_reset", { widgetId });

// --- panel geometry ------------------------------------------------------
// width/height are the measured size of the visible CSS shape (pill or panel);
// the Rust side sizes the window to match exactly — the vibrancy layer fills
// the window, so any excess renders as a bare frosted-glass rectangle.
export const setExpanded = (
  expanded: boolean,
  width?: number,
  height?: number,
): Promise<void> => call("set_expanded", { expanded, width, height });
export const panelInfo = (): Promise<PanelInfo> => call("panel_info");

// --- utilities -----------------------------------------------------------
export const openUrl = (url: string): Promise<void> => call("open_url", { url });
export const copyText = (text: string): Promise<void> =>
  call("copy_text", { text });

// --- events (Rust → frontend) --------------------------------------------
export const onHover = (cb: (e: HoverEvent) => void): Promise<UnlistenFn> =>
  subscribe("notch:hover", cb);
export const onShortcut = (cb: () => void): Promise<UnlistenFn> =>
  subscribe("notch:shortcut", () => cb());
export const onConnStatus = (
  cb: (e: ConnStatusEvent) => void,
): Promise<UnlistenFn> => subscribe("conn:status", cb);
export const onChatEvent = (cb: (e: ChatEvent) => void): Promise<UnlistenFn> =>
  subscribe("chat:event", cb);
