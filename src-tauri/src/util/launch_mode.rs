//! Launch-mode detection — distinguishes autostart-at-login from a
//! user-initiated launch (Finder, `open -a`, Raycast, `cargo tauri dev`).
//!
//! When the macOS LaunchAgent fires Klaar at login, `tauri-plugin-autostart`
//! writes the `--launched-at-login` argument into the LaunchAgent plist's
//! `ProgramArguments`. Detecting that flag lets `setup()` skip the
//! cold-launch `window.show()` so the app stays quietly in the menu bar
//! after a fresh boot.
//!
//! The flag is an internal marker — it is documented as such in code only
//! and is not part of any user-facing CLI surface.

const LAUNCHED_AT_LOGIN_FLAG: &str = "--launched-at-login";

/// Pure inner: scans an arbitrary iterator of args for the marker flag.
pub fn launched_at_login_from<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == LAUNCHED_AT_LOGIN_FLAG)
}

/// Inverse-form predicate that mirrors the cold-launch branching in
/// `setup()`: returns `true` when the cold-launch path SHALL surface the
/// configuration window. Pulled out as a separate helper so the cold-launch
/// gate is unit-testable without spinning up a full Tauri runtime — the
/// integration check this stands in for is "if `--launched-at-login` is in
/// argv, the window must NOT be shown automatically".
pub fn should_show_on_cold_launch<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    !launched_at_login_from(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_absent_returns_false() {
        let argv = vec_strs(&["/path/to/Klaar"]);
        assert!(!launched_at_login_from(argv));
    }

    #[test]
    fn flag_present_returns_true() {
        let argv = vec_strs(&["/path/to/Klaar", "--launched-at-login"]);
        assert!(launched_at_login_from(argv));
    }

    #[test]
    fn flag_mid_list_returns_true() {
        let argv = vec_strs(&[
            "/path/to/Klaar",
            "--something-else",
            "--launched-at-login",
            "--trailing",
        ]);
        assert!(launched_at_login_from(argv));
    }

    #[test]
    fn flag_duplicated_returns_true() {
        let argv = vec_strs(&[
            "/path/to/Klaar",
            "--launched-at-login",
            "--launched-at-login",
        ]);
        assert!(launched_at_login_from(argv));
    }

    #[test]
    fn empty_argv_returns_false() {
        let argv: Vec<String> = Vec::new();
        assert!(!launched_at_login_from(argv));
    }

    #[test]
    fn should_show_on_cold_launch_inverts_flag_presence() {
        // No flag → cold launch surfaces the window.
        assert!(should_show_on_cold_launch(vec_strs(&["/path/to/Klaar"])));
        // Flag present → cold launch keeps the window hidden (autostart).
        assert!(!should_show_on_cold_launch(vec_strs(&[
            "/path/to/Klaar",
            "--launched-at-login",
        ])));
        // Empty argv (e.g. `cargo test` synthetic invocation) → show.
        let empty: Vec<String> = Vec::new();
        assert!(should_show_on_cold_launch(empty));
    }

    #[test]
    fn similar_but_different_flag_returns_false() {
        let argv = vec_strs(&[
            "/path/to/Klaar",
            "--launched-at-login=true", // matched verbatim, no `=` parsing
            "launched-at-login",
            "--launched_at_login",
        ]);
        assert!(!launched_at_login_from(argv));
    }
}
