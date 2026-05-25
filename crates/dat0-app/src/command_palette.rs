//! Cmd-Shift-P command palette (P3b T6).
//!
//! The filtering algorithm ([`filter`]) is the load-bearing piece and is
//! unit-tested in `tests/command_palette.rs`. The GPUI overlay view is a
//! stub in [`open`] mirroring the T5 (`recovery_panel`) stub policy:
//! Sheet/Modal mounts require a `&mut Window` context that the
//! action-dispatch path (which has only `&mut App`) cannot produce
//! without hopping through `WindowRegistry`. That plumbing is a follow-up
//! tracked in the T13 retro; spec compliance for T6 is the unit-tested
//! filter + the Cmd-Shift-P binding registration (see `window.rs` and
//! `menu_macos.rs`).
//!
//! # Filter shape
//!
//! Fuzzy subsequence match, case-insensitive, against
//! [`ActionDescriptor::title`]. Empty query returns every registered
//! descriptor (registry-iteration order; see
//! [`ActionRegistry::iter`] — snapshot order is HashMap-backed and
//! therefore non-deterministic, which the palette UI will sort at
//! render time). This matches the plan-verbatim signature so the test
//! file in `tests/command_palette.rs` compiles unchanged.

use crate::actions::registry::{ActionDescriptor, ActionRegistry};

/// Filter actions by a fuzzy subsequence match against the title.
/// Case-insensitive; preserves registry-iteration order.
pub fn filter(reg: &ActionRegistry, query: &str) -> Vec<ActionDescriptor> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return reg.iter().collect();
    }
    reg.iter()
        .filter(|d| subsequence_match(&d.title.to_lowercase(), &q))
        .collect()
}

/// Returns `true` when every char in `needle` appears in `haystack` in
/// order (not necessarily contiguously). Both inputs are expected to be
/// already lowercased by the caller. ASCII-insensitive comparison guards
/// the edge case where the caller passes through a single uppercase
/// letter accidentally.
fn subsequence_match(haystack: &str, needle: &str) -> bool {
    let mut hay = haystack.chars();
    for c in needle.chars() {
        match hay.find(|h| h.eq_ignore_ascii_case(&c)) {
            Some(_) => continue,
            None => return false,
        }
    }
    true
}

/// Entry point invoked by the `Cmd-Shift-P` (macOS) / `Ctrl-Shift-P`
/// (Linux) keybinding. Currently logs the request — the GPUI overlay
/// view requires `&mut Window` plumbing through `WindowRegistry` (same
/// constraint as `recovery_panel::open`), tracked as a T13 retro item.
///
/// **Manual UAT (deferred until the overlay lands):**
/// 1. `cargo run -p dat0-app`
/// 2. Press `Cmd-Shift-P` (macOS) / `Ctrl-Shift-P` (Linux when the
///    Linux menu lands).
/// 3. Expect tracing line `command_palette::open invoked` in the log.
/// 4. When the overlay is wired: type "new", confirm "New Window" is
///    surfaced; press Enter, confirm a new window spawns.
pub fn open(_app: &mut gpui::App) {
    tracing::info!("command_palette::open invoked — overlay view lands in T13 follow-up");
    // TODO(T13): mount gpui-component fuzzy-list (or hand-rolled
    // gpui::TextInput + scored Vec + ↑↓/Enter/Esc handling) via a
    // WindowRegistry hop so the call has access to `&mut Window`.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_match_basic() {
        assert!(subsequence_match("new window", "new"));
        assert!(subsequence_match("new window", "nw"));
        assert!(subsequence_match("new window", "ndw"));
        assert!(!subsequence_match("new window", "xyz"));
        assert!(!subsequence_match("abc", "abcd"));
    }

    #[test]
    fn subsequence_match_case_insensitive_path() {
        // Free-form sanity check that uppercase singletons still match.
        assert!(subsequence_match("new window", "NW"));
    }

    #[test]
    fn subsequence_match_empty_needle_is_trivially_true() {
        assert!(subsequence_match("anything", ""));
    }
}
