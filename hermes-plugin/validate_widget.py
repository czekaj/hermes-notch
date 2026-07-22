#!/usr/bin/env python3
"""Validate a Hermes Notch widget directory against Widget Spec v1.

Usage:
    python3 validate_widget.py <widget-dir> [--run]

--run additionally executes script sources and validates the emitted Card.
Exit code 0 = valid (warnings allowed), 1 = errors.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{1,40}$")
BLOCK_TYPES = {"text", "md", "copy", "link", "kv", "progress", "divider"}
EFFECT_TYPES = {"chat", "script"}
URGENCIES = {"normal", "attention", "urgent"}
STATUS_STATES = {"ok", "stale", "error"}
STYLES = {"primary", "default", "danger"}

errors: list[str] = []
warnings: list[str] = []


def err(msg: str) -> None:
    errors.append(msg)


def warn(msg: str) -> None:
    warnings.append(msg)


def check_effect(effect: object, where: str, allow_textless_chat: bool = False) -> None:
    if not isinstance(effect, dict):
        err(f"{where}: effect must be an object")
        return
    etype = effect.get("type")
    if etype not in EFFECT_TYPES:
        err(f"{where}: effect.type must be one of {sorted(EFFECT_TYPES)}, got {etype!r}")
        return
    if etype == "chat" and not allow_textless_chat and not isinstance(effect.get("text"), str):
        err(f"{where}: chat effect needs a string 'text' (only the widget-level input may omit it)")
    if etype == "script":
        cmd = effect.get("command")
        if not (isinstance(cmd, list) and cmd and all(isinstance(c, str) for c in cmd)):
            err(f"{where}: script effect needs 'command' as a non-empty list of strings")


def check_script_source(src: dict, widget_dir: Path, where: str) -> None:
    cmd = src.get("command")
    if not (isinstance(cmd, list) and cmd and all(isinstance(c, str) for c in cmd)):
        err(f"{where}: script source needs 'command' as a non-empty list of strings")
        return
    # Heuristic: if the command references a file in the widget dir, it should exist.
    for part in cmd[1:]:
        if part.startswith("-"):
            continue
        candidate = widget_dir / part
        if ("." in part or "/" in part) and not candidate.exists():
            warn(f"{where}: command references {part!r} but {candidate} does not exist")


def validate_card(card: object, where: str) -> None:
    if not isinstance(card, dict):
        err(f"{where}: Card must be a JSON object")
        return
    if card.get("card") != 1:
        warn(f"{where}: Card 'card' version is {card.get('card')!r}, expected 1")
    glance = card.get("glance")
    if not isinstance(glance, dict) or not isinstance(glance.get("text"), str):
        err(f"{where}: Card needs glance.text (string)")
    elif len(glance["text"]) > 60:
        warn(f"{where}: glance.text is {len(glance['text'])} chars (spec: ≤60)")
    if isinstance(glance, dict) and glance.get("urgency") not in (None, *URGENCIES):
        err(f"{where}: glance.urgency must be one of {sorted(URGENCIES)}")
    for i, block in enumerate(card.get("body", []) or []):
        bwhere = f"{where}: body[{i}]"
        if not isinstance(block, dict):
            err(f"{bwhere}: block must be an object")
            continue
        btype = block.get("type")
        if btype not in BLOCK_TYPES:
            warn(f"{bwhere}: unknown block type {btype!r} (renderer will skip it)")
            continue
        if btype in ("text", "md") and not isinstance(block.get("text"), str):
            err(f"{bwhere}: {btype} block needs 'text'")
        if btype == "copy" and not isinstance(block.get("value"), str):
            err(f"{bwhere}: copy block needs 'value'")
        if btype == "link" and not isinstance(block.get("url"), str):
            err(f"{bwhere}: link block needs 'url'")
        if btype == "kv":
            items = block.get("items")
            ok = isinstance(items, list) and all(
                isinstance(p, list) and len(p) == 2 and all(isinstance(x, str) for x in p) for p in (items or [])
            )
            if not ok:
                err(f"{bwhere}: kv block needs 'items' as [[key, value], …] of strings")
        if btype == "progress" and not (
            isinstance(block.get("value"), (int, float)) and isinstance(block.get("max"), (int, float))
        ):
            err(f"{bwhere}: progress block needs numeric 'value' and 'max'")
    status = card.get("status")
    if status is not None:
        if not isinstance(status, dict) or status.get("state") not in STATUS_STATES:
            err(f"{where}: status.state must be one of {sorted(STATUS_STATES)}")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    widget_dir = Path(sys.argv[1]).expanduser().resolve()
    run_scripts = "--run" in sys.argv[2:]

    spec_path = widget_dir / "widget.json"
    if not spec_path.exists():
        print(f"ERROR: {spec_path} not found")
        return 1
    try:
        spec = json.loads(spec_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"ERROR: widget.json is not valid JSON: {exc}")
        return 1

    if spec.get("spec") != 1:
        err(f"'spec' must be 1, got {spec.get('spec')!r}")
    wid = spec.get("id")
    if not (isinstance(wid, str) and ID_RE.match(wid)):
        err(f"'id' must match {ID_RE.pattern}, got {wid!r}")
    elif wid != widget_dir.name:
        warn(f"'id' ({wid}) differs from directory name ({widget_dir.name}) — install dir must be named {wid}")
    if not isinstance(spec.get("name"), str) or not spec["name"]:
        err("'name' is required")
    if not isinstance(spec.get("icon"), str):
        warn("'icon' missing (expected a single emoji)")

    src = spec.get("source")
    if not isinstance(src, dict):
        err("'source' object is required")
    else:
        stype = src.get("type")
        if stype == "script":
            check_script_source(src, widget_dir, "source")
        elif stype == "chat":
            if not isinstance(src.get("on_start"), str) or not src["on_start"]:
                err("source(chat): 'on_start' message is required")
            glance = src.get("glance")
            if glance is not None:
                if not (isinstance(glance, dict) and glance.get("type") == "script"):
                    err("source(chat): 'glance' sub-source must be a script source")
                else:
                    check_script_source(glance, widget_dir, "source.glance")
            else:
                warn("source(chat): no 'glance' script — every glance poll will hit the agent session")
        elif stype == "file":
            if not isinstance(src.get("path"), str):
                err("source(file): 'path' is required")
        else:
            err(f"source.type must be script|chat|file, got {stype!r}")

    seen_ids: set[str] = set()
    actions = spec.get("actions", [])
    if not isinstance(actions, list):
        err("'actions' must be a list")
        actions = []
    for i, action in enumerate(actions):
        awhere = f"actions[{i}]"
        if not isinstance(action, dict):
            err(f"{awhere}: must be an object")
            continue
        aid = action.get("id")
        if not (isinstance(aid, str) and ID_RE.match(aid)):
            err(f"{awhere}: 'id' must match {ID_RE.pattern}")
        elif aid in seen_ids:
            err(f"{awhere}: duplicate action id {aid!r}")
        else:
            seen_ids.add(aid)
        if not isinstance(action.get("label"), str):
            err(f"{awhere}: 'label' is required")
        if action.get("style") not in (None, *STYLES):
            err(f"{awhere}: style must be one of {sorted(STYLES)}")
        check_effect(action.get("effect"), awhere)
        if isinstance(action.get("effect"), dict) and action["effect"].get("type") == "chat" \
                and isinstance(src, dict) and src.get("type") != "chat":
            err(f"{awhere}: chat effect requires a chat source")
    if len(actions) > 5:
        warn(f"{len(actions)} actions declared — spec recommends 3–5 (this is a HUD, not a dashboard)")

    inp = spec.get("input")
    if inp is not None:
        if not isinstance(inp, dict):
            err("'input' must be an object")
        else:
            check_effect(inp.get("effect"), "input", allow_textless_chat=True)

    refresh = spec.get("refresh")
    if refresh is not None and not (
        isinstance(refresh, dict) and all(isinstance(refresh.get(k, 1), (int, float)) for k in ("interval", "while_visible"))
    ):
        err("'refresh' must be {interval?: number, while_visible?: number}")

    if run_scripts and isinstance(src, dict):
        to_run = []
        if src.get("type") == "script":
            to_run.append(("source", src))
        if src.get("type") == "chat" and isinstance(src.get("glance"), dict):
            to_run.append(("source.glance", src["glance"]))
        for where, s in to_run:
            cmd = s.get("command")
            if not cmd:
                continue
            try:
                out = subprocess.run(
                    cmd, cwd=widget_dir, capture_output=True, text=True,
                    timeout=float(s.get("timeout", 20)),
                )
                if out.returncode != 0:
                    err(f"{where}: script exited {out.returncode}: {out.stderr.strip()[:200]}")
                    continue
                validate_card(json.loads(out.stdout), f"{where} output")
            except subprocess.TimeoutExpired:
                err(f"{where}: script timed out")
            except json.JSONDecodeError as exc:
                err(f"{where}: stdout is not valid JSON: {exc}")
            except FileNotFoundError as exc:
                err(f"{where}: {exc}")

    for w in warnings:
        print(f"WARN  {w}")
    for e in errors:
        print(f"ERROR {e}")
    print(f"{widget_dir.name}: {'INVALID' if errors else 'valid'} ({len(errors)} errors, {len(warnings)} warnings)")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
