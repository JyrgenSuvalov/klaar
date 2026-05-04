//! Menu bar tray icon management.
//!
//! Visual states (priority `Red > Muted > Yellow > Green`):
//!
//! | Variant  | Meaning                                                             |
//! |----------|---------------------------------------------------------------------|
//! | `Green`  | streaming AND processing enabled                                    |
//! | `Yellow` | passthrough (processing disabled) OR post-install reboot pending    |
//! | `Muted`  | global mute is engaged (post-limiter silence). Wins over            |
//! |          | Yellow/Green; loses to Red.                                         |
//! | `Red`    | error / device disconnected — wins over everything                  |
//!
//! Callers continue to pass the "base" state they computed from local context
//! (e.g. `set_processing_enabled` decides Red/Yellow/Green from running +
//! enabled flags). The reboot-pending overlay is layered on top by
//! [`set_tray_state`] itself reading the engine's `reboot_pending` atomic, so
//! every existing call site picks it up for free — when set, it promotes
//! Green to Yellow (Yellow stays Yellow, Red still wins).
//!
//! Tooltip composition tracks the dual meaning of Yellow: when reboot-pending
//! is set, the tooltip becomes the reboot-required copy regardless of the
//! base; the passthrough copy is only used when reboot-pending is NOT set.
//! Red+reboot-pending combines both messages. See `app-shell` spec for the
//! user-facing copy.
//!
//! An April 2026 change folded the previous standalone `Orange` variant
//! into `Yellow` — three icons proved a clearer signal than four. The
//! priority/overlay machinery is unchanged; only the resolved variant and
//! tooltip mapping differ.
//!
//! ## Right-click context menu
//!
//! Because the app runs `LSUIElement=true` (no menu bar, no Dock), the only
//! quit affordance available once the window is hidden is the tray. The tray
//! exposes a two-item right-click menu:
//!
//! 1. `show-hide` — toggles window visibility. Its label MUST track current
//!    visibility: "Hide Klaar" while shown, "Show Klaar" while
//!    hidden. Every site that flips visibility (`CloseRequested` handler,
//!    [`handle_tray_click`], `setup()` cold launch, `RunEvent::Reopen`) is
//!    required to call [`refresh_tray_menu_labels`] right after, otherwise
//!    the label drifts. See `app-shell` spec "Show/Hide menu item label
//!    tracks visibility".
//! 2. `quit` — calls `app.exit(0)` unconditionally.
//!
//! Left-click behavior is unchanged (`show_menu_on_left_click(false)`); the
//! menu only opens on macOS right-click.

use tauri::{
    image::Image,
    menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime,
};

use crate::commands::EngineHandle;
use crate::engine_state::AudioEngineState;

/// Newtype wrapper so we can store the tray's menu in app-managed state
/// without colliding with any other consumer of `Menu<R>`.
struct TrayMenu<R: Runtime>(Menu<R>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Green,
    Yellow,
    /// Global mute engaged. Distinct icon (`tray-muted.png`) and tooltip
    /// (`"Klaar — muted (⌃⇧M to unmute)"`). Promoted over Yellow/Green by
    /// the resolver whenever `DspParams::is_muted()` returns true; Red still
    /// wins the icon, but its tooltip composes both lines so the user sees
    /// the mute state isn't lost behind the error.
    Muted,
    Red,
}

/// Menu item id for the About item.
const MENU_ID_ABOUT: &str = "about";
/// Menu item id for the show/hide toggle.
const MENU_ID_SHOW_HIDE: &str = "show-hide";
/// Menu item id for the quit action.
const MENU_ID_QUIT: &str = "quit";
/// Menu item id for the global mute toggle.
const MENU_ID_MUTE: &str = "mute";

/// Label of the About item. The trailing horizontal-ellipsis (`…`, U+2026,
/// not three dots) follows macOS Human Interface Guidelines for menu items
/// that open a dialog.
const LABEL_ABOUT: &str = "About Klaar…";
/// Label shown on the show/hide menu item while the window is visible.
const LABEL_HIDE: &str = "Hide Klaar";
/// Label shown on the show/hide menu item while the window is hidden.
const LABEL_SHOW: &str = "Show Klaar";
/// Label of the unconditional quit menu item.
const LABEL_QUIT: &str = "Quit Klaar";
/// Label shown on the mute item while *unmuted* — clicking it will mute. The
/// accelerator hint is inlined into the label string rather than passed as a
/// `MenuItem` accelerator: a menu-item accelerator on macOS only fires while
/// the host window is focused, but the global ⌃⇧M chord is owned by the
/// `tauri-plugin-global-shortcut` registration in `lib.rs::setup`. Putting
/// it in the text gives the discoverability without registering twice.
const LABEL_MUTE: &str = "Mute Klaar  ⌃⇧M";
/// Label shown on the mute item while *muted* — clicking it will unmute.
/// Leading `✓` (U+2713) is the spec's mandated mute-state indicator.
const LABEL_UNMUTE: &str = "✓ Unmute Klaar  ⌃⇧M";

