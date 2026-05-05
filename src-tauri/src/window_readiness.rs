//! Tracks first-paint readiness of the main webview, scoped to a
//! monotonically-increasing generation counter.
//!
//! ## Why a generation counter
//!
//! Every call to `show_main_window` arms a 5 s timer that fires if the
//! frontend has not invoked `frontend_ready` in that window. The frontend's
//! `frontend_ready` and the timer's expiry race: in the worst case
//! `frontend_ready` arrives a millisecond after the timer fires.
//!
//! The generation counter resolves the race deterministically. Each show:
//!
//! 1. `bump_generation()` advances `generation` to a fresh value `G`.
//!    Any in-flight ready signal (for an older `G`) is invalidated by the
//!    bump because `is_ready_for(G)` will not match.
//! 2. The spawned timer captures `G`.
//! 3. On `frontend_ready`, the IPC handler calls `mark_ready_for(G_live)`
//!    using the *current* generation — only marking ready when the bump
//!    matches what we armed against.
//! 4. When the timer fires, it checks `is_ready_for(G)`. If true, the
//!    frontend won the race and the timer no-ops. If false, the timer
//!    declares the webview wedged and triggers recovery.
//!
//! The counter is a `u64` — overflow would require ~5×10^11 shows, which
//! is comfortably never.

use std::sync::atomic::{AtomicU64, Ordering};

/// Zero is the sentinel "no generation has reported ready yet".
/// `bump_generation` always returns a value ≥ 1.
const NO_GENERATION: u64 = 0;

#[derive(Debug)]
pub struct WindowReadiness {
    /// The generation that the most recent `show_main_window` armed against.
    /// Read by the timer to detect supersession; written on every show.
    generation: AtomicU64,
    /// The most recent generation for which the frontend reported ready.
    /// `0` (NO_GENERATION) means nothing has reported ready yet.
    ready_for_generation: AtomicU64,
}

impl WindowReadiness {
    pub fn new() -> Self {
        Self {
            generation: AtomicU64::new(NO_GENERATION),
            ready_for_generation: AtomicU64::new(NO_GENERATION),
        }
    }

    /// Advance the generation counter and return the fresh value.
    ///
    /// Implicitly invalidates any prior `ready_for_generation` because the
    /// reader compares against the live `generation`.
    pub fn bump_generation(&self) -> u64 {
        // `fetch_add` returns the previous value; we want the post-increment.
        let prev = self.generation.fetch_add(1, Ordering::AcqRel);
        prev + 1
    }

    /// The currently-armed generation — i.e. what the most recent show
    /// expects the frontend to ack.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Mark a specific generation as ready. Calls with a stale generation
    /// (i.e. one that no longer matches the live generation) are silently
    /// ignored — they belong to a superseded show cycle.
    ///
    /// Idempotent: a second call in the same generation is a no-op.
    pub fn mark_ready_for(&self, g: u64) {
        if g == NO_GENERATION {
            return;
        }
        let current = self.generation.load(Ordering::Acquire);
        if g != current {
            // Stale ack — its show was superseded.
            return;
        }
        // Use Release so a subsequent Acquire load on
        // `ready_for_generation` synchronises with this write.
        self.ready_for_generation.store(g, Ordering::Release);
    }

    /// Returns `true` when the frontend has confirmed readiness for
    /// generation `g` (and `g` is still the live generation — a stale
    /// generation cannot satisfy this predicate even if it was once marked).
    ///
    /// Currently only consumed by tests — no production code reads
    /// readiness state. Retained because (a) it's the inverse of
    /// `mark_ready_for` and the natural surface for any future consumer
    /// that wants to gate work on the frontend being responsive, and (b)
    /// it lets the regression tests below assert the focus-listener
    /// contract directly.
    #[allow(dead_code)]
    pub fn is_ready_for(&self, g: u64) -> bool {
        if g == NO_GENERATION {
            return false;
        }
        let live = self.generation.load(Ordering::Acquire);
        if g != live {
            return false;
        }
        self.ready_for_generation.load(Ordering::Acquire) == g
    }
}

