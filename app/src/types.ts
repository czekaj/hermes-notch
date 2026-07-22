// Shared contract types. Derived directly from:
//   docs/WIDGET_SPEC.md  (Card, WidgetSpec, blocks, actions)
//   docs/PROTOCOL.md §3  (Settings, HostInfo, ChatStatus, PanelInfo, events)
// These are the only place `any` would otherwise creep in; the invoke boundary
// (api.ts) casts wire values into these shapes and nothing else uses `any`.

export type Urgency = "normal" | "attention" | "urgent";

export interface Glance {
  text: string;
  detail?: string;
  urgency?: Urgency;
}

// --- Card body blocks (exhaustive for v1) --------------------------------
export interface TextBlock {
  type: "text";
  text: string;
}
export interface MdBlock {
  type: "md";
  text: string;
}
export interface CopyBlock {
  type: "copy";
  label?: string;
  value: string;
  mono?: boolean;
}
export interface LinkBlock {
  type: "link";
  label?: string;
  url: string;
}
export interface KvBlock {
  type: "kv";
  items: Array<[string, string]>;
}
export interface ProgressBlock {
  type: "progress";
  value: number;
  max: number;
  label?: string;
}
export interface DividerBlock {
  type: "divider";
}

export type Block =
  | TextBlock
  | MdBlock
  | CopyBlock
  | LinkBlock
  | KvBlock
  | ProgressBlock
  | DividerBlock;

export type CardState = "ok" | "stale" | "error";
export interface CardStatus {
  state: CardState;
  detail?: string;
}

export interface CardChat {
  on_start?: string;
  session_tag?: string;
}

export interface Card {
  card?: number;
  glance: Glance;
  title?: string;
  body?: Block[];
  actions_enabled?: string[];
  status?: CardStatus;
  chat?: CardChat;
  ts?: string;
}

// --- Widget spec (widget.json) -------------------------------------------
export type ActionStyle = "primary" | "default" | "danger";

export interface ChatEffect {
  type: "chat";
  text?: string;
}
export interface ScriptEffect {
  type: "script";
  command: string[];
  timeout?: number;
}
export type ActionEffect = ChatEffect | ScriptEffect;

export interface WidgetAction {
  id: string;
  label: string;
  icon?: string;
  style?: ActionStyle;
  effect: ActionEffect;
}

export interface WidgetInput {
  placeholder?: string;
  effect: { type: "chat" };
}

export interface WidgetRefresh {
  interval?: number;
  while_visible?: number;
}

export interface WidgetSource {
  type: "script" | "chat" | "file";
  [key: string]: unknown;
}

export interface WidgetSpec {
  spec: number;
  id: string;
  name: string;
  icon: string;
  description?: string;
  source: WidgetSource;
  refresh?: WidgetRefresh;
  actions?: WidgetAction[];
  input?: WidgetInput;
}

// --- App-internal contract (PROTOCOL §3) ---------------------------------
export interface Settings {
  host: string;
  port: number;
  username: string;
  password: string;
  token: string;
  autostart: boolean;
}

export interface HostInfo {
  ok: boolean;
  host_version: string;
  widgets: WidgetSpec[];
}

export interface ChatStatus {
  session_id: string;
  fresh: boolean;
}

export interface PanelInfo {
  has_notch: boolean;
  notch_width: number;
  notch_height: number;
  scale: number;
}

export type ConnState = "disconnected" | "connecting" | "connected" | "error";

export interface ConnStatusEvent {
  state: ConnState;
  detail?: string;
}

export type ChatEventKind = "start" | "delta" | "complete" | "status" | "error";
export interface ChatEvent {
  widgetId: string;
  kind: ChatEventKind;
  text?: string;
}

export interface HoverEvent {
  entered: boolean;
}