/// Action a tray context-menu item triggers. Side effects live in
/// `handle_menu_event`; this enum is the pure intent the resolver returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Open the in-app About dialog (and surface the window if hidden).
    OpenAbout,
    /// Toggle the configuration window's visibility.
    ToggleWindow,
    /// Toggle the global mute state via the shared chokepoint in
    /// `commands::mute`. Side effect: emits `klaar://mute-changed`, which
    /// drives the icon/tooltip refresh and the menu-label refresh.
    ToggleMute,
    /// Quit the app cleanly via `app.exit(0)`.
    Quit,
}

/// Pure resolver mapping a menu item id to its [`MenuAction`]. Unknown ids
/// return `None`, leaving `handle_menu_event` as a no-op for them. Mirrors
/// the existing [`show_hide_label`] pattern so the id-to-action mapping is
/// exhaustively unit-testable without spinning up a `MockRuntime`.
pub fn menu_action_for(id: &str) -> Option<MenuAction> {
    match id {
        MENU_ID_ABOUT => Some(MenuAction::OpenAbout),
        MENU_ID_SHOW_HIDE => Some(MenuAction::ToggleWindow),
        MENU_ID_MUTE => Some(MenuAction::ToggleMute),
        MENU_ID_QUIT => Some(MenuAction::Quit),
        _ => None,
    }
}

/// Tooltip strings matched 1:1 with the spec scenarios.
const TOOLTIP_GREEN: &str = "Klaar — streaming";
const TOOLTIP_YELLOW: &str = "Klaar — processing disabled (passthrough)";
const TOOLTIP_RED: &str = "Klaar — error";
/// Tooltip used for the Yellow + reboot-pending overlay. Takes precedence
/// over `TOOLTIP_YELLOW` whenever the engine's `reboot_pending` atomic is set.
const TOOLTIP_REBOOT_PENDING: &str = "Reboot required to activate Klaar driver";
/// Composite tooltip for the Red+reboot-pending scenario (see `app-shell`
/// spec "Red takes precedence over reboot pending").
const TOOLTIP_RED_AND_REBOOT_PENDING: &str =
    "Klaar — error\nReboot required to activate Klaar driver";
/// Tooltip used when the icon is Muted (no error overlay).
const TOOLTIP_MUTED: &str = "Klaar — muted (⌃⇧M to unmute)";
/// Composite tooltip for the Red+muted scenario. Red wins the icon but the
/// user must still see the mute state — otherwise unmuting after the error
/// clears would be invisible.
const TOOLTIP_RED_AND_MUTED: &str = "Klaar — error\nMuted (⌃⇧M to unmute)";

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Initialise the tray icon. Call once from `tauri::Builder::setup`.
///
/// On startup we don't yet know the audio state, so we render Yellow
/// (passthrough) — the first `set_tray_state` from `try_start_engine` /
/// `set_processing_enabled` will overwrite it within a few hundred ms. If
/// `reboot_pending` was seeded from disk, the icon stays Yellow but
/// the tooltip flips to "Reboot required…".
pub fn init_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn std::error::Error>> {
    let engine_handle: tauri::State<'_, EngineHandle> = app.state();
    // Initial mute is always false (DspParams default + never persisted),
    // but read it anyway for symmetry with the rest of the resolver
    // call sites — that way a future change to the default doesn't have
    // to hunt down hidden hard-coded `false`s.
    let initial = resolve_tray_state(
        &engine_handle.state,
        TrayState::Yellow,
        engine_handle.dsp_params.is_muted(),
    );
    let icon = load_icon(app, initial)?;

    // Build the right-click context menu. The `show-hide` item starts with
    // the "Hide" label because cold-launch shows the window in `setup()`;
    // any later visibility flip is responsible for calling
    // `refresh_tray_menu_labels` so the label keeps tracking reality.
    // Menu order (with About kept as the first item per established
    // convention):
    //   1. About Klaar…
    //   2. ─── separator ───
    //   3. Mute Klaar  ⌃⇧M  (or "✓ Unmute Klaar  ⌃⇧M" while muted)
    //   4. ─── separator ───
    //   5. Show/Hide Klaar
    //   6. Quit Klaar
    //
    // The mute item's initial label is read off `DspParams::is_muted()` for
    // symmetry with the icon resolver — even though the default is always
    // unmuted (mute is never persisted), reading the live value means a
    // future change to the default doesn't have to hunt down a hidden
    // hard-coded `false`.
    let about = MenuItem::with_id(app, MENU_ID_ABOUT, LABEL_ABOUT, true, None::<&str>)?;
    let separator_top = PredefinedMenuItem::separator(app)?;
    let mute = MenuItem::with_id(
        app,
        MENU_ID_MUTE,
        mute_label(engine_handle.dsp_params.is_muted()),
        true,
        None::<&str>,
    )?;
    let separator_mid = PredefinedMenuItem::separator(app)?;
    let show_hide = MenuItem::with_id(
        app,
        MENU_ID_SHOW_HIDE,
        LABEL_HIDE,
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, MENU_ID_QUIT, LABEL_QUIT, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &about,
            &separator_top,
            &mute,
            &separator_mid,
            &show_hide,
            &quit,
        ],
    )?;

    // Stash the menu in app-managed state so `refresh_tray_menu_labels` can
    // look up the show-hide item by id from anywhere. (TrayIcon does not
    // expose a `menu()` getter, so we keep our own handle.)
    app.manage(TrayMenu(menu.clone()));

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        // Template mode: AppKit treats the alpha channel as a silhouette and
        // tints it to match the menu bar appearance (light/dark mode, focus,
        // reduce-transparency, high-contrast). Non-negotiable for menu-bar
        // extras — colour-encoded state is what the redesigned glyph family
        // (#47) explicitly avoids.
        .icon_as_template(true)
        .menu(&menu)
        // macOS opens tray menus on right-click by default; keep left-click
        // bound to the existing toggle handler.
        .show_menu_on_left_click(false)
        .tooltip(tooltip_for(initial))
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                handle_tray_click(app);
            }
        })
        .build(app)?;
    drop(tray);

    Ok(())
}

