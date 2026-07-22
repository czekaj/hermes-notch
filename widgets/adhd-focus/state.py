#!/usr/bin/env python3
"""adhd-focus widget — glance/state from the focus queue file.

Deterministically parses ~/.hermes/data/focus_queue.md (format defined by the
adhd-focus skill) into a Hermes Notch Card. No model, no network: the glance
must stay cheap because it is polled frequently.

Usage:
    state.py            # full Card (used when no chat session is active yet)
    state.py --glance   # same Card; flag kept for spec symmetry / future diet
"""
from __future__ import annotations

import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

QUEUE_PATH = Path.home() / ".hermes" / "data" / "focus_queue.md"

ITEM_RE = re.compile(r"^- (?:\[(?P<box>[ xX])\] )?(?P<text>.+?)\s*$")
SOURCE_RE = re.compile(r"\s*[—-]+\s*source:\s*(?P<source>[a-zA-Z]+:[\w./-]+)(?P<rest>.*)$")
EST_RE = re.compile(r"\(~\s*(?P<min>\d+)\s*min\)")
UPDATED_RE = re.compile(r"^# Focus queue\s*[—-]+\s*updated\s+(?P<date>.+?)\s*$")


def parse_queue(text: str) -> dict:
    """Split the queue markdown into sections of parsed items."""
    sections: dict[str, list[dict]] = {}
    updated = None
    current = None
    for line in text.splitlines():
        m = UPDATED_RE.match(line)
        if m:
            updated = m.group("date")
            continue
        if line.startswith("## "):
            heading = line[3:].strip().lower()
            if heading.startswith("now"):
                current = "now"
            elif heading.startswith("next"):
                current = "next"
            elif heading.startswith("parked"):
                current = "parked"
            elif heading.startswith("triage"):
                current = "triage"
            elif heading.startswith("done"):
                current = "done"
            else:
                current = heading
            sections.setdefault(current, [])
            continue
        if current is None or not line.startswith("- "):
            continue
        m = ITEM_RE.match(line)
        if not m:
            continue
        text_part = m.group("text")
        item = {"done": (m.group("box") or " ").lower() == "x"}
        sm = SOURCE_RE.search(text_part)
        if sm:
            item["source"] = sm.group("source")
            text_part = text_part[: sm.start()] + sm.group("rest")
        em = EST_RE.search(text_part)
        if em:
            item["est_min"] = int(em.group("min"))
            text_part = EST_RE.sub("", text_part)
        item["text"] = text_part.strip(" —-").strip()
        sections[current].append(item)
    return {"updated": updated, "sections": sections}


def build_card(parsed: dict, mtime: datetime | None) -> dict:
    sections = parsed["sections"]
    now_items = sections.get("now", [])
    open_steps = [i for i in now_items if not i["done"]]
    done_steps = [i for i in now_items if i["done"]]
    next_items = sections.get("next", [])
    parked = sections.get("parked", [])
    triage = sections.get("triage", [])

    # Protocol words (done/skip/…) only make sense while a step is open;
    # otherwise the offered actions are starting a new batch and refreshing —
    # a stray `done` right after /adhd could close a just-served task at
    # source. `why` stays declared but off the button row (typeable); five
    # visible buttons max per the design doc.
    step_actions = ["done", "skip", "smaller", "pause", "refresh"]
    if open_steps:
        actions_enabled = step_actions
        step = open_steps[0]
        pos = len(done_steps) + 1
        total = len(now_items)
        detail_bits = []
        if step.get("est_min"):
            detail_bits.append(f"~{step['est_min']} min")
        detail_bits.append(f"step {pos} of {total}")
        glance = {
            "text": step["text"][:60],
            "detail": " · ".join(detail_bits)[:40],
            "urgency": "attention",
        }
        title = f"Step {pos} of {total}"
    elif next_items:
        actions_enabled = ["start", "refresh"]
        glance = {
            "text": f"Batch clear — next up: {next_items[0]['text']}"[:60],
            "detail": f"{len(next_items)} queued",
            "urgency": "normal",
        }
        title = "Batch clear"
    else:
        actions_enabled = ["start", "refresh"]
        glance = {"text": "Focus queue empty", "detail": "start a batch to build one", "urgency": "normal"}
        title = "Focus"

    body: list[dict] = []
    if open_steps:
        step = open_steps[0]
        body.append({"type": "text", "text": step["text"]})
        kv = []
        if step.get("source"):
            kv.append(["Source", step["source"]])
        if step.get("est_min"):
            kv.append(["Est", f"~{step['est_min']} min"])
        if kv:
            body.append({"type": "kv", "items": kv})
        body.append({"type": "progress", "value": len(done_steps), "max": len(now_items), "label": "batch"})
    if next_items:
        body.append({"type": "divider"})
        body.append({"type": "md", "text": "**Next:** " + " · ".join(i["text"] for i in next_items[:2])
                     + (f" (+{len(next_items) - 2} more)" if len(next_items) > 2 else "")})
    counts = f"{len(next_items)} next · {len(parked)} parked · {len(triage)} triage"
    body.append({"type": "kv", "items": [["Queue", counts]]})

    stale = False
    if mtime is not None:
        age_days = (datetime.now(timezone.utc) - mtime).days
        if age_days >= 3:
            stale = True

    return {
        "card": 1,
        "glance": glance,
        "title": title,
        "body": body,
        "actions_enabled": actions_enabled,
        "status": {"state": "stale" if stale else "ok",
                   "detail": f"queue updated {parsed['updated']}" if stale and parsed.get("updated") else ""},
        "ts": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }


def main() -> int:
    try:
        text = QUEUE_PATH.read_text(encoding="utf-8")
        mtime = datetime.fromtimestamp(QUEUE_PATH.stat().st_mtime, tz=timezone.utc)
    except FileNotFoundError:
        print(json.dumps({
            "card": 1,
            "glance": {"text": "No focus queue yet", "detail": "start a batch", "urgency": "normal"},
            "title": "Focus",
            "body": [{"type": "text", "text": "No queue file at ~/.hermes/data/focus_queue.md — start a batch to build one."}],
            "actions_enabled": ["start"],
            "status": {"state": "ok", "detail": ""},
            "ts": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        }))
        return 0
    except Exception as exc:  # unreadable file → error Card, never a traceback
        print(json.dumps({
            "card": 1,
            "glance": {"text": "Focus queue unreadable", "detail": type(exc).__name__, "urgency": "attention"},
            "status": {"state": "error", "detail": str(exc)[:200]},
        }))
        return 0

    print(json.dumps(build_card(parse_queue(text), mtime), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
