---
name: notch-widgetizer
description: Use when the user wants to turn a Hermes skill, a recurring prompt/cron job, or any data source into a Hermes Notch widget — translates it into a Widget Spec v1 directory (widget.json + state script), validates it, and installs it into ~/.hermes/notch-widgets.
version: 1.0.0
author: Hermes Notch
license: MIT
metadata:
  hermes:
    tags: [notch, hud, widgets, codegen, productivity]
    related_skills: [hermes-agent-skill-authoring]
---

# Notch Widgetizer — anything → a Hermes Notch widget

You translate three kinds of input into a valid Widget Spec v1 directory:

- **A skill** ("widgetize adhd-focus") — read its `SKILL.md` and scripts.
- **A prompt / cron job** ("make a widget from my morning event briefing") —
  read the job definition in `~/.hermes/cron/jobs.json`.
- **A raw description** ("a widget showing my kanban WIP count") — design
  from scratch.

The contract you emit against is `docs/WIDGET_SPEC.md` in the Hermes Notch
repo (installed copy: `~/.hermes/notch-widgets/.spec/WIDGET_SPEC.md` if
present; otherwise fetch from the repo). **Read it first, every time.** The
spec is the source of truth; this skill is only the translation procedure.

## Procedure

1. **Locate the source material.** For a skill: its `SKILL.md`, scripts
   directory, and any state files it maintains (queue files, output files,
   SQLite pools). For a cron job: the `prompt`, `deliver` target, and any
   scripts it calls. List what deterministic assets already exist — the
   whole game is reusing them, never re-implementing logic in the widget.

2. **Choose the source type** (decision order):
   - State lives in a file the skill/job already maintains → `script` source
     that parses that file. Cheapest, always fresh, zero agent tokens.
   - State needs a computation the skill already ships as a script
     (aggregators, rankers) → `script` source that shells out to it. Mind the
     20 s cap: if the underlying script is slow (network fan-out), have the
     widget script read that script's cached/last output instead, and add a
     `script`-effect "Refresh" action that reruns the slow path.
   - Interaction inherently needs agent judgment per exchange (conversational
     protocols like adhd-focus, anything with "apply judgment") → `chat`
     source with the skill's invocation alias as `on_start`, plus a `glance`
     script so frequent polls never wake the agent.
   - A cron job already delivers a periodic briefing → `file` source: add a
     step to the job writing a Card JSON next to its normal delivery, or a
     `script` source parsing the job's newest output in `~/.hermes/cron/output/`.

3. **Design the glance** before anything else: one line ≤ 60 chars answering
   "what would Lucas want pinned next to the camera from this?" — the single
   most actionable/urgent datum, not a summary. If the source has ranked
   items, glance = top item. If it has counts, glance = the count that
   demands action. Add `detail` (≤ 40 chars) for the second-most-useful datum.

4. **Map the interaction protocol to actions.** From a skill's protocol table
   (user says X → agent does Y): each protocol word becomes a `chat` action
   with the exact word as `text`. From a mechanical operation (close task,
   rerun refresh): a `script` action. Cap at 5; pick the ones used every
   session, not the long tail. If the skill accepts free-form replies, add
   the widget-level `input`.

5. **Write the state script.** Rules from the spec, enforced here:
   - stdout = exactly one JSON Card; nothing else ever prints to stdout.
   - Every failure path emits a `status: error` Card (exit 0) — a widget
     must never render a traceback or vanish.
   - No secrets in any Card field; the Card crosses the network.
   - Python 3 stdlib only unless the reused skill scripts already require
     more.

6. **Validate and install:**
   ```bash
   python3 <plugin-dir>/validate_widget.py <widget-dir> --run
   mkdir -p ~/.hermes/notch-widgets && cp -R <widget-dir> ~/.hermes/notch-widgets/<id>
   curl -s -X POST <dashboard>/api/plugins/hermes-notch/rescan
   ```
   Then fetch `<dashboard>/api/plugins/hermes-notch/widgets/<id>/state` once
   and eyeball the Card. Do not declare success until both the validator and
   the live fetch pass.

7. **Report** to the user: widget id, source type chosen and why, the glance
   line as it will render, actions list, and anything you deliberately left
   out (with the one-line reason).

## Translation heuristics (learned defaults)

- Prefer parsing a skill's **persistent state file** over rerunning its
  aggregator: state files are the skill's own cache of "current truth".
- A skill's **batch/session structure** maps to `progress` blocks; its
  **rankings** map to glance ordering; its **win lists** map to a trailing
  `md` block, not the glance.
- Protocol words are sent **verbatim** — the skill's session already knows
  how to interpret them; do not paraphrase (`done`, not "mark as done").
- When a skill says "never show the full list" (overwhelm-sensitive design
  like adhd-focus), the widget must respect it: glance + one step only,
  counts instead of inventories. Widget UX inherits the skill's UX rules.
- Chat-backed widgets should keep `refresh.interval` ≥ 60 s and always ship a
  glance script; script widgets can poll faster (15–30 s).

## Pitfalls

1. Do not invent rendering the app can't do — only the seven block types.
2. Do not put ranking/judgment logic in the widget script if the skill
   already has it — shell out or read its output.
3. Do not exceed the glance budget; truncation is the renderer's job but
   design-to-fit is yours.
4. Do not create a chat-backed widget for something a script can compute —
   agent sessions cost tokens and latency; scripts are free.
5. Do not skip `--run` validation: a spec-valid widget with a broken script
   is still a broken widget.