/// Update the tray icon to reflect a "base" state computed by the caller.
///
/// The base is then passed through [`resolve_tray_state`] to apply the
/// `Red > Yellow(reboot-pending) > Green` priority — meaning callers do NOT
/// need to know about reboot-pending; the overlay is applied uniformly here.
///
/// Always called on the non-audio thread (command handlers, device monitor).
pub fn set_tray_state<R: Runtime>(app: &AppHandle<R>, base: TrayState) {
    // Reading the engine state + mute atomics from a tray-update path is
    // cheap and correct — we want every status update to re-evaluate
    // against current `reboot_pending` and `mute_target` values.
    let resolved = match app.try_state::<EngineHandle>() {
        Some(handle) => {
            let muted = handle.dsp_params.is_muted();
            resolve_tray_state(&handle.state, base, muted)
        }
        None => base, // setup hasn't `manage`d the handle yet
    };
    apply_tray_state(app, resolved);
}

/// Refresh the tray purely from current state (e.g. after `reboot_pending`
/// flips). Recomputes from the last known "base" by inferring it from the
/// engine running flag + processing_enabled. Public so the
/// `set_reboot_pending` IPC can re-render without needing to know the
/// caller's last base.
pub fn refresh_tray_for_reboot_pending_change<R: Runtime>(app: &AppHandle<R>) {
    let Some(handle) = app.try_state::<EngineHandle>() else {
        return;
    };
    let base = infer_base_state(&handle);
    let muted = handle.dsp_params.is_muted();
    apply_tray_state(app, resolve_tray_state(&handle.state, base, muted));
}

/// Refresh the tray icon + tooltip + mute menu label when the global mute
/// state flips.
///
/// Mirrors [`refresh_tray_for_reboot_pending_change`]: re-infers the base
/// from the running engine, re-resolves through the priority rule, and
/// re-applies the icon and tooltip. Also refreshes the right-click menu's
/// mute item label (`Mute Klaar` ↔ `✓ Unmute Klaar`) so all three signals
/// stay in lockstep.
///
/// Wired to the `klaar://mute-changed` event in `lib.rs::run`'s setup hook
/// so a hotkey, IPC, or tray-menu toggle all converge on the same code path.
pub fn refresh_tray_for_mute_change<R: Runtime>(app: &AppHandle<R>) {
    let Some(handle) = app.try_state::<EngineHandle>() else {
        return;
    };
    let base = infer_base_state(&handle);
    let muted = handle.dsp_params.is_muted();
    apply_tray_state(app, resolve_tray_state(&handle.state, base, muted));
    // Single entry point for mute-driven UI refresh — folding the label
    // refresh in here means callers (and the event listener) can't forget
    // one of the two updates.
    refresh_tray_mute_label(app);
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure resolver — easy to unit test, no Tauri dependencies
// ─────────────────────────────────────────────────────────────────────────────

/// Apply the `Red > Muted > Yellow(reboot-pending) > Green` priority rule.
///
/// Pure function — no Tauri, no I/O — so the priority logic is exhaustively
/// unit-testable without spinning up a `MockRuntime`. Note that Yellow is
/// used for both passthrough and reboot-pending; the tooltip disambiguates
/// (see [`tooltip_for`] and [`apply_tray_state`]).
///
/// `muted` is passed in explicitly (rather than read off `DspParams` here)
/// so the resolver stays a pure function. The single live caller
/// ([`set_tray_state`]) reads `DspParams::is_muted()` and forwards it.
pub fn resolve_tray_state(
    state: &AudioEngineState,
    base: TrayState,
    muted: bool,
) -> TrayState {
    // Red wins over everything. We don't surface a "Red+muted" icon
    // variant — composition happens in the tooltip instead.
    if matches!(base, TrayState::Red) {
        return TrayState::Red;
    }
    // Muted next: an active mute is the user's most recent intent and the
    // most actionable signal. It's louder than reboot-pending because the
    // user just toggled it; reboot-pending has been there since install.
    if muted {
        return TrayState::Muted;
    }
    if state.is_reboot_pending() {
        return TrayState::Yellow;
    }
    base
}

/// Tooltip copy for a resolved tray state, ignoring overlays. Callers that
/// know about active overlays (reboot-pending) should use the more specific
/// helpers below.
pub fn tooltip_for(resolved: TrayState) -> &'static str {
    match resolved {
        TrayState::Green => TOOLTIP_GREEN,
        TrayState::Yellow => TOOLTIP_YELLOW,
        TrayState::Muted => TOOLTIP_MUTED,
        TrayState::Red => TOOLTIP_RED,
    }
}

