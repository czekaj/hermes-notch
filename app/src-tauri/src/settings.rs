//! Persisted settings, backed by tauri-plugin-store ("settings.json").
//!
//! Also stores the per-widget chat session map (`sessions` key: widget-id ->
//! durable stored_session_id) so sessions survive app restarts.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// Store file name (also the key namespace) used by the whole app.
pub const STORE_FILE: &str = "settings.json";
const SESSIONS_KEY: &str = "sessions";

/// User-facing connection settings. Mirrors PROTOCOL §3.1 `Settings`.
///
/// NOTE(security): `password` is persisted in plaintext inside `settings.json`
/// for v1. TODO: move the password (and token) into the macOS Keychain.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// Loopback dev session token; "" selects password mode.
    pub token: String,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            host: "127.0.0.1".into(),
            port: 9119,
            username: String::new(),
            password: String::new(),
            token: String::new(),
            autostart: false,
        }
    }
}

impl Settings {
    /// `http://host:port` base URL for the host HTTP API.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Load settings, falling back to defaults for any missing/invalid field.
pub fn load(app: &AppHandle) -> Settings {
    let store = match app.store(STORE_FILE) {
        Ok(s) => s,
        Err(_) => return Settings::default(),
    };
    let mut map = Map::new();
    for key in ["host", "port", "username", "password", "token", "autostart"] {
        if let Some(v) = store.get(key) {
            map.insert(key.to_string(), v);
        }
    }
    // Overlay stored values on defaults so partial/legacy files still load.
    let mut base = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
    if let (Value::Object(base_map), true) = (&mut base, !map.is_empty()) {
        for (k, v) in map {
            base_map.insert(k, v);
        }
    }
    serde_json::from_value(base).unwrap_or_default()
}

/// Apply a partial patch (any subset of the settings fields) and persist.
pub fn apply_patch(app: &AppHandle, patch: Value) -> Result<Settings, String> {
    let current = load(app);
    let mut merged = serde_json::to_value(&current).map_err(|e| e.to_string())?;
    if let (Value::Object(dst), Value::Object(src)) = (&mut merged, &patch) {
        for (k, v) in src {
            if dst.contains_key(k) {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
    let next: Settings = serde_json::from_value(merged)
        .map_err(|e| format!("invalid settings patch: {e}"))?;

    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set("host", json!(next.host));
    store.set("port", json!(next.port));
    store.set("username", json!(next.username));
    store.set("password", json!(next.password));
    store.set("token", json!(next.token));
    store.set("autostart", json!(next.autostart));
    store.save().map_err(|e| e.to_string())?;
    Ok(next)
}

/// Load the persisted widget-id -> stored_session_id map.
pub fn load_sessions(app: &AppHandle) -> HashMap<String, String> {
    let store = match app.store(STORE_FILE) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    match store.get(SESSIONS_KEY) {
        Some(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect(),
        _ => HashMap::new(),
    }
}

/// Persist the widget-id -> stored_session_id map.
pub fn save_sessions(app: &AppHandle, sessions: &HashMap<String, String>) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let obj: Map<String, Value> = sessions
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    store.set(SESSIONS_KEY, Value::Object(obj));
    store.save().map_err(|e| e.to_string())
}
