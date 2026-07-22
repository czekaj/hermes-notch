//! Tauri command surface (PROTOCOL §3.1) and shared app state. The Rust core
//! owns all network I/O; the webview only invokes these commands.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::chat::{Chat, ChatStatus};
use crate::geometry::Geometry;
use crate::http::Net;
use crate::settings::{self, Settings};

/// Cross-command shared state. All fields lock briefly; guards are never held
/// across an `.await`.
#[derive(Default)]
pub struct AppState {
    net: Mutex<Option<Net>>,
    chat: Mutex<Option<Chat>>,
    /// widget-id -> spec (from the cached /widgets response).
    widgets: Mutex<HashMap<String, Value>>,
    geometry: Mutex<Option<Geometry>>,
}

impl AppState {
    fn net(&self) -> Option<Net> {
        self.net.lock().ok().and_then(|g| g.clone())
    }
    fn set_net(&self, n: Option<Net>) {
        if let Ok(mut g) = self.net.lock() {
            *g = n;
        }
    }
    fn chat(&self) -> Option<Chat> {
        self.chat.lock().ok().and_then(|g| g.clone())
    }
    fn set_chat(&self, c: Option<Chat>) {
        if let Ok(mut g) = self.chat.lock() {
            *g = c;
        }
    }
    fn set_widgets(&self, w: HashMap<String, Value>) {
        if let Ok(mut g) = self.widgets.lock() {
            *g = w;
        }
    }
    fn spec(&self, id: &str) -> Option<Value> {
        self.widgets.lock().ok().and_then(|g| g.get(id).cloned())
    }
    fn geometry(&self) -> Option<Geometry> {
        self.geometry.lock().ok().and_then(|g| *g)
    }
    /// Store the latest measured geometry (called from setup and commands).
    pub fn set_geometry(&self, geo: Geometry) {
        if let Ok(mut g) = self.geometry.lock() {
            *g = Some(geo);
        }
    }
}

/// PROTOCOL §3.1 `HostInfo`.
#[derive(Serialize)]
pub struct HostInfo {
    ok: bool,
    host_version: String,
    /// Raw `/widgets` entries: `{id, spec}` or `{id, error}`.
    widgets: Vec<Value>,
}

/// PROTOCOL §3.1 `PanelInfo`.
#[derive(Serialize)]
pub struct PanelInfo {
    has_notch: bool,
    notch_width: f64,
    notch_height: f64,
    scale: f64,
}

fn emit_status(app: &AppHandle, state: &str, detail: Option<&str>) {
    let mut v = json!({ "state": state });
    if let Some(d) = detail {
        v["detail"] = json!(d);
    }
    let _ = app.emit("conn:status", v);
}

// ---- settings ----

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
pub fn set_settings(app: AppHandle, patch: Value) -> Result<Settings, String> {
    let next = settings::apply_patch(&app, patch)?;
    apply_autostart(&app, next.autostart);
    Ok(next)
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt as _;
    let al = app.autolaunch();
    let current = al.is_enabled().unwrap_or(false);
    if enabled && !current {
        let _ = al.enable();
    } else if !enabled && current {
        let _ = al.disable();
    }
}

// ---- connection lifecycle ----