/// Composite tooltip for the Red+muted scenario — Red wins the icon, but
/// we still want the user to see that mute is also active.
pub fn tooltip_red_and_muted() -> &'static str {
    TOOLTIP_RED_AND_MUTED
}

/// Tooltip used when the icon is Yellow specifically because of a pending
/// reboot. Since Yellow now covers both passthrough and reboot-pending, the
/// tooltip is the only signal that distinguishes them.
pub fn tooltip_reboot_pending() -> &'static str {
    TOOLTIP_REBOOT_PENDING
}

/// The Red-and-reboot-pending composite tooltip — Red wins the icon, but we
/// still want the user to see that a reboot is also required.
pub fn tooltip_red_and_reboot_pending() -> &'static str {
    TOOLTIP_RED_AND_REBOOT_PENDING
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

fn apply_tray_state<R: Runtime>(app: &AppHandle<R>, resolved: TrayState) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Ok(icon) = load_icon(app, resolved) {
        let _ = tray.set_icon(Some(icon));
        // Re-apply the template flag after every icon swap. Upstream
        // `tray-icon` (≤0.21.3 at time of writing) hardcodes
        // `is_template = false` inside its macOS `set_icon` implementation,
        // ignoring the value we passed at builder time. Without this line,
        // the cold-launch icon is correctly tinted but flips to raw black
        // pixels on the first state update — visible in dark mode as a
        // pure-black silhouette that ignores the menu bar appearance.
        // Re-asserting the flag here forces `nsimage.setTemplate(true)` on
        // the freshly-installed NSImage and restores AppKit's auto-tinting.
        let _ = tray.set_icon_as_template(true);
    }

    // Tooltip composition handles overlays that the icon variant alone
    // can't communicate:
    //   - Red + muted → composite "error + muted" copy. Mute survived the
    //     error; the user needs to see both signals.
    //   - Red + reboot-pending → composite "error + reboot" copy.
    //   - Yellow + reboot-pending → reboot copy (passthrough copy is
    //     suppressed; reboot is the more actionable signal).
    //   - Otherwise → the per-variant copy from `tooltip_for`.
    //
    // Red+muted+reboot-pending is theoretically possible but the spec
    // gives mute priority because it's the more recent user action; the
    // reboot-pending state is still surfaced in the System Settings
    // affordance.
    let (reboot_pending, muted) = app
        .try_state::<EngineHandle>()
        .map(|h| (h.state.is_reboot_pending(), h.dsp_params.is_muted()))
        .unwrap_or((false, false));
    let tooltip = match (resolved, reboot_pending, muted) {
        (TrayState::Red, _, true) => tooltip_red_and_muted(),
        (TrayState::Red, true, false) => tooltip_red_and_reboot_pending(),
        (TrayState::Yellow, true, _) => tooltip_reboot_pending(),
        _ => tooltip_for(resolved),
    };
    let _ = tray.set_tooltip(Some(tooltip));
}

/// Best-effort reconstruction of the "base" state when we only have the
/// engine handle. Used by [`refresh_tray_for_reboot_pending_change`] — when
/// the only thing that changed is `reboot_pending`, we don't want to
/// downgrade an existing Red. We can't see "Red" directly (it's a transient
/// signal, not persisted), so we infer from `is_running` + `processing_enabled`:
///
///   - engine not running → Yellow (the spec calls this idle/passthrough;
///     a true Red would have been re-emitted by whoever owns the error).
///   - engine running + processing off → Yellow.
///   - engine running + processing on → Green.
///
/// This deliberately does NOT resurrect Red — if the app is in a Red state
/// the next caller-driven `set_tray_state` will re-emit it. The reboot-pending
/// refresh path is only about toggling the Yellow overlay (and its tooltip)
/// on/off.
fn infer_base_state(handle: &EngineHandle) -> TrayState {
    use std::sync::atomic::Ordering;
    let running = handle.engine.lock().map(|e| e.is_running()).unwrap_or(false);
    if !running {
        return TrayState::Yellow;
    }
    if handle.state.processing_enabled.load(Ordering::Relaxed) {
        TrayState::Green
    } else {
        TrayState::Yellow
    }
}

