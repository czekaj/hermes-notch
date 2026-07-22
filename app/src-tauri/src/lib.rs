//! Hermes Notch — Tauri core. Wires plugins, the notch panel, the global
//! shortcut, and the tray, and registers the command surface (PROTOCOL §3).

mod chat;
mod commands;
mod geometry;
mod http;
mod panel;
mod settings;

use commands::AppState;
use tauri::{AppHandle, Emitter, Manager};

/// Build and run the application. Blocks until the app exits.
pub fn run() {
    tauri::Builder::default()
        // single-instance MUST be first in the chain (Rust-only plugin).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch summons the HUD.
            let _ = app.emit("notch:shortcut", serde_json::json!({}));
        }))
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_nspanel::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::connect,
            commands::disconnect,
            commands::get_glance,
            commands::get_state,
            commands::run_action,
            commands::chat_ensure,
            commands::chat_send,
            commands::chat_reset,
            commands::chat_history,
            commands::chat_interrupt,
            commands::set_expanded,
            commands::panel_info,
            commands::open_url,
            commands::copy_text,
        ])
        .setup(|app| {
            // Hide the Dock icon — this is a menu-bar/notch accessory.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            // Build the notch panel and cache the measured geometry.
            match panel::setup(&handle) {
                Ok(geo) => app.state::<AppState>().set_geometry(geo),
                Err(e) => eprintln!("hermes-notch: panel setup failed: {e}"),
            }

            register_shortcut(&handle);

            if let Err(e) = build_tray(&handle) {
                eprintln!("hermes-notch: tray setup failed: {e}");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Hermes Notch");
}

/// Register the global summon shortcut (⌥Space) → "notch:shortcut".
fn register_shortcut(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

    let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::Space);
    let app2 = app.clone();
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                let _ = app2.emit("notch:shortcut", serde_json::json!({}));
            }
        });
    if let Err(e) = result {
        eprintln!("hermes-notch: failed to register global shortcut: {e}");
    }
}

/// Build the menu-bar tray: Show/Hide HUD, Settings, Quit.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::image::Image;
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let toggle = MenuItemBuilder::with_id("toggle", "Show/Hide HUD").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&toggle, &settings])
        .separator()
        .item(&quit)
        .build()?;

    // Template icon (black + alpha) renders correctly in light/dark menu bars.
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;

    TrayIconBuilder::with_id("hermes-notch")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            // Both toggling the HUD and opening Settings are summon signals the
            // webview interprets (it owns expand/collapse per PROTOCOL §3.3).
            "toggle" | "settings" => {
                let _ = app.emit("notch:shortcut", serde_json::json!({}));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
