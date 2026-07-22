//! Chat transport (PROTOCOL §2): JSON-RPC 2.0 over a WebSocket to the Hermes
//! gateway. One background task owns the socket, correlates request ids with
//! oneshot replies, routes streamed events to the webview as `chat:event`, and
//! reconnects with backoff (re-ticketing each time — tickets are single-use).
//!
//! Higher-level helpers implement per-widget session ownership (§2.4) and the
//! client-side slash-resolution chain (§2.3).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use crate::http::Net;
use crate::settings;

/// Result of `chat_ensure` (PROTOCOL §3.1 `ChatStatus`).
#[derive(Serialize, Clone)]
pub struct ChatStatus {
    pub session_id: String,
    pub fresh: bool,
}

/// In-memory session bookkeeping.
#[derive(Default)]
struct Sessions {
    /// widget-id -> live/durable session ids.
    by_widget: HashMap<String, SessionEntry>,
    /// live session-id -> widget-id, for routing streamed events.
    by_live: HashMap<String, String>,
}

struct SessionEntry {
    /// Live session id used for prompt.submit / history / interrupt. Empty until
    /// this run has resumed or created the session.
    live: String,
    /// Durable id (`stored_session_id`) persisted across restarts.
    stored: String,
    /// True once the session has conversational context (resumed with history,
    /// or any turn submitted). Unprimed free text gets prefixed with the
    /// widget's `on_start` command so the skill arrives with the input.
    primed: bool,
}

/// A request to the connection task.
enum WsCommand {
    Rpc {
        method: String,
        params: Value,
        reply: oneshot::Sender<Result<Value, String>>,
    },
}

/// Handle to the running chat transport. Cheap to clone.
#[derive(Clone)]
pub struct Chat {
    tx: mpsc::UnboundedSender<WsCommand>,
    app: AppHandle,
    sessions: Arc<Mutex<Sessions>>,
}

impl Chat {
    /// Spawn the connection manager and return a handle. `persisted` seeds the
    /// durable session ids loaded from the settings store.
    pub fn start(app: AppHandle, net: Net, persisted: HashMap<String, String>) -> Chat {
        let mut sessions = Sessions::default();
        for (widget, stored) in persisted {
            if !stored.is_empty() {
                sessions.by_widget.insert(
                    widget,
                    SessionEntry {
                        live: String::new(),
                        stored,
                        primed: true, // persisted sessions have prior turns
                    },
                );
            }
        }
        let sessions = Arc::new(Mutex::new(sessions));
        let (tx, rx) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(run_manager(app.clone(), net, sessions.clone(), rx));
        Chat { tx, app, sessions }
    }