fn load_icon<R: Runtime>(
    app: &AppHandle<R>,
    state: TrayState,
) -> Result<Image<'static>, Box<dyn std::error::Error>> {
    // Filenames are *semantic* (default/warning/error/muted) rather than
    // colour-named — the assets are macOS template images (monochrome,
    // auto-tinted by AppKit), so the old green/yellow/red labels no longer
    // describe what's on disk. The `TrayState` enum still uses colour names
    // because every call site reasons about it that way ("turn it red on
    // error"); the mapping here is the seam between the two vocabularies.
    //
    // We deliberately load the `-2x.png` (44×44) variant rather than the
    // `.png` (22×22). The `tray-icon` crate's macOS backend decodes our
    // bytes into an `NSImage` that keeps the source bitmap's pixel
    // dimensions in its rep, then forcibly calls `nsimage.setSize` to
    // 18pt height (proportional width). When the bitmap is 44×44 and the
    // logical size is ~18pt, AppKit infers a scale factor of ~2.44× and
    // uses the high-res bitmap directly on Retina displays — this is the
    // standard NSImage HiDPI trick, just driven by the upstream crate
    // rather than by us.
    //
    // Feeding it the 22×22 source instead would give a ~1.22× NSImage —
    // effectively non-Retina-aware, blurry on every modern Mac. The 22×22
    // assets ship anyway as design artefacts (the @1x is the
    // pixel-tight master per Klaar's icon spec) but they're not loaded
    // at runtime. On non-Retina Macs the 44×44 source gets downsampled
    // to ~18×18px on screen — high-quality but not pixel-perfect; an
    // acceptable trade in 2026.
    //
    // The 18pt target is hardcoded in `tray-icon` and we can't change it
    // without reaching past the Tauri abstraction (`ns_status_item()`).
    // It's close enough to the bjango-article 22pt working area for the
    // ~16pt glyph guide we designed against to read at the intended
    // weight.
    let filename = match state {
        TrayState::Green => "tray-default-2x.png",
        TrayState::Yellow => "tray-warning-2x.png",
        TrayState::Muted => "tray-muted-2x.png",
        TrayState::Red => "tray-error-2x.png",
    };

    // Production: icons live under Contents/Resources/icons/ (declared in
    // tauri.conf.json `bundle.resources`). Development: fall back to the
    // source tree via CARGO_MANIFEST_DIR.
    //
    // The fallback used to be unconditional and silent — meaning a missing
    // bundled icon would resolve to the developer's local source path,
    // which is baked into the binary at compile time. That hid the fact
    // that tray-*.png weren't being bundled at all: it worked in dev,
    // worked in `tauri build` runs of the same binary on the same Mac,
    // and panicked the setup hook the moment the .app ran on any other
    // machine (ENOENT on a path that only exists on the build host).
    //
    // We now log a WARN whenever the fallback is taken, so the next
    // accidental drop from `bundle.resources` is visible the first time
    // anyone runs the app — even on the dev's own Mac.
    let bundled = app.path().resource_dir().ok().map(|d| d.join("icons").join(filename));

    if let Some(path) = bundled.as_ref().filter(|p| p.exists()) {
        return Ok(Image::from_path(path)?);
    }

    // Dev fallback: locate icons via CARGO_MANIFEST_DIR. Gated to debug builds
    // because `env!` bakes the literal path into the binary (it is NOT
    // affected by `--remap-path-prefix`), which would leak the build-host
    // home directory into shipped strings. Release builds must not need this
    // fallback — bundle.resources is the single source of truth there.
    #[cfg(debug_assertions)]
    {
        let dev_fallback = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join(filename);
        log::warn!(
            "tray icon {filename} not found in bundled resources ({:?}); \
             falling back to dev path {}",
            bundled,
            dev_fallback.display()
        );
        return Ok(Image::from_path(dev_fallback)?);
    }
    #[cfg(not(debug_assertions))]
    {
        Err(format!(
            "tray icon {filename} not found in bundled resources ({:?}) — \
             missing from bundle.resources?",
            bundled
        )
        .into())
    }
}

