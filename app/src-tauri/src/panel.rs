//! NSPanel lifecycle: turn the "main" window into a non-activating, always-on-top
//! notch panel, wire the hover tracking area, and drive its frame.
//!
//! macOS-only; a no-op stub is provided elsewhere so the crate still builds.

#[cfg(target_os = "macos")]
mod imp {
    // NOTE: MainThreadMarker, NSPoint, NSRect and NSSize are brought into scope
    // by the `tauri_panel!` macro expansion below, so they are not imported here.
    use serde_json::json;
    use tauri::{AppHandle, Emitter, Manager};
    use tauri_nspanel::{
        tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, TrackingAreaOptions,
        WebviewWindowExt,
    };

    use crate::geometry::{self, Geometry};

    tauri_panel! {
        panel!(NotchPanel {
            config: {
                can_become_main_window: false,
                can_become_key_window: true,
                becomes_key_only_if_needed: true,
                is_floating_panel: true
            }
            with: {
                tracking_area: {
                    options: TrackingAreaOptions::new()
                        .active_always()
                        .mouse_entered_and_exited()
                        .mouse_moved(),
                    auto_resize: true
                }
            }
        })

        panel_event!(NotchPanelHandler {})
    }

    /// Convert the main window into the notch panel, position the collapsed pill,
    /// wire hover events, and show it. Returns the measured geometry.
    /// Runs during Tauri `setup`, i.e. on the main thread.
    pub fn setup(app: &AppHandle) -> Result<Geometry, String> {
        let window = app
            .get_webview_window("main")
            .ok_or("main window is missing")?;

        // Apply HUD vibrancy before converting to a panel.
        window_vibrancy::apply_vibrancy(
            &window,
            window_vibrancy::NSVisualEffectMaterial::HudWindow,
            None,
            None,
        )
        .map_err(|e| format!("failed to apply vibrancy: {e}"))?;

        let panel = window
            .to_panel::<NotchPanel>()
            .map_err(|e| format!("failed to create panel: {e}"))?;

        // Above the menu bar and full-screen apps; never activates the app.
        panel.set_level(PanelLevel::ScreenSaver.value());
        panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .stationary()
                .into(),
        );
        panel.set_hides_on_deactivate(false);

        // Measure the notch and position the collapsed pill before showing.
        let mtm = MainThreadMarker::new().ok_or("panel setup must run on the main thread")?;
        let geo = geometry::compute(mtm);
        apply_frame(app, &geo, false);

        // Tracking-area hover callbacks → "notch:hover" { entered } (PROTOCOL §3.2).
        let handler = NotchPanelHandler::new();
        let enter_app = app.clone();
        handler.on_mouse_entered(move |_event| {
            let _ = enter_app.emit("notch:hover", json!({ "entered": true }));
        });
        let exit_app = app.clone();
        handler.on_mouse_exited(move |_event| {
            let _ = exit_app.emit("notch:hover", json!({ "entered": false }));
        });
        panel.set_event_handler(Some(handler.as_ref()));

        // Show without activating the app.
        panel.show();

        Ok(geo)
    }

    /// Set the panel frame for the given state, anchored to the notch top-center.
    /// MUST be called on the main thread.
    pub fn apply_frame(app: &AppHandle, geo: &Geometry, expanded: bool) {
        let (x, y, w, h) = geo.cocoa_frame(expanded);
        if let Ok(panel) = app.get_webview_panel("main") {
            let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
            // NSPanel derefs to NSWindow; set the full frame in Cocoa coords.
            panel.as_panel().setFrame_display(rect, true);
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::{apply_frame, setup};

#[cfg(not(target_os = "macos"))]
pub fn setup(_app: &tauri::AppHandle) -> Result<crate::geometry::Geometry, String> {
    Ok(crate::geometry::Geometry::default())
}

#[cfg(not(target_os = "macos"))]
pub fn apply_frame(_app: &tauri::AppHandle, _geo: &crate::geometry::Geometry, _expanded: bool) {}
