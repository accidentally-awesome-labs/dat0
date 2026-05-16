//! Banner (persistent inline notice) state primitive.
//!
//! Banner is a pure-data type. Boot-time call sites (P2 T14) need a way to
//! stash banners produced before any window exists; the render layer drains
//! the queue when it wires up notification surfaces. The minimal in-process
//! registry below provides `push` / `drain` for that flow without dictating
//! a render strategy.

use std::sync::Mutex;

use once_cell::sync::Lazy;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Info/Error reserved for future call sites; full API surface.
pub enum BannerSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Banner {
    pub message: String,
    pub severity: BannerSeverity,
    pub dismissible: bool,
    /// Reserved for future use (action button label). Unread today.
    #[allow(dead_code)]
    pub action_label: Option<String>,
}

impl Banner {
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: BannerSeverity::Warning,
            dismissible: true,
            action_label: None,
        }
    }
}

/// Convenience: push a warning-severity banner with a plain string message.
pub fn push_warning_message(msg: impl Into<String>) {
    push(Banner::warning(msg));
}

static PENDING: Lazy<Mutex<Vec<Banner>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Stash a banner produced before any window exists. Render code drains
/// via [`drain_pending`] when notification surfaces come online.
pub fn push(banner: Banner) {
    match PENDING.lock() {
        Ok(mut q) => q.push(banner),
        Err(poisoned) => {
            // Recover from a poisoned mutex rather than panicking — banner
            // delivery is best-effort and a panic here would mask the
            // underlying error the caller was trying to surface.
            tracing::warn!("error_ux::banner pending queue mutex poisoned; recovering");
            let mut q = poisoned.into_inner();
            q.push(banner);
        }
    }
}

/// Drain all pending banners. Returns them in push order; the queue is
/// empty after this call.
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

    /// `push` then `drain_pending` returns banners in FIFO order; subsequent
    /// drain returns empty. The queue is process-global so this test owns
    /// the queue for its duration — drain at the start to clear any state.
    #[test]
    fn push_drain_round_trip() {
        let _ = drain_pending(); // clear any queued banners from prior tests
        push(Banner::warning("first"));
        push(Banner::warning("second"));

        let drained = drain_pending();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].message, "first");
        assert_eq!(drained[1].message, "second");

        let empty = drain_pending();
        assert!(empty.is_empty());
    }
}