/// Tray click handler. While reboot-pending, also emits a frontend event so
/// `EnumerationGate` can re-render the `reboot-required` dialog snappily
/// (the persisted-state read is the source of truth, but the event avoids
/// waiting for the next focus-driven re-evaluation).
fn handle_tray_click<R: Runtime>(app: &AppHandle<R>) {
    if let Some(handle) = app.try_state::<EngineHandle>() {
        if handle.state.is_reboot_pending() {
            // Fire-and-forget — the gate already handles the case where the
            // event arrives before the listener is attached (it re-checks
            // on focus / mount).
            let _ = app.emit("klaar://reboot-required-requested", ());
        }
    }
    toggle_window(app);
    // Left-click also flips visibility — keep the menu label in sync.
    refresh_tray_menu_labels(app);
}

/// Right-click menu dispatcher.
///
/// `about` shows the window if hidden, refreshes the show/hide label, then
/// emits `klaar://about-requested` — never hides on About, never
/// toggles. `show-hide` mirrors the left-click toggle (incl. label refresh).
/// `quit` exits unconditionally — same shutdown path as `Cmd+Q` from the
/// focused window. Quit is intentionally not gated on any state (onboarding,
/// errors, reboot-pending) per the `app-shell` spec.
///
/// Id resolution is delegated to the pure [`menu_action_for`] resolver so
/// the id-to-action mapping is exhaustively unit-testable; only the side
/// effects live here.
fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let Some(action) = menu_action_for(event.id().as_ref()) else {
        return;
    };
    match action {
        MenuAction::OpenAbout => {
            // `about-requested` MUST never hide a visible window — the user's
            // ask is "show me the About surface", which would be hostile if
            // it sometimes hid the window. Show + focus when hidden,
            // otherwise leave the window untouched.
            if let Some(window) = app.get_webview_window("main") {
                if !window.is_visible().unwrap_or(false) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            refresh_tray_menu_labels(app);
            // Frontend listens at the App root and flips the in-memory
            // `aboutStore` flag. Fire-and-forget — same idiom as the
            // reboot-required event.
            let _ = app.emit("klaar://about-requested", ());
        }
        MenuAction::ToggleWindow => {
            toggle_window(app);
            refresh_tray_menu_labels(app);
        }
        MenuAction::ToggleMute => {
            // Funnel through the shared chokepoint so the hotkey, the IPC
            // commands, and this menu all hit the same atomic write +
            // `klaar://mute-changed` emission. The event listener wired up
            // in `lib.rs::setup` then drives `refresh_tray_for_mute_change`,
            // which updates the icon, tooltip, AND the mute label — we do
            // NOT call any refresh helper here, to keep the single
            // event-driven path.
            if let Some(handle) = app.try_state::<EngineHandle>() {
                let _ = crate::commands::mute::toggle_mute_shared(app, &handle.dsp_params);
            }
        }
        MenuAction::Quit => {
            app.exit(0);
        }
    }
}

fn toggle_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// Pure resolver for the show/hide label — exhaustively unit-testable.
pub fn show_hide_label(visible: bool) -> &'static str {
    if visible {
        LABEL_HIDE
    } else {
        LABEL_SHOW
    }
}

/// Pure resolver for the mute menu item label.
///
/// Mirrors [`show_hide_label`]: the label text *is* the state indicator.
/// Returns the action-label (i.e. what clicking the item will do):
///   - `false` (currently unmuted) → "Mute Klaar  ⌃⇧M"
///   - `true`  (currently muted)   → "✓ Unmute Klaar  ⌃⇧M"
///
/// The leading `✓` while muted is the spec's mandated indicator.
pub fn mute_label(muted: bool) -> &'static str {
    if muted {
        LABEL_UNMUTE
    } else {
        LABEL_MUTE
    }
}

/// Refresh the mute menu item's text to match the current mute state.
///
/// Item-level `set_text` (no menu rebuild) — same pattern as
/// [`refresh_tray_menu_labels`]. A no-op if the tray menu state, or the
/// `mute` item, can't be found: a label-refresh failure must never break
/// the underlying mute toggle.
pub fn refresh_tray_mute_label<R: Runtime>(app: &AppHandle<R>) {
    let Some(handle) = app.try_state::<EngineHandle>() else {
        return;
    };
    let label = mute_label(handle.dsp_params.is_muted());

    let Some(tray_menu) = app.try_state::<TrayMenu<R>>() else {
        return;
    };
    if let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get(MENU_ID_MUTE) {
        let _ = item.set_text(label);
    }
}

