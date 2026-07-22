"""Hermes Notch — widget host (dashboard plugin backend).

Serves Widget Spec v1 directories from ~/.hermes/notch-widgets as specs and
render-ready Cards, and executes server-side widget actions (script effects).
Chat effects are transported by the Notch app itself over the dashboard's
JSON-RPC WebSocket (/api/ws) — see docs/PROTOCOL.md in the hermes-notch repo.

Routes (mounted under /api/plugins/hermes-notch):
    GET  /widgets                    — installed widget specs
    GET  /widgets/{id}/state         — run the widget's source → Card
    GET  /widgets/{id}/glance        — cheap glance Card (never wakes an agent)
    POST /widgets/{id}/action        — execute a declared script action → Card
    POST /rescan                     — rescan the widgets directory
    GET  /spec                       — the installed WIDGET_SPEC.md (markdown)
    GET  /health                     — host info for the app's settings screen
"""
from __future__ import annotations

import json
import re
import subprocess
import threading
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from fastapi import APIRouter, Body, HTTPException
from fastapi.responses import PlainTextResponse

router = APIRouter()

WIDGETS_DIR = Path.home() / ".hermes" / "notch-widgets"
SPEC_COPY = WIDGETS_DIR / ".spec" / "WIDGET_SPEC.md"
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{1,40}$")
DEFAULT_TIMEOUT = 20.0
HOST_VERSION = "0.1.0"

_lock = threading.Lock()
_widgets: dict[str, dict[str, Any]] = {}
_loaded_at: float = 0.0


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _error_card(detail: str, glance_text: str = "Widget error") -> dict[str, Any]:
    return {
        "card": 1,
        "glance": {"text": glance_text[:60], "detail": "", "urgency": "attention"},
        "body": [{"type": "text", "text": detail[:400]}],
        "status": {"state": "error", "detail": detail[:200]},
        "ts": _now_iso(),
    }


