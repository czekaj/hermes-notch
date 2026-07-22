# Hermes Notch — Protocol

Three layers, documented top-down:

1. [Host HTTP API](#1-host-http-api) — the app ↔ hermes-notch plugin (widgets, Cards, script actions)
2. [Chat transport](#2-chat-transport) — the app ↔ Hermes gateway WebSocket (chat-backed widgets)
3. [App-internal contract](#3-app-internal-contract) — Tauri commands/events between the Rust core and the webview

The Hermes host runs `hermes serve` (or `hermes dashboard`) — a FastAPI server,
default `127.0.0.1:9119`. For remote use it must be started with
`--host 0.0.0.0`, which **requires** `dashboard.basic_auth` to be configured in
`~/.hermes/config.yaml` (the server refuses public binds without an auth
provider). The plugin must be listed under `plugins.enabled`.

## 1. Host HTTP API

Base: `http(s)://<host>:<port>`. All routes below require auth (§1.1).

### 1.1 Auth

Two modes; the app picks by whether a session token is configured:

**Password mode (remote, normal case)**
```
POST /auth/password-login
{"provider": "basic", "username": "<u>", "password": "<p>", "next": "/"}
→ 200 {"ok": true, "next": "/"}  + Set-Cookie: hermes_session_at=…; hermes_session_rt=…
```
Send `hermes_session_at` (cookie) on every request. On 401, re-login once.

**Loopback token mode (same-machine dev)**
Header `X-Hermes-Session-Token: <token>` where the token is the value of
`HERMES_DASHBOARD_SESSION_TOKEN` the server was launched with.

### 1.2 Widget host routes (`/api/plugins/hermes-notch`)

| Route | Returns |
|---|---|
| `GET /health` | `{ok, host_version, widgets, widget_errors, widgets_dir, ts}` |
| `GET /widgets` | `{widgets: [{id, spec} \| {id, error}], scanned_at, dir}` |
| `GET /widgets/{id}/glance` | Card — cheap, poll this |
| `GET /widgets/{id}/state` | Card — full source run |
| `POST /widgets/{id}/action` `{"id": "<action-id>"}` | Card from a `script` effect; `409` for `chat` effects (those go over §2) |
| `POST /rescan` | `{ok, widgets: [ids]}` |
| `GET /spec` | WIDGET_SPEC.md (text/markdown) |

Cards from a `chat`-source widget carry an extra field:
```json
"chat": {"on_start": "/adhd", "session_tag": "notch:adhd-focus"}
```
signalling that body/actions ride the chat transport.

## 2. Chat transport

JSON-RPC 2.0 over WebSocket at `/api/ws`. This is the dashboard's own gateway
protocol; Notch is just another client.

**Connect (password mode):** mint a single-use ticket, then dial:
```
POST /api/auth/ws-ticket           (cookie-authed) → {"ticket": "…", "ttl_seconds": 30}
ws://<host>:<port>/api/ws?ticket=<ticket>
```
**Connect (loopback token mode):** `ws://…/api/ws?token=<session-token>`.

On open the server pushes `{"method":"event","params":{"type":"gateway.ready",…}}`.
Requests are `{"jsonrpc":"2.0","id":n,"method":…,"params":{…}}`.

### 2.1 Methods used by Notch

| Method | Params | Notes |
|---|---|---|
| `session.create` | `{title}` | → `{session_id, stored_session_id, …}` — title it with the widget's `session_tag` |
| `session.resume` | `{session_id}` | reattach across restarts (follows compression chain) |
| `session.history` | `{session_id}` | → `{messages}` — restore the last agent reply on startup |
| `session.status` | `{session_id}` | idle/running |
| `prompt.submit` | `{session_id, text}` | verbatim text; → `{"status":"streaming"}` then events |
| `session.interrupt` | `{session_id}` | cancel the running turn |
| `slash.exec` | `{command, session_id}` | first stop for `/name` inputs |
| `command.dispatch` | `{name, arg, session_id}` | quick_commands / skills resolution |

### 2.2 Streamed events

`{"method":"event","params":{"type":…,"session_id":…,"payload":{…}}}` — filter
by `session_id`. Types Notch renders: `message.start`, `message.delta`
(`payload.text` chunk), `message.complete` (`payload.text` full),
`status.update`, `error`. Others (`tool.start`, `reasoning.delta`, …) map to a
subtle "working…" indicator.

### 2.3 Slash resolution (client-side, required for `/adhd`)

`prompt.submit` does **not** expand aliases. For input starting with `/`:

```
parse "/name arg…"
→ try slash.exec {command: name, session_id}; if it succeeds, done
→ else command.dispatch {name, arg, session_id}:
    {"type":"alias","target":T}      → recurse on "/T arg…"
    {"type":"exec","output":…}       → render output directly
    result with a "message" field    → prompt.submit {session_id, text: message}
```

Non-slash text (`done`, `skip`, anything typed) is one `prompt.submit`.

### 2.4 Session ownership

One session per chat widget, titled with its `session_tag` (`notch:<id>`).
Persist `stored_session_id` locally; resume on reconnect, create on failure.
Never inject into sessions Notch didn't create — the Discord gateway may be
running them in another process.

## 3. App-internal contract

Fixed interface between the Rust core (owns ALL network I/O — the webview
never fetches; this sidesteps CORS and keeps cookies/tickets in one place) and
the frontend. All commands return `Result`; errors are strings.

### 3.1 Commands (frontend → Rust)

```ts
// settings — persisted via tauri-plugin-store ("settings.json")
get_settings(): Settings
set_settings(patch: Partial<Settings>): Settings
// Settings = { host: string, port: number, username: string, password: string,
//              token: string,           // loopback dev mode; "" = password mode
//              autostart: boolean }

// connection lifecycle
connect(): HostInfo          // login → /health → /widgets; starts WS; idempotent
disconnect(): void
// HostInfo = { ok: boolean, host_version: string, widgets: WidgetSpec[] }

// cards
get_glance(widgetId: string): Card
get_state(widgetId: string): Card
run_action(widgetId: string, actionId: string): Card | null  // script effects only

// chat
chat_ensure(widgetId: string): ChatStatus   // resume-or-create session; NEVER runs a turn
chat_send(widgetId: string, text: string): void
// slash chain per §2.3; replies stream as events. Free text into a session
// with no prior turns is prefixed with the widget's on_start command
// ("/adhd veto that") so the skill arrives with the input in ONE turn.
// Action buttons should send self-sufficient slash commands.
chat_history(widgetId: string): string      // last assistant message markdown ("" if none)
chat_interrupt(widgetId: string): void
chat_reset(widgetId: string): void          // forget the session; next send creates fresh
// ChatStatus = { session_id: string, fresh: boolean }

// panel geometry (Rust owns the NSPanel frame, anchored to the notch top-center)
set_expanded(expanded: boolean,
             width?: number, height?: number): void
// width/height = the measured size (offset metrics) of the visible CSS shape
// (collapsed pill or expanded panel). The window is sized to match EXACTLY:
// the vibrancy layer fills the window, so excess area renders as bare glass
// and a too-small window clips the shape. The frontend re-reports on every
// paint and content resize (ResizeObserver).
panel_info(): PanelInfo
// PanelInfo = { has_notch: boolean, notch_width: number, notch_height: number, scale: number }

// utilities
open_url(url: string): void                  // opener plugin, on the viewing Mac
copy_text(text: string): void                // clipboard-manager
```

### 3.2 Events (Rust → frontend)

```ts
"notch:hover"      { entered: boolean }        // tracking-area enter/exit
"notch:shortcut"   {}                          // global shortcut toggled
"conn:status"      { state: "disconnected"|"connecting"|"connected"|"error", detail?: string }
"chat:event"       { widgetId: string,
                     kind: "start"|"delta"|"complete"|"status"|"error",
                     text?: string }           // delta = chunk, complete = full markdown
```

### 3.3 Frontend responsibilities

- Poll `get_glance` per widget at `refresh.interval` (`while_visible` when the
  panel is expanded); render Cards per WIDGET_SPEC.md exactly.
- Convert completed chat markdown into Card-equivalent blocks locally
  (fenced code → copy chips, bare/markdown links → link rows, rest → md).
- Drive expand/collapse: call `set_expanded` then run the CSS transition
  (§DESIGN.md); collapse on mouse-exit grace timeout (350 ms) unless an input
  is focused.
- Escape hatch: `Esc` collapses; the global shortcut toggles.