#[tauri::command]
pub async fn connect(app: AppHandle, state: State<'_, AppState>) -> Result<HostInfo, String> {
    emit_status(&app, "connecting", None);

    let cfg = settings::load(&app);
    let net = Net::new(&cfg).map_err(|e| fail(&app, e))?;

    net.login().await.map_err(|e| fail(&app, e))?;

    let health = net.health().await.map_err(|e| fail(&app, e))?;
    let ok = health.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let host_version = health
        .get("host_version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let widgets_resp = net.widgets().await.map_err(|e| fail(&app, e))?;
    let items = widgets_resp
        .get("widgets")
        .and_then(|w| w.as_array())
        .cloned()
        .unwrap_or_default();

    // The host returns wrapper items ({id, spec} or {id, error}); the app's
    // contract (HostInfo.widgets: WidgetSpec[]) wants the bare specs. Unwrap
    // once here — passing wrappers through left spec.actions/input/source
    // undefined in the frontend (no buttons, no input, chat never ensured).
    let mut specs = HashMap::new();
    let mut widget_specs = Vec::new();
    for item in &items {
        if let (Some(id), Some(spec)) = (item.get("id").and_then(|i| i.as_str()), item.get("spec")) {
            specs.insert(id.to_string(), spec.clone());
            widget_specs.push(spec.clone());
        }
    }
    state.set_widgets(specs);
    state.set_net(Some(net.clone()));

    // (Re)start the chat transport with the persisted session ids.
    let persisted = settings::load_sessions(&app);
    let chat = Chat::start(app.clone(), net, persisted);
    state.set_chat(Some(chat));

    emit_status(&app, "connected", None);
    Ok(HostInfo {
        ok,
        host_version,
        widgets: widget_specs,
    })
}

/// Emit an error status and pass the message through unchanged.
fn fail(app: &AppHandle, e: String) -> String {
    emit_status(app, "error", Some(&e));
    e
}

#[tauri::command]
pub fn disconnect(app: AppHandle, state: State<'_, AppState>) {
    state.set_chat(None); // dropping the handle stops the WS manager
    state.set_net(None);
    emit_status(&app, "disconnected", None);
}

// ---- cards ----

#[tauri::command]
pub async fn get_glance(state: State<'_, AppState>, widget_id: String) -> Result<Value, String> {
    let net = state.net().ok_or("not connected")?;
    net.glance(&widget_id).await
}

#[tauri::command]
pub async fn get_state(state: State<'_, AppState>, widget_id: String) -> Result<Value, String> {
    let net = state.net().ok_or("not connected")?;
    net.state(&widget_id).await
}

#[tauri::command]
pub async fn run_action(
    state: State<'_, AppState>,
    widget_id: String,
    action_id: String,
) -> Result<Option<Value>, String> {
    let net = state.net().ok_or("not connected")?;
    // `Err("chat-effect")` here means the action rides the chat transport.
    let card = net.action(&widget_id, &action_id).await?;
    Ok(Some(card))
}

// ---- chat ----

fn on_start_of(state: &State<'_, AppState>, widget_id: &str) -> Option<String> {
    state.spec(widget_id).and_then(|s| {
        s.get("source")
            .and_then(|src| src.get("on_start"))
            .and_then(|v| v.as_str())
            .map(String::from)
    })
}

#[tauri::command]
pub async fn chat_ensure(
    state: State<'_, AppState>,
    widget_id: String,
) -> Result<ChatStatus, String> {
    let chat = state.chat().ok_or("not connected")?;
    chat.ensure(&widget_id).await
}

#[tauri::command]
pub async fn chat_send(
    state: State<'_, AppState>,
    widget_id: String,
    text: String,
) -> Result<(), String> {
    let chat = state.chat().ok_or("not connected")?;
    let on_start = on_start_of(&state, &widget_id);
    chat.send(&widget_id, &text, on_start).await
}

#[tauri::command]
pub async fn chat_reset(
    state: State<'_, AppState>,
    widget_id: String,
) -> Result<(), String> {
    let chat = state.chat().ok_or("not connected")?;
    chat.reset(&widget_id).await
}

#[tauri::command]
pub async fn chat_history(
    state: State<'_, AppState>,
    widget_id: String,
) -> Result<String, String> {
    let chat = state.chat().ok_or("not connected")?;
    chat.history(&widget_id).await
}

#[tauri::command]
pub async fn chat_interrupt(
    state: State<'_, AppState>,
    widget_id: String,
) -> Result<(), String> {
    let chat = state.chat().ok_or("not connected")?;
    chat.interrupt(&widget_id).await
}

// ---- panel geometry ----

#[tauri::command]
pub fn set_expanded(
    app: AppHandle,
    state: State<'_, AppState>,
    expanded: bool,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    let fallback = state.geometry().unwrap_or_default();
    let measured = width.zip(height);
    let geo = measure_and_apply(&app, expanded, measured, fallback)?;
    state.set_geometry(geo);
    Ok(())
}

#[tauri::command]
pub fn panel_info(app: AppHandle, state: State<'_, AppState>) -> PanelInfo {
    let fallback = state.geometry().unwrap_or_default();
    let geo = measure(&app, fallback);
    state.set_geometry(geo);
    PanelInfo {
        has_notch: geo.has_notch,
        notch_width: geo.notch_width,
        notch_height: geo.notch_height,
        scale: geo.scale,
    }
}

/// Re-measure geometry on the main thread, fold in the frontend-measured shape
/// size for this state, and apply the panel frame.
#[cfg(target_os = "macos")]
fn measure_and_apply(
    app: &AppHandle,
    expanded: bool,
    measured: Option<(f64, f64)>,
    fallback: Geometry,
) -> Result<Geometry, String> {
    use crate::{geometry, panel};
    let (tx, rx) = std::sync::mpsc::channel();
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let mut geo = objc2_foundation::MainThreadMarker::new()
            .map(geometry::compute)
            .unwrap_or_default();
        geo.carry_measured_from(&fallback);
        if let Some((w, h)) = measured {
            geo.set_measured(expanded, w, h);
        }
        panel::apply_frame(&app2, &geo, expanded);
        let _ = tx.send(geo);
    })
    .map_err(|e| e.to_string())?;
    Ok(rx
        .recv_timeout(std::time::Duration::from_millis(800))
        .unwrap_or(fallback))
}

#[cfg(not(target_os = "macos"))]
fn measure_and_apply(
    _app: &AppHandle,
    _expanded: bool,
    _measured: Option<(f64, f64)>,
    fallback: Geometry,
) -> Result<Geometry, String> {
    Ok(fallback)
}

/// Re-measure geometry on the main thread (no frame change).
#[cfg(target_os = "macos")]
fn measure(app: &AppHandle, fallback: Geometry) -> Geometry {
    use crate::geometry;
    let (tx, rx) = std::sync::mpsc::channel();
    if app
        .run_on_main_thread(move || {
            let mut geo = objc2_foundation::MainThreadMarker::new()
                .map(geometry::compute)
                .unwrap_or_default();
            geo.carry_measured_from(&fallback);
            let _ = tx.send(geo);
        })
        .is_ok()
    {
        if let Ok(g) = rx.recv_timeout(std::time::Duration::from_millis(800)) {
            return g;
        }
    }
    fallback
}

#[cfg(not(target_os = "macos"))]
fn measure(_app: &AppHandle, fallback: Geometry) -> Geometry {
    fallback
}

// ---- utilities ----

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}