def _scan() -> None:
    global _loaded_at
    found: dict[str, dict[str, Any]] = {}
    if WIDGETS_DIR.is_dir():
        for entry in sorted(WIDGETS_DIR.iterdir()):
            if entry.name.startswith(".") or not entry.is_dir():
                continue
            spec_path = entry / "widget.json"
            if not spec_path.is_file():
                continue
            try:
                spec = json.loads(spec_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                found[entry.name] = {"dir": entry, "spec": None, "load_error": f"{type(exc).__name__}: {exc}"}
                continue
            wid = spec.get("id")
            if not (isinstance(wid, str) and ID_RE.match(wid) and wid == entry.name):
                found[entry.name] = {"dir": entry, "spec": None,
                                     "load_error": f"id {wid!r} must match directory name {entry.name!r}"}
                continue
            found[wid] = {"dir": entry, "spec": spec, "load_error": None}
    with _lock:
        _widgets.clear()
        _widgets.update(found)
        _loaded_at = time.time()


def _get(widget_id: str) -> dict[str, Any]:
    with _lock:
        if not _widgets and _loaded_at == 0.0:
            pass  # first access — scan below
    if not _widgets and _loaded_at == 0.0:
        _scan()
    with _lock:
        w = _widgets.get(widget_id)
    if w is None:
        raise HTTPException(status_code=404, detail=f"unknown widget {widget_id!r}")
    if w["spec"] is None:
        raise HTTPException(status_code=500, detail=f"widget {widget_id!r} failed to load: {w['load_error']}")
    return w


def _run_script(widget: dict[str, Any], command: list[str], timeout: float) -> dict[str, Any]:
    """Run a widget script and normalize stdout to a Card (never raises)."""
    wid = widget["spec"]["id"]
    try:
        proc = subprocess.run(
            command, cwd=widget["dir"], capture_output=True, text=True, timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return _error_card(f"{wid}: script timed out after {timeout:.0f}s", f"{wid} timed out")
    except (OSError, ValueError) as exc:
        return _error_card(f"{wid}: cannot run script: {exc}", f"{wid} broken")
    if proc.returncode != 0:
        tail = (proc.stderr or proc.stdout or "").strip()[-300:]
        return _error_card(f"{wid}: script exited {proc.returncode}: {tail}", f"{wid} failed")
    try:
        card = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        return _error_card(f"{wid}: script output is not JSON: {exc}", f"{wid} bad output")
    if not isinstance(card, dict) or not isinstance(card.get("glance"), dict):
        return _error_card(f"{wid}: script output is not a Card (missing glance)", f"{wid} bad card")
    card.setdefault("card", 1)
    card.setdefault("ts", _now_iso())
    return card


def _read_file_card(widget: dict[str, Any], path_str: str) -> dict[str, Any]:
    wid = widget["spec"]["id"]
    path = Path(path_str).expanduser()
    try:
        card = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return _error_card(f"{wid}: card file not found: {path}", f"{wid}: no data yet")
    except (OSError, json.JSONDecodeError) as exc:
        return _error_card(f"{wid}: cannot read card file: {exc}", f"{wid} bad file")
    if not isinstance(card, dict) or not isinstance(card.get("glance"), dict):
        return _error_card(f"{wid}: file does not contain a Card", f"{wid} bad card")
    return card


def _source_card(widget: dict[str, Any], glance_only: bool) -> dict[str, Any]:
    spec = widget["spec"]
    source = spec.get("source") or {}
    stype = source.get("type")
    if stype == "script":
        return _run_script(widget, source.get("command") or [], float(source.get("timeout", DEFAULT_TIMEOUT)))
    if stype == "file":
        return _read_file_card(widget, source.get("path", ""))
    if stype == "chat":
        glance = source.get("glance")
        if isinstance(glance, dict) and glance.get("type") == "script":
            card = _run_script(widget, glance.get("command") or [], float(glance.get("timeout", DEFAULT_TIMEOUT)))
        else:
            card = {
                "card": 1,
                "glance": {"text": spec.get("name", widget["dir"].name), "detail": "chat widget", "urgency": "normal"},
                "status": {"state": "ok", "detail": ""},
                "ts": _now_iso(),
            }
        # Tell the app this widget's body/actions ride the chat transport.
        card["chat"] = {"on_start": source.get("on_start"), "session_tag": f"notch:{spec['id']}"}
        return card
    return _error_card(f"{spec['id']}: unsupported source type {stype!r}", "bad widget spec")


@router.get("/widgets")
def list_widgets() -> dict[str, Any]:
    if not _widgets and _loaded_at == 0.0:
        _scan()
    with _lock:
        items = []
        for name, w in _widgets.items():
            if w["spec"] is None:
                items.append({"id": name, "error": w["load_error"]})
            else:
                items.append({"id": name, "spec": w["spec"]})
    return {"widgets": items, "scanned_at": _loaded_at, "dir": str(WIDGETS_DIR)}


@router.get("/widgets/{widget_id}/state")
def widget_state(widget_id: str) -> dict[str, Any]:
    return _source_card(_get(widget_id), glance_only=False)


@router.get("/widgets/{widget_id}/glance")
def widget_glance(widget_id: str) -> dict[str, Any]:
    return _source_card(_get(widget_id), glance_only=True)


@router.post("/widgets/{widget_id}/action")
def widget_action(widget_id: str, payload: dict[str, Any] = Body(...)) -> dict[str, Any]:
    widget = _get(widget_id)
    action_id = payload.get("id")
    actions = {a.get("id"): a for a in widget["spec"].get("actions", []) if isinstance(a, dict)}
    action = actions.get(action_id)
    if action is None:
        raise HTTPException(status_code=404, detail=f"unknown action {action_id!r}")
    effect = action.get("effect") or {}
    etype = effect.get("type")
    if etype == "chat":
        # Chat effects are executed by the app over /api/ws (prompt.submit) —
        # the host has no standing credential to inject into gateway sessions.
        raise HTTPException(status_code=409, detail="chat effects are executed by the app over /api/ws")
    if etype == "script":
        return _run_script(widget, effect.get("command") or [], float(effect.get("timeout", DEFAULT_TIMEOUT)))
    raise HTTPException(status_code=400, detail=f"unsupported effect type {etype!r}")


@router.post("/rescan")
def rescan() -> dict[str, Any]:
    _scan()
    with _lock:
        return {"ok": True, "widgets": sorted(_widgets), "scanned_at": _loaded_at}


@router.get("/spec", response_class=PlainTextResponse)
def spec_markdown() -> str:
    try:
        return SPEC_COPY.read_text(encoding="utf-8")
    except OSError:
        raise HTTPException(status_code=404, detail=f"spec not installed at {SPEC_COPY}")


@router.get("/health")
def health() -> dict[str, Any]:
    if not _widgets and _loaded_at == 0.0:
        _scan()
    with _lock:
        n_ok = sum(1 for w in _widgets.values() if w["spec"] is not None)
        n_err = len(_widgets) - n_ok
    return {
        "ok": True,
        "host_version": HOST_VERSION,
        "widgets": n_ok,
        "widget_errors": n_err,
        "widgets_dir": str(WIDGETS_DIR),
        "ts": _now_iso(),
    }


_scan()
