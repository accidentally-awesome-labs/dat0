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

/// Boot-time stash for banners produced before any window exists.
static PENDING: Lazy<Mutex<Vec<Banner>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn push(banner: Banner) {
    match PENDING.lock() {
        Ok(mut q) => q.push(banner),
        Err(poisoned) => {
            tracing::warn!("error_ux::banner pending queue mutex poisoned; recovering");
            let mut q = poisoned.into_inner();
            q.push(banner);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
}