    // ---- low-level RPC ----

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(WsCommand::Rpc {
                method: method.to_string(),
                params,
                reply,
            })
            .map_err(|_| "chat transport is not running".to_string())?;
        let out = match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err("chat request was dropped".to_string()),
            Err(_) => Err("chat request timed out".to_string()),
        };
        #[cfg(debug_assertions)]
        if let Err(e) = &out {
            if method == "slash.exec" {
                // Expected for skill commands — the chain proceeds to dispatch.
                eprintln!("[notch] chat: slash.exec declined (normal, continuing via dispatch): {e}");
            } else {
                eprintln!("[notch] chat: rpc {method} failed: {e}");
            }
        }
        out
    }

    // ---- session bookkeeping ----

    fn live_in_memory(&self, widget_id: &str) -> Option<String> {
        let s = self.sessions.lock().ok()?;
        s.by_widget
            .get(widget_id)
            .filter(|e| !e.live.is_empty())
            .map(|e| e.live.clone())
    }

    fn stored_id(&self, widget_id: &str) -> Option<String> {
        let s = self.sessions.lock().ok()?;
        s.by_widget
            .get(widget_id)
            .map(|e| e.stored.clone())
            .filter(|s| !s.is_empty())
    }

    fn remember(&self, widget_id: &str, live: &str, stored: &str, primed: bool) {
        if let Ok(mut s) = self.sessions.lock() {
            // Drop any stale reverse mapping for this widget's previous live id.
            if let Some(prev) = s.by_widget.get(widget_id) {
                let prev_live = prev.live.clone();
                if !prev_live.is_empty() && prev_live != live {
                    s.by_live.remove(&prev_live);
                }
            }
            s.by_widget.insert(
                widget_id.to_string(),
                SessionEntry {
                    live: live.to_string(),
                    stored: stored.to_string(),
                    primed,
                },
            );
            s.by_live.insert(live.to_string(), widget_id.to_string());
        }
    }

    fn is_primed(&self, widget_id: &str) -> bool {
        self.sessions
            .lock()
            .ok()
            .and_then(|s| s.by_widget.get(widget_id).map(|e| e.primed))
            .unwrap_or(false)
    }

    fn mark_primed(&self, widget_id: &str) {
        if let Ok(mut s) = self.sessions.lock() {
            if let Some(e) = s.by_widget.get_mut(widget_id) {
                e.primed = true;
            }
        }
    }

    /// Forget the widget's session entirely — the next interaction creates a
    /// fresh gateway session (the old one stays on the host as plain history).
    pub async fn reset(&self, widget_id: &str) -> Result<(), String> {
        let live = self.live_in_memory(widget_id);
        if let Some(live) = live {
            // Best-effort: stop any running turn before we abandon it.
            let _ = self
                .rpc("session.interrupt", json!({ "session_id": live }))
                .await;
        }
        if let Ok(mut s) = self.sessions.lock() {
            if let Some(entry) = s.by_widget.remove(widget_id) {
                if !entry.live.is_empty() {
                    s.by_live.remove(&entry.live);
                }
            }
        }
        self.persist();
        Ok(())
    }

    fn persist(&self) {
        let snapshot: HashMap<String, String> = match self.sessions.lock() {
            Ok(s) => s
                .by_widget
                .iter()
                .filter(|(_, e)| !e.stored.is_empty())
                .map(|(w, e)| (w.clone(), e.stored.clone()))
                .collect(),
            Err(_) => return,
        };
        let _ = settings::save_sessions(&self.app, &snapshot);
    }

    /// Get the live session id, resuming from the durable id if needed. Never
    /// creates a session (that only happens via `chat_ensure`).
    async fn live_or_resume(&self, widget_id: &str) -> Result<String, String> {
        if let Some(live) = self.live_in_memory(widget_id) {
            return Ok(live);
        }
        if let Some(stored) = self.stored_id(widget_id) {
            let res = self
                .rpc("session.resume", json!({ "session_id": stored }))
                .await?;
            let live = res
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or(&stored)
                .to_string();
            self.remember(widget_id, &live, &stored, true);
            return Ok(live);
        }
        Err("no chat session yet — open the widget first".to_string())
    }

    // ---- public commands (PROTOCOL §3.1) ----

    /// Resume the widget's stored session or create a fresh one. Never runs an
    /// agent turn — priming happens in `send` (see the `primed` field), so a
    /// mere hover/expand never costs tokens.
    pub async fn ensure(&self, widget_id: &str) -> Result<ChatStatus, String> {
        if let Some(live) = self.live_in_memory(widget_id) {
            return Ok(ChatStatus {
                session_id: live,
                fresh: false,
            });
        }
        // Try to resume a persisted session first.
        if let Some(stored) = self.stored_id(widget_id) {
            if let Ok(res) = self
                .rpc("session.resume", json!({ "session_id": stored }))
                .await
            {
                let live = res
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .unwrap_or(&stored)
                    .to_string();
                self.remember(widget_id, &live, &stored, true);
                return Ok(ChatStatus {
                    session_id: live,
                    fresh: false,
                });
            }
            // Resume failed (stale id) — fall through and create a new one.
        }
        // Create a fresh session, titled with the widget's session tag.
        let res = self
            .rpc(
                "session.create",
                json!({ "title": format!("notch:{widget_id}") }),
            )
            .await?;
        let live = res
            .get("session_id")
            .and_then(|s| s.as_str())
            .ok_or("session.create returned no session_id")?
            .to_string();
        let stored = res
            .get("stored_session_id")
            .and_then(|s| s.as_str())
            .unwrap_or(&live)
            .to_string();
        self.remember(widget_id, &live, &stored, false);
        self.persist();
        Ok(ChatStatus {
            session_id: live,
            fresh: true,
        })
    }

    /// Submit text: slash inputs go through the resolution chain, plain text is
    /// a single `prompt.submit`. Free text into an unprimed session is prefixed
    /// with `on_start` ("/adhd veto that") so the skill arrives with the input
    /// in a single turn. Replies stream back as `chat:event`s.
    pub async fn send(
        &self,
        widget_id: &str,
        text: &str,
        on_start: Option<String>,
    ) -> Result<(), String> {
        // Sending may need to create the session (e.g. reset, then a button).
        let live = match self.live_or_resume(widget_id).await {
            Ok(l) => l,
            Err(_) => self.ensure(widget_id).await?.session_id,
        };
        let text = text.trim();
        let effective = match on_start.filter(|s| !s.trim().is_empty()) {
            Some(prime) if !text.starts_with('/') && !self.is_primed(widget_id) => {
                format!("{} {}", prime.trim(), text)
            }
            _ => text.to_string(),
        };
        let out = self
            .resolve_and_submit(widget_id, &live, &effective, 0)
            .await;
        if out.is_ok() {
            self.mark_primed(widget_id);
        }
        out
    }

    /// Return the last assistant message's markdown ("" if none).
    pub async fn history(&self, widget_id: &str) -> Result<String, String> {
        let live = self.live_or_resume(widget_id).await?;
        let res = self
            .rpc("session.history", json!({ "session_id": live }))
            .await?;
        let text = res
            .get("messages")
            .and_then(|m| m.as_array())
            .and_then(|arr| last_assistant_text(arr))
            .unwrap_or_default();
        Ok(text)
    }

    /// Cancel the running turn, if any.
    pub async fn interrupt(&self, widget_id: &str) -> Result<(), String> {
        let live = match self.live_in_memory(widget_id) {
            Some(l) => l,
            None => return Ok(()),
        };
        self.rpc("session.interrupt", json!({ "session_id": live }))
            .await
            .map(|_| ())
    }

    // ---- slash resolution (§2.3) ----

    async fn resolve_and_submit(
        &self,
        widget_id: &str,
        session_id: &str,
        input: &str,
        depth: u8,
    ) -> Result<(), String> {
        if depth > 5 {
            return Err("slash alias resolution went too deep".to_string());
        }
        let input = input.trim();
        let rest = match input.strip_prefix('/') {
            None => return self.submit(session_id, input).await,
            Some(r) => r,
        };
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let arg = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            return self.submit(session_id, input).await;
        }

        // 1) slash.exec — handles dashboard built-ins, and for skills it loads
        //    the skill into the live session's context ("⚡ Loading skill: …")
        //    WITHOUT running an agent turn. Its success must NOT end the chain:
        //    the dispatch step below yields the actual message that makes the
        //    agent run. Keep its output to resolve slash-only commands.
        let slash_output: Option<String> = match self
            .rpc(
                "slash.exec",
                json!({ "command": name, "session_id": session_id }),
            )
            .await
        {
            Ok(res) => Some(
                res.get("output")
                    .and_then(|o| o.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            ),
            Err(_) => None,
        };

        // 2) command.dispatch — quick_commands / skills / aliases.
        let res = match self
            .rpc(
                "command.dispatch",
                json!({ "name": name, "arg": arg, "session_id": session_id }),
            )
            .await
        {
            Ok(res) => res,
            Err(e) => {
                // Dispatch doesn't know the command. If slash.exec handled it,
                // render its output as the completed reply; else it's a failure.
                return match slash_output {
                    Some(out) => {
                        let text = if out.is_empty() { format!("/{name} ✓") } else { out };
                        self.emit_event(widget_id, "complete", Some(&text));
                        Ok(())
                    }
                    None => Err(e),
                };
            }
        };
        match res.get("type").and_then(|t| t.as_str()) {
            Some("alias") => {
                let target = res.get("target").and_then(|t| t.as_str()).unwrap_or("");
                if target.is_empty() {
                    return Err("alias command had no target".to_string());
                }
                let next = if arg.is_empty() {
                    format!("/{target}")
                } else {
                    format!("/{target} {arg}")
                };
                Box::pin(self.resolve_and_submit(widget_id, session_id, &next, depth + 1)).await
            }
            Some("exec") => {
                let output = res.get("output").and_then(|o| o.as_str()).unwrap_or("");
                // Render the command output directly as a completed reply.
                self.emit_event(widget_id, "complete", Some(output));
                Ok(())
            }
            _ => {
                if let Some(msg) = res.get("message").and_then(|m| m.as_str()) {
                    self.submit(session_id, msg).await
                } else if let Some(out) = slash_output {
                    // Nothing to submit — a slash-only command; show its output.
                    let text = if out.is_empty() { format!("/{name} ✓") } else { out };
                    self.emit_event(widget_id, "complete", Some(&text));
                    Ok(())
                } else {
                    Err(format!("/{name}: nothing to run (unknown command?)"))
                }
            }
        }
    }

    async fn submit(&self, session_id: &str, text: &str) -> Result<(), String> {
        #[cfg(debug_assertions)]
        eprintln!(
            "[notch] chat: prompt.submit to {session_id} ({} chars)",
            text.len()
        );
        self.rpc(
            "prompt.submit",
            json!({ "session_id": session_id, "text": text }),
        )
        .await
        .map(|_| ())
    }

    fn emit_event(&self, widget_id: &str, kind: &str, text: Option<&str>) {
        let mut ev = json!({ "widgetId": widget_id, "kind": kind });
        if let Some(t) = text {
            ev["text"] = json!(t);
        }
        let _ = self.app.emit("chat:event", ev);
    }
}

