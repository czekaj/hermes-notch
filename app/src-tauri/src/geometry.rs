//! Notch geometry: measure the built-in display's notch (or a fallback strip on
//! external displays) and derive the collapsed/expanded panel frames.
//!
//! Frames are returned in Cocoa's bottom-left-origin coordinate space (points),
//! the same space as `NSScreen::frame` and `NSWindow::setFrame:display:`, so no
//! coordinate flip is needed when positioning the panel.
//!
//! `compute` reads `NSScreen`, so it MUST run on the main thread; callers pass a
//! `MainThreadMarker` obtained there.

#[cfg(target_os = "macos")]
use objc2_app_kit::NSScreen;
#[cfg(target_os = "macos")]
use objc2_foundation::MainThreadMarker;

/// The measured notch/strip and the frames derived from it.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// True when the main display physically has a notch (safe-area top inset).
    pub has_notch: bool,
    /// Width of the notch cut-out (or the fallback strip width) in points.
    pub notch_width: f64,
    /// Height of the notch (safe-area top inset) or the menu-bar height.
    pub notch_height: f64,
    /// Backing scale factor of the main display.
    pub scale: f64,

    /// Main display frame in Cocoa (bottom-left origin) points.
    screen_x: f64,
    screen_y: f64,
    screen_w: f64,
    screen_h: f64,

    collapsed_w: f64,
    collapsed_h: f64,
    expanded_w: f64,
    expanded_h: f64,
}

impl Default for Geometry {
    fn default() -> Self {
        // Reasonable non-notch fallback used before the first measurement.
        Geometry {
            has_notch: false,
            notch_width: 300.0,
            notch_height: 32.0,
            scale: 2.0,
            screen_x: 0.0,
            screen_y: 0.0,
            screen_w: 1440.0,
            screen_h: 900.0,
            collapsed_w: 300.0,
            collapsed_h: 32.0,
            expanded_w: 420.0,
            expanded_h: 340.0,
        }
    }
}

impl Geometry {
    /// Cocoa-space `(x, y, width, height)` for the requested state, anchored so
    /// the window's top edge stays glued to the screen top and it stays centered
    /// on the notch (top-center of the main display).
    pub fn cocoa_frame(&self, expanded: bool) -> (f64, f64, f64, f64) {
        let (w, h) = if expanded {
            (self.expanded_w, self.expanded_h)
        } else {
            (self.collapsed_w, self.collapsed_h)
        };
        let screen_top = self.screen_y + self.screen_h; // Cocoa top edge (y-up)
        let x = self.screen_x + (self.screen_w - w) / 2.0;
        let y = screen_top - h; // bottom origin so the top edge == screen_top
        (x, y, w, h)
    }
}

/// Measure the current main-display geometry. Main-thread only.
#[cfg(target_os = "macos")]
pub fn compute(mtm: MainThreadMarker) -> Geometry {
    let main = match NSScreen::mainScreen(mtm) {
        Some(s) => s,
        None => return Geometry::default(),
    };

    let frame = main.frame();
    let visible = main.visibleFrame();
    let insets = main.safeAreaInsets();
    let scale = main.backingScaleFactor();

    let has_notch = insets.top > 0.0;
    let (notch_width, notch_height) = if has_notch {
        // notch width = full width minus the usable menu-bar areas either side.
        let left = main.auxiliaryTopLeftArea();
        let right = main.auxiliaryTopRightArea();
        let nw = frame.size.width - left.size.width - right.size.width;
        let nw = if nw > 0.0 { nw } else { 180.0 };
        (nw, insets.top)
    } else {
        // Menu-bar height = gap between the frame top and the visible-area top
        // (isolates the top inset regardless of the Dock at the bottom).
        let menu = (frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height);
        (300.0, menu.clamp(24.0, 40.0))
    };

    build(
        has_notch,
        notch_width,
        notch_height,
        scale,
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    )
}

/// Assemble a `Geometry`, deriving the collapsed/expanded window sizes.
#[allow(clippy::too_many_arguments)]
fn build(
    has_notch: bool,
    notch_width: f64,
    notch_height: f64,
    scale: f64,
    screen_x: f64,
    screen_y: f64,
    screen_w: f64,
    screen_h: f64,
) -> Geometry {
    // Collapsed: on a notch, extend the black pill 320pt so the glance "wings"
    // show beside the camera island; on external displays, a fixed 300pt strip.
    let collapsed_w = if has_notch { notch_width + 320.0 } else { 300.0 };
    let collapsed_h = notch_height;
    // Expanded: 420 wide, or wider if the collapsed pill already exceeds it.
    let expanded_w = collapsed_w.max(420.0);
    let expanded_h = 340.0;

    Geometry {
        has_notch,
        notch_width,
        notch_height,
        scale,
        screen_x,
        screen_y,
        screen_w,
        screen_h,
        collapsed_w,
        collapsed_h,
        expanded_w,
        expanded_h,
    }
}