impl Default for WindowReadiness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_returns_increasing_values_starting_at_one() {
        let r = WindowReadiness::new();
        assert_eq!(r.generation(), 0);
        let g1 = r.bump_generation();
        assert_eq!(g1, 1);
        let g2 = r.bump_generation();
        assert_eq!(g2, 2);
        assert_eq!(r.generation(), 2);
    }

    #[test]
    fn mark_ready_for_matching_generation_marks_ready() {
        let r = WindowReadiness::new();
        let g = r.bump_generation();
        assert!(!r.is_ready_for(g));
        r.mark_ready_for(g);
        assert!(r.is_ready_for(g));
    }

    #[test]
    fn mark_ready_for_stale_generation_is_ignored() {
        let r = WindowReadiness::new();
        let g_stale = r.bump_generation();
        let g_live = r.bump_generation();
        // Trying to mark the stale generation: ignored.
        r.mark_ready_for(g_stale);
        assert!(!r.is_ready_for(g_stale));
        // And the live generation is also still un-ready.
        assert!(!r.is_ready_for(g_live));
    }

    #[test]
    fn bump_invalidates_prior_ready_signal() {
        let r = WindowReadiness::new();
        let g1 = r.bump_generation();
        r.mark_ready_for(g1);
        assert!(r.is_ready_for(g1));
        // A new show bumps; the old ready value is now stale.
        let g2 = r.bump_generation();
        assert!(!r.is_ready_for(g1));
        assert!(!r.is_ready_for(g2));
    }

    #[test]
    fn idempotent_mark_ready() {
        let r = WindowReadiness::new();
        let g = r.bump_generation();
        r.mark_ready_for(g);
        r.mark_ready_for(g);
        r.mark_ready_for(g);
        assert!(r.is_ready_for(g));
    }

    #[test]
    fn zero_generation_is_never_ready() {
        let r = WindowReadiness::new();
        // Never bumped.
        assert!(!r.is_ready_for(0));
        r.mark_ready_for(0); // no-op
        assert!(!r.is_ready_for(0));
    }

    /// Regression test for the warm-system autostart-then-tray-click bug
    /// fixed by the focus-listener re-ack (commit `1c08389`).
    ///
    /// Sequence:
    ///   1. Cold launch arms generation 1.
    ///   2. Frontend mounts and `frontend_ready` acks gen 1 (mount-style).
    ///   3. User hides the window. Tray click later bumps to generation 2
    ///      WITHOUT remounting the React tree — so `Root`'s mount-time
    ///      `frontend_ready` will not fire again.
    ///   4. Without the focus listener, gen 2 stays un-acked indefinitely.
    ///   5. With the focus listener, the next `set_focus` triggers an
    ///      ack for the live generation, which lands cleanly.
    ///
    /// This test locks in step (4) → step (5): a bump after a successful
    /// mount-style ack must leave the new generation un-acked, and a
    /// subsequent focus-style ack against the live generation must lift it.
    #[test]
    fn focus_listener_re_ack_covers_bump_without_remount() {
        let r = WindowReadiness::new();

        // Step 1 + 2: cold-launch ack (mount-style).
        let g1 = r.bump_generation();
        r.mark_ready_for(g1);
        assert!(r.is_ready_for(g1), "mount-time ack should land for gen 1");

        // Step 3: tray click bumps the generation without a remount; no
        // mount-time `frontend_ready` fires for gen 2.
        let g2 = r.bump_generation();
        assert_ne!(g1, g2, "bump must produce a fresh generation");

        // Step 4: gen 2 is un-acked. The previous gen-1 ack does NOT
        // satisfy gen 2 — `is_ready_for` keys on the live generation.
        assert!(
            !r.is_ready_for(g2),
            "post-bump gen must not inherit the prior generation's ack"
        );
        assert!(
            !r.is_ready_for(g1),
            "stale gen must not be reported ready once superseded"
        );

        // Step 5: focus listener fires `frontend_ready` for the live gen.
        let live = r.generation();
        assert_eq!(live, g2, "focus listener acks the live generation");
        r.mark_ready_for(live);

        assert!(
            r.is_ready_for(g2),
            "focus-style ack must lift the un-acked post-bump generation"
        );
    }
}
