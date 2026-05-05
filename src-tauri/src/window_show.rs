//! Centralised "surface the main window" pathway.
//!
//! Every code path that wants to make the configuration window visible —
//! cold launch (when not auto-started), `RunEvent::Reopen`, the tray-menu
//! Show item, and tray-icon left-click — funnels through
//! [`show_main_window`]. The helper:
//!
//! 1. Bumps the readiness generation. The bump is what lets the focus-
//!    listener re-ack contract work: if a previous show cycle's mount-time
//!    ack arrived against the old generation, the bump invalidates it,
//!    and the frontend's window-focus listener (see `src/main.tsx`) will
//!    re-invoke `frontend_ready` for the new generation when the window
//!    next gains focus.
//! 2. Recreates the `main` webview if it was destroyed externally
//!    (defensive — nothing in our own code destroys it any more).
//! 3. Reattaches the close-intercept handler via [`attach_window_handlers`].
//! 4. Calls `show()` + `set_focus()`.
//! 5. Refreshes the tray Show/Hide label.
//!
//! ## History
//!
//! An earlier revision (commit `1c08389`) layered a 5-second readiness
//! watchdog and a destroy + rebuild recovery path on top of this helper to
//! recover from a hypothesised wedged-WKWebView state on cold-boot
//! autostart. Real-reboot QA showed the focus-listener re-ack alone is
//! sufficient — the watchdog never fired usefully — so the destroy +
//! rebuild machinery was retired in a follow-up cleanup.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::tray::refresh_tray_menu_labels;
use crate::window_readiness::WindowReadiness;

/// Window label for the configuration window. Single source of truth so
/// the cold-launch, recreate, and tray-menu paths all reference the same
/// constant.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Window dimensions mirrored from `tauri.conf.json`. Kept in code so the
/// recreate path doesn't have to re-read the JSON config — Tauri does not
/// expose the parsed config on `AppHandle` after setup.
const WINDOW_DEFAULT_WIDTH: f64 = 940.0;
const WINDOW_DEFAULT_HEIGHT: f64 = 720.0;
const WINDOW_MIN_WIDTH: f64 = 800.0;
const WINDOW_MIN_HEIGHT: f64 = 560.0;
const WINDOW_TITLE: &str = "Klaar";

/// Public entry point: surface the configuration window.
///
/// MUST be called from the main thread (Tauri window operations require
/// it). All current call sites (setup hook, `RunEvent::Reopen`, tray menu
/// handlers, tray click) already run on the main thread.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(readiness) = app.try_state::<WindowReadiness>() else {
        // Setup didn't manage the readiness state — abort gracefully
        // rather than panicking. (Should never happen in practice — the
        // setup hook installs the state before the first show.)
        log::error!("show_main_window: WindowReadiness not managed");
        return;
    };

    // Bump the generation. Any in-flight ack from a previous show is now
    // stale; the focus-listener path on the frontend will re-ack the new
    // generation when the window gains focus, so callers that want to
    // observe readiness can poll `is_ready_for(live)` afterwards.
    let _g = readiness.bump_generation();

    // Ensure the window exists, attach handlers, show.
    if let Err(e) = ensure_visible(app) {
        log::error!("show_main_window: failed to surface window: {e}");
        return;
    }

    // Keep the tray label synced with the new visibility state.
    refresh_tray_menu_labels(app);
}

/// Attach the close-intercept handler (red close button + `Cmd+W` →
/// hide instead of destroy) to a window. Pulled out of the inline
/// `setup()` registration so the same code runs for the original window
/// AND for any window we recreate via the defensive rebuild path.
pub fn attach_window_handlers<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let app_handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = w.hide();
            }
            refresh_tray_menu_labels(&app_handle);
        }
    });
}

/// Internal: ensure the `main` window exists, attach handlers, then
/// `show()` + `set_focus()`. If the window was destroyed externally the
/// rebuild branch recreates it via `WebviewWindowBuilder` mirroring the
/// `tauri.conf.json` config — defensive only; nothing in our own code
/// destroys the window.
fn ensure_visible<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn std::error::Error>> {
    let window = if let Some(existing) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        existing
    } else {
        log::info!("show_main_window: building main window");
        build_main_window(app)?
    };

    // Always (re)attach the close handler. Tauri's `on_window_event` is
    // additive (multiple listeners stack), so for an already-handled
    // window this would register a second handler. In practice the
    // re-show path either (a) goes through this helper for the first
    // time after the inlined registration in `setup()`, or (b) operates
    // on a freshly-created window that has no handler yet. Idempotency
    // is acceptable: the second handler also calls `prevent_close()` and
    // `hide()`, which are idempotent themselves.
    attach_window_handlers(app, &window);

    window.show()?;
    window.set_focus()?;
    Ok(())
}

fn build_main_window<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<WebviewWindow<R>, Box<dyn std::error::Error>> {
    let window = WebviewWindowBuilder::new(
        app,
        MAIN_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title(WINDOW_TITLE)
    .inner_size(WINDOW_DEFAULT_WIDTH, WINDOW_DEFAULT_HEIGHT)
    .min_inner_size(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)
    .resizable(true)
    .decorations(true)
    // We explicitly `show()` after creation so the unstyled-flash window
    // doesn't appear before the webview is ready.
    .visible(false)
    .build()?;
    Ok(window)
}