/// Refresh the show/hide menu item to match the current window visibility.
///
/// Must be called from any site that flips visibility (close-requested
/// handler, tray click handler, `setup()` cold launch, `RunEvent::Reopen`).
/// A no-op if the tray, its menu, or the `show-hide` item cannot be found —
/// we never want a label-refresh failure to interrupt the visibility change.
pub fn refresh_tray_menu_labels<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let label = show_hide_label(visible);

    let Some(tray_menu) = app.try_state::<TrayMenu<R>>() else {
        return;
    };
    if let Some(MenuItemKind::MenuItem(item)) = tray_menu.0.get(MENU_ID_SHOW_HIDE) {
        let _ = item.set_text(label);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — pure resolver only (no Tauri runtime needed)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Priority: Red wins over everything, including reboot-pending.
    #[test]
    fn red_wins_over_reboot_pending() {
        let state = AudioEngineState::new();
        state.set_reboot_pending(true);
        assert_eq!(
            resolve_tray_state(&state, TrayState::Red, false),
            TrayState::Red
        );
    }

    /// Reboot-pending promotes Green to Yellow (no separate
    /// Orange variant any more — Yellow doubles as the reboot indicator,
    /// disambiguated by tooltip).
    #[test]
    fn yellow_overrides_green_when_reboot_pending() {
        let state = AudioEngineState::new();
        state.set_reboot_pending(true);
        assert_eq!(
            resolve_tray_state(&state, TrayState::Green, false),
            TrayState::Yellow
        );
    }

    /// Yellow stays Yellow under reboot-pending — the tooltip is what
    /// changes (passthrough → "Reboot required…").
    #[test]
    fn yellow_stays_yellow_when_reboot_pending() {
        let state = AudioEngineState::new();
        state.set_reboot_pending(true);
        assert_eq!(
            resolve_tray_state(&state, TrayState::Yellow, false),
            TrayState::Yellow
        );
    }

    /// Without reboot_pending, the base state passes through unchanged.
    #[test]
    fn base_state_unchanged_when_no_reboot_pending() {
        let state = AudioEngineState::new();
        assert_eq!(
            resolve_tray_state(&state, TrayState::Green, false),
            TrayState::Green
        );
        assert_eq!(
            resolve_tray_state(&state, TrayState::Yellow, false),
            TrayState::Yellow
        );
        assert_eq!(
            resolve_tray_state(&state, TrayState::Red, false),
            TrayState::Red
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // Mute overlay.
    //
    // Priority: Red > Muted > Yellow(reboot-pending) > Green.
    // ─────────────────────────────────────────────────────────────────────

    /// Red still wins the icon when the user is also muted; tooltip
    /// composition (`tooltip_red_and_muted`) carries the second signal.
    #[test]
    fn red_wins_over_muted() {
        let state = AudioEngineState::new();
        assert_eq!(
            resolve_tray_state(&state, TrayState::Red, true),
            TrayState::Red
        );
    }

    /// Muted promotes Green to Muted.
    #[test]
    fn muted_promotes_green() {
        let state = AudioEngineState::new();
        assert_eq!(
            resolve_tray_state(&state, TrayState::Green, true),
            TrayState::Muted
        );
    }

    /// Muted promotes Yellow (passthrough) to Muted — the user's most
    /// recent intent (mute) wins over the older one (passthrough).
    #[test]
    fn muted_promotes_yellow_passthrough() {
        let state = AudioEngineState::new();
        assert_eq!(
            resolve_tray_state(&state, TrayState::Yellow, true),
            TrayState::Muted
        );
    }

    /// Muted wins over reboot-pending (more recent user action).
    /// Reboot-pending is still surfaced in System Settings; the tray icon
    /// reflects what the user just did.
    #[test]
    fn muted_wins_over_reboot_pending() {
        let state = AudioEngineState::new();
        state.set_reboot_pending(true);
        assert_eq!(
            resolve_tray_state(&state, TrayState::Green, true),
            TrayState::Muted
        );
        assert_eq!(
            resolve_tray_state(&state, TrayState::Yellow, true),
            TrayState::Muted
        );
    }

    /// Exhaustive 2×2×2×3 state table over (error, reboot_pending,
    /// processing_enabled-as-base-Green-vs-Yellow, muted). The base param
    /// is the caller-computed value; we sweep Green/Yellow/Red as the three
    /// distinct base inputs the rest of the codebase emits.
    #[test]
    fn state_table_exhaustive() {
        for &base in &[TrayState::Green, TrayState::Yellow, TrayState::Red] {
            for &reboot_pending in &[false, true] {
                for &muted in &[false, true] {
                    let state = AudioEngineState::new();
                    state.set_reboot_pending(reboot_pending);
                    let resolved = resolve_tray_state(&state, base, muted);
                    let expected = match (base, muted, reboot_pending) {
                        // Red always wins the icon.
                        (TrayState::Red, _, _) => TrayState::Red,
                        // Muted next.
                        (_, true, _) => TrayState::Muted,
                        // Reboot-pending promotes Green→Yellow; Yellow stays.
                        (TrayState::Green, false, true) => TrayState::Yellow,
                        (TrayState::Yellow, false, true) => TrayState::Yellow,
                        // Otherwise pass through.
                        (b, false, false) => b,
                        // Muted variant never appears as the *base* —
                        // callers only compute Green/Yellow/Red.
                        (TrayState::Muted, _, _) => unreachable!(
                            "Muted is never a base state — only resolver output"
                        ),
                    };
                    assert_eq!(
                        resolved, expected,
                        "base={base:?} reboot_pending={reboot_pending} muted={muted}"
                    );
                }
            }
        }
    }

    /// The show/hide label resolver returns the "Hide" copy while the
    /// window is visible and "Show" while hidden.
    #[test]
    fn show_hide_label_tracks_visibility() {
        assert_eq!(show_hide_label(true), "Hide Klaar");
        assert_eq!(show_hide_label(false), "Show Klaar");
    }

    /// Pure id-to-action resolver covers every registered menu id
    /// (including `mute`) and rejects unknowns.
    #[test]
    fn menu_action_for_resolves_known_ids() {
        assert_eq!(menu_action_for("about"), Some(MenuAction::OpenAbout));
        assert_eq!(menu_action_for("show-hide"), Some(MenuAction::ToggleWindow));
        assert_eq!(menu_action_for("mute"), Some(MenuAction::ToggleMute));
        assert_eq!(menu_action_for("quit"), Some(MenuAction::Quit));
    }

    #[test]
    fn menu_action_for_returns_none_for_unknown_ids() {
        assert_eq!(menu_action_for("garbage"), None);
        assert_eq!(menu_action_for(""), None);
        // Case-sensitivity guard — the menu builder uses lowercase ids
        // verbatim.
        assert_eq!(menu_action_for("About"), None);
    }

    /// `MenuAction` derives the standard trait set so it slots into the
    /// existing test style.
    #[test]
    fn menu_action_implements_expected_trait_set() {
        // `Copy` lets us pass actions by value without explicit `.clone()`
        // sprinkles in tests.
        fn assert_copy<T: Copy>() {}
        fn assert_clone<T: Clone>() {}
        fn assert_eq_<T: Eq>() {}
        fn assert_partial_eq<T: PartialEq>() {}
        fn assert_debug<T: std::fmt::Debug>() {}
        assert_copy::<MenuAction>();
        assert_clone::<MenuAction>();
        assert_eq_::<MenuAction>();
        assert_partial_eq::<MenuAction>();
        assert_debug::<MenuAction>();

        // Round-trip equality / Debug at runtime so the trait impls are
        // actually exercised, not just bounded.
        let a = MenuAction::OpenAbout;
        let b = a;
        assert_eq!(a, b);
        assert_eq!(format!("{a:?}"), "OpenAbout");
    }

    /// Tooltip lookup covers every variant (compile-time exhaustive via
    /// the match in `tooltip_for`).
    #[test]
    fn tooltip_for_each_state_is_non_empty() {
        for s in [
            TrayState::Green,
            TrayState::Yellow,
            TrayState::Muted,
            TrayState::Red,
        ] {
            assert!(!tooltip_for(s).is_empty(), "tooltip empty for {:?}", s);
        }
        // Reboot-pending tooltip stands on its own and mentions reboot.
        assert!(tooltip_reboot_pending().to_lowercase().contains("reboot"));
        // Red + reboot-pending composite mentions both signals.
        let combined = tooltip_red_and_reboot_pending();
        assert!(combined.contains("error"));
        assert!(combined.contains("Reboot"));
    }

    /// Lock the exact mute tooltip strings here so a wording drift fails
    /// CI rather than reaching users.
    #[test]
    fn mute_tooltips_match_spec() {
        assert_eq!(tooltip_for(TrayState::Muted), "Klaar — muted (⌃⇧M to unmute)");
        assert_eq!(
            tooltip_red_and_muted(),
            "Klaar — error\nMuted (⌃⇧M to unmute)"
        );
    }

    /// The mute menu label resolver returns the action label (i.e. what
    /// clicking will *do*), with the leading `✓` glyph appearing only while
    /// currently muted. Lock the exact strings and the accelerator hint so
    /// any wording drift breaks CI.
    #[test]
    fn mute_label_tracks_state() {
        // Unmuted → action = "mute"; no checkmark.
        assert_eq!(mute_label(false), "Mute Klaar  ⌃⇧M");
        assert!(!mute_label(false).starts_with('✓'));

        // Muted → action = "unmute"; leading checkmark glyph.
        assert_eq!(mute_label(true), "✓ Unmute Klaar  ⌃⇧M");
        assert!(mute_label(true).starts_with('✓'));

        // Accelerator hint present in both branches so the user can
        // discover the chord regardless of current state.
        assert!(mute_label(false).contains("⌃⇧M"));
        assert!(mute_label(true).contains("⌃⇧M"));
    }

    /// The resolver still rejects unknown ids after the `mute` id was
    /// added (regression guard for `menu_action_for_returns_none_for_unknown_ids`).
    #[test]
    fn menu_action_for_rejects_unknown_after_mute_added() {
        assert_eq!(menu_action_for("Mute"), None); // case-sensitive
        assert_eq!(menu_action_for("toggle-mute"), None);
        assert_eq!(menu_action_for("mute "), None); // trailing whitespace
    }
}
