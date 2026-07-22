# Hermes Notch — Widget Spec v1

This document is the **complete contract** for Hermes Notch widgets. It is written
so that a human *or an AI agent* can produce a working widget from it without
reading the host source code. If you are an agent asked to "widgetize" a skill,
a prompt, or a data source: read this file, then emit a widget directory as
described in [Authoring](#authoring-a-widget). The
[`notch-widgetizer`](../skills/notch-widgetizer/SKILL.md) skill automates this.

## Mental model

```
┌────────────── Notch app (Tauri, remote Mac) ──────────────┐
│  renders Cards, fires Actions — knows nothing else        │
└──────────────────────────┬────────────────────────────────┘
                           │ HTTP (see PROTOCOL.md)
┌──────────────────────────┴────────────────────────────────┐
│  Hermes host: notch plugin (widget host)                  │
│  · scans widget dirs, serves specs                        │
│  · runs each widget's SOURCE → normalizes to a CARD       │
│  · executes ACTIONS (chat injection, scripts)             │
└───────────────────────────────────────────────────────────┘
```

Three ideas, strictly separated:

1. **Widget spec** (`widget.json`) — static declaration: identity, where state
   comes from, how often to refresh, which actions exist. Never contains state.
2. **Card** — the render-ready state object a source emits. The app renders
   Cards with native fidelity (copy chips, links, buttons, progress) and adds
   nothing. If it isn't in the Card or the spec, it doesn't exist on screen.
3. **Action** — a button press or text submission sent back to the host, which
   executes its declared effect (inject a chat message into a Hermes session,
   run a script) and returns the refreshed Card.

The intelligence lives at the edges: a deterministic script (or a Hermes agent
session) produces the Card; the app is a dumb, pretty renderer. Keep it that way.

## Widget directory layout

A widget is a directory installed at `~/.hermes/notch-widgets/<widget-id>/`:

```
adhd-focus/
├── widget.json      # required — the spec
├── state.py         # optional — script source (any executable works)
└── README.md        # optional — human notes
```

`<widget-id>` must match `widget.json .id` and `^[a-z0-9][a-z0-9-]{1,40}$`.

## `widget.json`

```jsonc
{
  "spec": 1,                    // spec version — always 1 for now
  "id": "adhd-focus",
  "name": "Focus",              // short label shown in the HUD tab strip
  "icon": "🎯",                 // single emoji
  "description": "One step at a time over the personal task universe.",

  // Where Cards come from — exactly one of the source types below.
  "source": { ... },

  // Polling hints for the app (seconds). The host may also push via SSE later.
  "refresh": { "interval": 30, "while_visible": 10 },

  // Actions the app may show. The Card can enable/disable/re-order per state.
  "actions": [ ... ],

  // Optional free-text input line (e.g. to talk to a chat-backed widget).
  "input": { "placeholder": "tell Hermes…", "effect": { "type": "chat" } }
}
```

### Source types

**`script`** — the workhorse. The host runs the command from the widget
directory with a timeout (default 20 s) and parses stdout as a Card JSON.
Deterministic scripts beat model calls; prefer this type whenever the state can
be computed from files, databases, or APIs on the Hermes host.

```jsonc
"source": {
  "type": "script",
  "command": ["python3", "state.py"],   // argv, run with cwd = widget dir
  "timeout": 20
}
```

**`chat`** — the widget is a live Hermes agent session. The host lazily opens a
dedicated session (tagged `notch:<widget-id>`), sends `on_start` as the first
message (aliases like `/adhd` resolve exactly as they do on Discord), and
renders the **latest agent reply** as the Card body (markdown is translated:
fenced code → copy blocks, links → link blocks, the rest → md blocks). Chat
`actions` inject their text into the same session.

```jsonc
"source": {
  "type": "chat",
  "on_start": "/adhd",
  "glance": { "type": "script", "command": ["python3", "state.py", "--glance"] }
}
```

The optional `glance` sub-source lets a cheap script provide the collapsed
one-liner without waking the agent — recommended, since glances are polled
frequently.

**`file`** — serve a JSON file that something else (a cron job, a skill) keeps
fresh. The file must contain a Card.

```jsonc
"source": { "type": "file", "path": "~/.hermes/data/my-widget-card.json" }
```

### Actions

```jsonc
"actions": [
  { "id": "done",    "label": "Done",    "icon": "✓", "style": "primary",
    "effect": { "type": "chat", "text": "done" } },
  { "id": "skip",    "label": "Skip",    "icon": "→",
    "effect": { "type": "chat", "text": "skip" } },
  { "id": "smaller", "label": "Smaller", "icon": "✂",
    "effect": { "type": "chat", "text": "smaller" } },
  { "id": "refresh", "label": "Refresh",
    "effect": { "type": "script", "command": ["python3", "state.py", "--refresh"] } }
]
```

Effect types executed by the **host**:

| type     | payload                          | behavior |
|----------|----------------------------------|----------|
| `chat`   | `text` (omit to use user input)  | inject into the widget's Hermes session; reply becomes the next Card |
| `script` | `command`, optional `timeout`    | run in widget dir; stdout (if JSON Card) replaces the Card |

Effect types executed by the **app** (never reach the host): every `copy` and
`link` block in a Card body is automatically copyable/clickable — you do not
declare client-side actions.

`style` is one of `primary`, `default`, `danger`. Buttons render in declared
order; a Card may narrow them per state via `actions_enabled`.

## The Card object

What sources emit. Everything is optional except `glance`.

```jsonc
{
  "card": 1,                       // card schema version
  "glance": {
    "text": "Renew passport: book appointment",   // ≤ 60 chars, the collapsed line
    "detail": "~5 min · step 2 of 4",          // ≤ 40 chars, dimmed suffix
    "urgency": "normal"                        // normal | attention | urgent
  },
  "title": "Step 2 of 4",          // expanded-card header (defaults to widget name)
  "body": [                        // ordered blocks, rendered top to bottom
    { "type": "text",  "text": "Book the passport renewal appointment — the form takes ~3 min." },
    { "type": "link",  "label": "Passport appointment portal", "url": "https://example.gov/appointments" },
    { "type": "copy",  "label": "Reference number", "value": "REF-48219" },
    { "type": "copy",  "label": "Command", "value": "remctl done 42", "mono": true },
    { "type": "kv",    "items": [["Source", "todoist"], ["Est", "~5 min"]] },
    { "type": "progress", "value": 2, "max": 4, "label": "batch" },
    { "type": "md",    "text": "Anything richer — *markdown subset*: bold, italics, `code`, lists, links." },
    { "type": "divider" }
  ],
  "actions_enabled": ["done", "skip", "smaller"],  // omit → all declared actions
  "status": { "state": "ok", "detail": "" },       // ok | stale | error
  "ts": "2026-07-22T19:04:11Z"                     // when this state was computed
}
```

Block types (exhaustive for v1): `text`, `md`, `copy`, `link`, `kv`,
`progress`, `divider`. Unknown block types are skipped by the renderer, so the
schema can grow without breaking old apps — but emit only these seven from a
v1 widget.

Renderer guarantees:

- `copy` blocks get a one-tap copy chip (`mono: true` renders monospace).
- `link` blocks open in the default browser on the *viewing* Mac.
- `md` supports: bold, italic, inline code, fenced code (becomes a copy
  block), bullet lists, links. Nothing else — no images, no tables, no HTML.
- `glance.urgency: "urgent"` tints the collapsed pill; use it sparingly or it
  becomes noise.

Error convention: on failure emit a valid Card with
`status: {"state": "error", "detail": "<one line>"}` and a glance that says
what broke. Exit codes and stderr are logged host-side but never rendered.

## Authoring a widget

Checklist — an agent following these steps produces a valid widget:

1. **Pick the source type.** Is the state computable by a deterministic
   script from files/APIs on the Hermes host? → `script` (preferred). Is the
   widget inherently conversational (needs agent judgment per interaction)?
   → `chat`, ideally with a `glance` script. Does a cron job already produce
   the data? → `file`.
2. **Design the glance first.** One line, ≤ 60 chars, that is worth pinning
   next to the camera. If you can't compress the state to one line, the
   widget is probably two widgets.
3. **Write `widget.json`** against the schema above. Validate:
   `python3 <plugin-dir>/validate_widget.py <widget-dir>`.
4. **Write the state script** (if `script`/`glance`). Rules: stdout is
   exactly one JSON Card; every network/subprocess failure is caught and
   becomes a `status: error` Card; runtime < 5 s typical, 20 s hard cap;
   no secrets in output — the Card crosses the network.
5. **Map interactions to actions.** Every protocol word the underlying
   skill/workflow understands (`done`, `skip`, …) becomes a `chat` action;
   mechanical operations become `script` actions. 3–5 buttons max — this is
   a HUD, not a dashboard.
6. **Install**: place the directory in `~/.hermes/notch-widgets/` and hit
   `POST /api/plugins/hermes-notch/rescan` (or restart the dashboard).

### Widgetizing an existing skill — translation heuristics

Given a `SKILL.md` (or a raw prompt), derive the widget mechanically:

- The skill's **"on invocation" data pipeline** (aggregator scripts, ranked
  candidates) → the `glance`/`state` script. Reuse the skill's own scripts;
  never re-implement ranking in the widget.
- The skill's **interaction protocol table** (user says X → agent does Y) →
  `chat` actions, one per protocol word, verbatim text.
- The skill's **invocation alias** (`/adhd` style `quick_commands`) →
  `source.on_start`.
- The skill's **persistent state file** (queue files, briefing outputs) →
  what the glance script parses. State files beat re-running aggregators.
- Anything requiring **judgment mid-interaction** stays chat-backed; anything
  deterministic gets promoted to a script. When in doubt: glance = script,
  body + actions = chat.

## Versioning

`spec` / `card` integers bump only on breaking changes. Hosts serve widgets of
any spec version they support; apps skip Cards with a `card` version above
what they render. Additive fields never bump versions.