// ---- connection manager ----

enum LoopExit {
    ChannelClosed,
    Disconnected,
}

async fn run_manager(
    app: AppHandle,
    net: Net,
    sessions: Arc<Mutex<Sessions>>,
    mut cmd_rx: mpsc::UnboundedReceiver<WsCommand>,
) {
    let mut backoff = Duration::from_millis(500);
    let max_backoff = Duration::from_secs(10);

    loop {
        match connect(&net).await {
            Ok(ws) => {
                backoff = Duration::from_millis(500);
                match run_connected(ws, &mut cmd_rx, &app, &sessions).await {
                    LoopExit::ChannelClosed => return,
                    LoopExit::Disconnected => {}
                }
            }
            Err(_e) => { /* fall through to backoff + reconnect */ }
        }

        // Backoff before reconnecting. While waiting, drain any requests with an
        // error so callers don't hang, and exit if the handle was dropped.
        let deadline = tokio::time::sleep(backoff);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                cmd = cmd_rx.recv() => match cmd {
                    None => return,
                    Some(WsCommand::Rpc { reply, .. }) => {
                        let _ = reply.send(Err("chat transport is reconnecting".to_string()));
                    }
                }
            }
        }
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn connect(
    net: &Net,
) -> Result<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    String,
> {
    let url = net.ws_connect_url().await?;
    let (ws, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("ws connect failed: {e}"))?;
    Ok(ws)
}

