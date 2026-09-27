//! Banner (persistent inline notice) — structured shape (P3b T2).
//!
//! Previous P3a shape (`message: String + action_label: Option<String>`)
//! is replaced with a typed two-action shape so the recovery flow
//! (T5), import wizard (T9), and fetch-failed UX (T8) can wire
//! discoverable buttons. The boot-time push / drain primitive is
//! preserved.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BannerKind {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BannerLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BannerAction {
    pub label: String,
    /// Stable action id; resolved by `ActionRegistry` (T3).
    pub action_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Banner {
    pub title: String,
    pub body: String,
    pub link: Option<BannerLink>,
    pub primary: Option<BannerAction>,
    pub secondary: Option<BannerAction>,
    pub kind: BannerKind,
    pub dismissible: bool,
}

impl Banner {
    pub fn info(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: String::new(),
            link: None,
            primary: None,
            secondary: None,
            kind: BannerKind::Info,
            dismissible: true,
        }
    }

    pub fn warning(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: String::new(),
            link: None,
            primary: None,
            secondary: None,
            kind: BannerKind::Warning,
            dismissible: true,
        }
    }

    pub fn warning_with_body(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            link: None,
            primary: None,
            secondary: None,
            kind: BannerKind::Warning,
            dismissible: true,
        }
    }

    pub fn error(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            link: None,
            primary: None,
            secondary: None,
            kind: BannerKind::Error,
            dismissible: true,
        }
    }

    pub fn with_primary(mut self, label: impl Into<String>, action_id: impl Into<String>) -> Self {
        self.primary = Some(BannerAction {
            label: label.into(),
            action_id: action_id.into(),
        });
        self
    }

    pub fn with_secondary(
        mut self,
        label: impl Into<String>,
        action_id: impl Into<String>,
    ) -> Self {
        self.secondary = Some(BannerAction {
            label: label.into(),
            action_id: action_id.into(),
        });
        self
    }

    pub fn with_link(mut self, label: impl Into<String>, url: impl Into<String>) -> Self {
        self.link = Some(BannerLink {
            label: label.into(),
            url: url.into(),
        });
        self
    }
}

/// Process-global queue for banners raised with no window in hand: at boot,
/// before any window exists, and from core code that has no handle on the
/// window it is working for. A window drains it on every [`push`] — see
/// [`subscribe`]. Code that does have a window pushes to that window directly,
/// so its banner cannot surface in another one.
static PENDING: Lazy<Mutex<Vec<Banner>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Bumped on every [`push`].
///
/// The queue alone is not enough: a window that only drains when something
/// else re-renders it finds a banner late or never. The Dioxus shell did
/// exactly that — its drain lived in a `use_effect` that read no signal, so it
/// ran once per mount and every banner raised after the first frame stayed in
/// the queue until some *other* window mounted and showed it there.
static GENERATION: Lazy<tokio::sync::watch::Sender<u64>> =
    Lazy::new(|| tokio::sync::watch::channel(0).0);

pub fn push(banner: Banner) {
    match PENDING.lock() {
        Ok(mut q) => q.push(banner),
        Err(poisoned) => {
            tracing::warn!("error_ux::banner pending queue mutex poisoned; recovering");
            let mut q = poisoned.into_inner();
            q.push(banner);
        }
    }
    // After the lock is released, so a woken window can take it at once.
    GENERATION.send_modify(|g| *g = g.wrapping_add(1));
}

/// A receiver that changes whenever [`push`] queues a banner.
///
/// A window awaits `changed()` on it and calls [`drain_pending`] on each wake.
/// Subscribe *before* the first drain: a push that lands between the two then
/// wakes the loop once more, rather than slipping past both.
pub fn subscribe() -> tokio::sync::watch::Receiver<u64> {
    GENERATION.subscribe()
}

/// Convenience for migrating call sites that just want a warning with a
/// single-line title.
pub fn push_warning(title: impl Into<String>) {
    push(Banner::warning(title));
}

pub fn drain_pending() -> Vec<Banner> {
    match PENDING.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(poisoned) => {
            tracing::warn!("error_ux::banner pending queue mutex poisoned; recovering");
            let mut q = poisoned.into_inner();
            std::mem::take(&mut *q)
        }
    }
}

/// Move any globally-stashed banners into a per-window live list (PD-021).
///
/// Call it from a loop driven by [`subscribe`], not from a render or an
/// effect: neither re-runs because the queue changed.
pub fn merge_pending(live: &mut Vec<Banner>) {
    live.append(&mut drain_pending());
}
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // These tests mutate the process-global PENDING queue. `#[serial]` joins the
    // crate-wide serial group (shared with the file_drop/session banner tests) so
    // a concurrent test's `drain_pending()` can't steal banners mid-test — the CI
    // linux flake on 2026-06-07 where `push_drain_round_trip` saw 1 banner, not 2.
    #[test]
    #[serial]
    fn push_drain_round_trip() {
        let _ = drain_pending();
        push(Banner::warning("first"));
        push(Banner::warning("second"));
        let drained = drain_pending();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].title, "first");
        assert_eq!(drained[1].title, "second");
        assert!(drain_pending().is_empty());
    }

    #[test]
    #[serial]
    fn a_push_wakes_every_subscriber() {
        let _ = drain_pending();
        let mut first = subscribe();
        let second = subscribe();
        assert!(!first.has_changed().unwrap(), "nothing pushed yet");

        push(Banner::warning("raised after the window mounted"));

        // Both windows must be told; which of them drains is a race they
        // settle between themselves, but neither may sleep through it.
        assert!(
            first.has_changed().unwrap(),
            "a push must wake a subscriber"
        );
        assert!(second.has_changed().unwrap(), "and every subscriber");
        first.borrow_and_update();
        assert!(
            !first.has_changed().unwrap(),
            "seen once, quiet until the next push"
        );
        let _ = drain_pending();
    }

    #[test]
    #[serial]
    fn merge_pending_moves_global_into_live_vec() {
        // clear any cross-test residue
        let _ = drain_pending();
        push(Banner::error("export failed", "disk full"));
        push(Banner::info("done"));
        let mut live: Vec<Banner> = Vec::new();
        merge_pending(&mut live);
        assert_eq!(live.len(), 2, "both pending banners moved into live vec");
        assert!(drain_pending().is_empty(), "PENDING drained after merge");
    }
}
