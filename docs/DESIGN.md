# Hermes Notch — Design Language

The bar: sleek, modern, uncluttered, functional, beautiful, visually
glanceable. The HUD must feel like Apple shipped it — like the notch itself
learned a trick, not like a web page taped to the screen. Every widget
inherits this language automatically because the app renders all Cards; widget
authors control content, never chrome.

## Core metaphor

The collapsed state IS the notch: a pure-black extension growing seamlessly
from the camera island, with the same corner radius the notch has (10 px
bottom corners). No border, no shadow in collapsed state — it must be
indistinguishable from hardware until it has something to say. On displays
without a notch, the same black pill floats at top-center, full radius.

```
            ┌───────────[ ▪ camera island ▪ ]───────────┐
            └──╮ 🎯 Renew passport: book appt · ~5m ╭──┘   ← collapsed: +28px height
               ╰────────────────────────────────────────╯
                              hover ↓
            ┌───────────[ ▪ camera island ▪ ]────────────┐
            │                                            │
            │   Step 2 of 4                    ● ● ○     │  ← expanded: 420×~300
            │   Book the passport renewal appointment…   │
            │   ┌──────────────────────────────────┐     │
            │   │ ⧉  remctl done 42                │     │  ← copy chip
            │   └──────────────────────────────────┘     │
            │   ↗ Passport appointment portal            │  ← link row
            │   ▓▓▓▓▓▓▓▓░░░░░░░  batch 2/4               │
            │                                            │
            │   [ ✓ Done ]  [ → Skip ]  [ ✂ Smaller ]    │
            │   ‹ tell Hermes… ›                    ⚙    │
            ╰────────────────────────────────────────────╯
```

## Rules

**Color.** Collapsed: `#000` always, both modes — the notch is black in light
mode too. Expanded: layered on macOS HUD vibrancy (the Rust side applies
`NSVisualEffectMaterial::HudWindow`); CSS paints `rgba(18,18,20,0.72)` in dark
and `rgba(242,242,247,0.78)` in light over it, so the desktop glows through
faintly. Text: `rgba(255,255,255,0.92)` / secondary `0.55` (dark);
`rgba(0,0,0,0.88)` / `0.5` (light). Accent: `AccentColor` CSS keyword with
`#0A84FF` fallback — never a brand color. Urgency tints are the only other
color: attention = system orange, urgent = system red, applied as a 3 px glance
underline dot, never as fills.

**Typography.** `-apple-system` exclusively. Glance: 12 px / 500 weight,
detail suffix 11 px / 400 at secondary opacity. Card title: 13 px / 600.
Body: 13 px / 400, line-height 1.45. Mono (copy chips, code):
`ui-monospace, "SF Mono"` 11.5 px. Nothing bigger than 15 px, ever — this is a
HUD, not a website. `-webkit-font-smoothing: antialiased`.

**Space.** 8 px grid. Panel padding 16 px, block gap 10 px, button gap 8 px.
One column, no sidebars, no tables. Max 7 blocks visible; overflow scrolls
with hidden scrollbars and a bottom fade mask. Empty states are one quiet
line, not illustrations.

**Motion.** One spring: `cubic-bezier(0.32, 0.72, 0, 1)`. Expand: 280 ms —
panel scales from the notch (transform-origin: top center) while content
fades+rises 8 px with 60 ms stagger. Collapse: 200 ms, no stagger. Glance text
changes: crossfade 160 ms. Copy confirmation: chip flashes to a checkmark for
900 ms. Honor `prefers-reduced-motion` by replacing all of it with 80 ms
opacity fades. Nothing else moves; no pulsing, no parallax, no confetti.

**Glanceability.** The collapsed strip answers "what should I do next" in one
saccade: icon + one line + dimmed detail. No counters ticking, no marquees; if
text overflows, it truncates with an ellipsis (design-to-fit is the widget
author's job, spec §glance). Multiple widgets: the strip shows the
highest-urgency glance; the expanded header shows a dot per widget (active =
accent, urgent = red) for switching. Never a tab bar of labels.

**Buttons.** Pill, 26 px tall, 12 px side padding, 11.5 px / 590 weight
labels. `primary` = accent fill, white text. `default` = white/black 8%
overlay, no border. `danger` = plain text in system red — destructive actions
never get a big red button in a HUD. Icons inline before labels, from the
action's declared glyph. Hover = +4% overlay, press = scale 0.97. Max one
`primary` visible per card.

**Interaction.** Hover over the strip expands; mouse-out collapses after a
350 ms grace (unless the text input has focus). `Esc` always collapses. The
global shortcut (default `⌥Space`, rebindable) toggles from anywhere. Copy
chips copy on single click. Links open on single click and collapse the panel.
The free-text input is a single quiet line at the bottom — placeholder from the
widget spec, no send button (Enter sends), agent "working…" shown as three
soft dots in the input's place.

**States.** Connecting: strip shows `— connecting to hermes —` at secondary
opacity. Error/unreachable: strip shows the host name + a red dot, expanded
panel becomes the settings form. `status.stale` Cards get a small clock glyph
before the glance detail. Streaming replies render live (delta text appended
raw, converted to blocks on `complete`).

**Settings** is a card like any other: host, port, username, password, token
(dev), autostart toggle — each a quiet underlined field, one column, a single
`Connect` primary button. No modal windows, no separate preferences window,
no macOS Settings pane. The gear glyph in the expanded footer flips the card;
back arrow flips it back.

## What is banned

Borders around everything, drop shadows inside the panel, gradients, cards
within cards, badges with counts, emoji as decoration (widget icons are the
one sanctioned emoji), spinners (use the three-dot working indicator),
toasts/notifications, horizontal scrolling, and any UI that appears without
being asked. When in doubt, remove it — glanceable means less.