async fn run_connected(
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    cmd_rx: &mut mpsc::UnboundedReceiver<WsCommand>,
    app: &AppHandle,
    sessions: &Arc<Mutex<Sessions>>,
) -> LoopExit {
    let (mut write, mut read) = ws.split();
    let mut pending: HashMap<u64, oneshot::Sender<Result<Value, String>>> = HashMap::new();
    let mut next_id: u64 = 0;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                None => {
                    fail_all(&mut pending, "chat transport shut down");
                    return LoopExit::ChannelClosed;
                }
                Some(WsCommand::Rpc { method, params, reply }) => {
                    next_id += 1;
                    let id = next_id;
                    let frame = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": method,
                        "params": params,
                    });
                    match write.send(Message::text(frame.to_string())).await {
                        Ok(()) => { pending.insert(id, reply); }
                        Err(e) => {
                            let _ = reply.send(Err(format!("failed to send request: {e}")));
                            fail_all(&mut pending, "connection lost");
                            return LoopExit::Disconnected;
                        }
                    }
                }
            },
            msg = read.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    handle_frame(&t.to_string(), &mut pending, app, sessions);
                }
                Some(Ok(Message::Close(_))) | None => {
                    fail_all(&mut pending, "connection closed");
                    return LoopExit::Disconnected;
                }
                Some(Err(e)) => {
                    fail_all(&mut pending, &format!("connection error: {e}"));
                    return LoopExit::Disconnected;
                }
                // Ping/Pong/Binary/Frame — ignore.
                Some(Ok(_)) => {}
            },
        }
    }
}

