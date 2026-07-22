//! Host HTTP client (PROTOCOL §1). Owns auth: token mode sends the
//! `X-Hermes-Session-Token` header on every request; password mode logs in once
//! and rides the cookie jar, re-logging in a single time on a 401.
//!
//! All network errors surface as user-showable strings — never panics.

use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use url::form_urlencoded::byte_serialize;

use crate::settings::Settings;

/// Widget-host route prefix (PROTOCOL §1.2).
const PLUGIN: &str = "/api/plugins/hermes-notch";

/// A configured HTTP client bound to one host. Cheap to clone (shares the
/// underlying connection pool and cookie jar).
#[derive(Clone)]
pub struct Net {
    client: reqwest::Client,
    base: String,
    token: String,
    username: String,
    password: String,
}

impl Net {
    /// Build a client with a cookie jar (needed for password mode) over rustls.
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .user_agent("hermes-notch/0.1")
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Net {
            client,
            base: settings.base_url(),
            token: settings.token.clone(),
            username: settings.username.clone(),
            password: settings.password.clone(),
        })
    }

    fn token_mode(&self) -> bool {
        !self.token.is_empty()
    }

    /// POST /auth/password-login and store the session cookies.
    pub async fn login(&self) -> Result<(), String> {
        if self.token_mode() {
            return Ok(());
        }
        let url = format!("{}/auth/password-login", self.base);
        let body = json!({
            "provider": "basic",
            "username": self.username,
            "password": self.password,
            "next": "/",
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(net_err)?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("login failed (HTTP {code}): {}", first_line(&text)));
        }
        Ok(())
    }

    /// Send a request, retrying once after a fresh login on a 401 (password mode).
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response, String> {
        let resp = self.dispatch(method.clone(), path, body.clone()).await?;
        if resp.status() == StatusCode::UNAUTHORIZED && !self.token_mode() {
            self.login().await?;
            return self.dispatch(method, path, body).await;
        }
        Ok(resp)
    }

    async fn dispatch(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response, String> {
        let url = format!("{}{}", self.base, path);
        let mut req = self.client.request(method, &url);
        if self.token_mode() {
            req = req.header("X-Hermes-Session-Token", &self.token);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        req.send().await.map_err(net_err)
    }

    async fn get_json(&self, path: &str) -> Result<Value, String> {
        let resp = self.send(Method::GET, path, None).await?;
        read_json(resp).await
    }

    /// GET /health — host info for the settings screen and connection check.
    pub async fn health(&self) -> Result<Value, String> {
        self.get_json(&format!("{PLUGIN}/health")).await
    }

    /// GET /widgets — installed widget specs.
    pub async fn widgets(&self) -> Result<Value, String> {
        self.get_json(&format!("{PLUGIN}/widgets")).await
    }

    /// GET /widgets/{id}/glance — cheap collapsed-line Card.
    pub async fn glance(&self, widget_id: &str) -> Result<Value, String> {
        self.get_json(&format!("{PLUGIN}/widgets/{widget_id}/glance"))
            .await
    }

    /// GET /widgets/{id}/state — full Card.
    pub async fn state(&self, widget_id: &str) -> Result<Value, String> {
        self.get_json(&format!("{PLUGIN}/widgets/{widget_id}/state"))
            .await
    }

    /// POST /widgets/{id}/action — script effects only. Returns `Err("chat-effect")`
    /// for chat effects (HTTP 409); those ride the WebSocket instead.
    pub async fn action(&self, widget_id: &str, action_id: &str) -> Result<Value, String> {
        let resp = self
            .send(
                Method::POST,
                &format!("{PLUGIN}/widgets/{widget_id}/action"),
                Some(json!({ "id": action_id })),
            )
            .await?;
        if resp.status() == StatusCode::CONFLICT {
            return Err("chat-effect".to_string());
        }
        read_json(resp).await
    }

    /// Mint a single-use WebSocket ticket (password mode).
    async fn ws_ticket(&self) -> Result<String, String> {
        let resp = self.send(Method::POST, "/api/auth/ws-ticket", None).await?;
        let v = read_json(resp).await?;
        v.get("ticket")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "ws-ticket response missing 'ticket'".to_string())
    }

    /// Full `ws://…/api/ws?…` URL to dial, minting a fresh ticket if needed.
    /// Tickets are single-use (30s TTL) so this must be called for every dial.
    pub async fn ws_connect_url(&self) -> Result<String, String> {
        let ws_base = self
            .base
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        if self.token_mode() {
            Ok(format!("{ws_base}/api/ws?token={}", enc(&self.token)))
        } else {
            let ticket = self.ws_ticket().await?;
            Ok(format!("{ws_base}/api/ws?ticket={}", enc(&ticket)))
        }
    }
}

async fn read_json(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status.as_u16(), first_line(&text)));
    }
    resp.json::<Value>()
        .await
        .map_err(|e| format!("bad JSON from host: {e}"))
}

fn net_err(e: reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        format!("cannot reach host: {e}")
    } else {
        format!("request failed: {e}")
    }
}

/// First non-empty line of a body, trimmed and length-capped for UI display.
fn first_line(s: &str) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.len() > 200 {
        format!("{}…", &line[..200])
    } else {
        line.to_string()
    }
}

fn enc(s: &str) -> String {
    byte_serialize(s.as_bytes()).collect()
}