fn fail_all(pending: &mut HashMap<u64, oneshot::Sender<Result<Value, String>>>, reason: &str) {
    for (_, reply) in pending.drain() {
        let _ = reply.send(Err(reason.to_string()));
    }
}

fn handle_frame(
    text: &str,
    pending: &mut HashMap<u64, oneshot::Sender<Result<Value, String>>>,
    app: &AppHandle,
    sessions: &Arc<Mutex<Sessions>>,
) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    // JSON-RPC response (has an id and a result/error).
    if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
        if v.get("result").is_some() || v.get("error").is_some() {
            if let Some(reply) = pending.remove(&id) {
                if let Some(err) = v.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("rpc error");
                    let _ = reply.send(Err(msg.to_string()));
                } else {
                    let _ = reply.send(Ok(v.get("result").cloned().unwrap_or(Value::Null)));
                }
            }
            return;
        }
    }

    // Streamed event frame.
    if v.get("method").and_then(|m| m.as_str()) == Some("event") {
        route_event(v.get("params"), app, sessions);
    }
}

fn route_event(params: Option<&Value>, app: &AppHandle, sessions: &Arc<Mutex<Sessions>>) {
    let params = match params {
        Some(p) => p,
        None => return,
    };
    let typ = params.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let session_id = match params.get("session_id").and_then(|s| s.as_str()) {
        Some(s) => s,
        None => return, // e.g. gateway.ready — nothing to route
    };
    let payload = params.get("payload");

    // Only route events for sessions this app owns.
    let widget_id = match sessions
        .lock()
        .ok()
        .and_then(|s| s.by_live.get(session_id).cloned())
    {
        Some(w) => w,
        None => {
            #[cfg(debug_assertions)]
            if typ.starts_with("message.") {
                eprintln!("[notch] chat: dropped {typ} for unowned session {session_id}");
            }
            return;
        }
    };

    let text_field = |keys: &[&str]| -> Option<String> {
        let p = payload?;
        for k in keys {
            if let Some(s) = p.get(*k).and_then(|v| v.as_str()) {
                return Some(s.to_string());
            }
        }
        None
    };

    let (kind, text) = match typ {
        "message.start" => ("start", None),
        "message.delta" => ("delta", text_field(&["text"])),
        "message.complete" => ("complete", text_field(&["text"])),
        "status.update" => ("status", text_field(&["text", "status"])),
        "error" => ("error", text_field(&["message", "error", "text"])),
        t if t.starts_with("tool.") || t.starts_with("reasoning.") => {
            ("status", Some("working".to_string()))
        }
        _ => return,
    };

    let mut ev = json!({ "widgetId": widget_id, "kind": kind });
    if let Some(t) = text {
        ev["text"] = json!(t);
    }
    let _ = app.emit("chat:event", ev);
}

/// Extract the last assistant reply's text from a `session.history` messages list.
fn last_assistant_text(messages: &[Value]) -> Option<String> {
    for msg in messages.iter().rev() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if matches!(role, "assistant" | "agent" | "model") {
            let text = extract_text(msg);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

/// Best-effort text extraction from a message object.
fn extract_text(msg: &Value) -> String {
    if let Some(t) = msg.get("text").and_then(|t| t.as_str()) {
        return t.to_string();
    }
    match msg.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| p.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}
