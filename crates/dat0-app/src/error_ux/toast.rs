//! Toast (transient notification) state primitive.
//!
//! Errors do not auto-dismiss; informational/success/warning toasts dismiss
//! after a short delay by default.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Success/Warning reserved for future call sites; full API surface.
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Toast {
    pub message: String,
    pub severity: ToastSeverity,
    pub auto_dismiss_after: Option<Duration>,
}

impl Toast {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Info,
            auto_dismiss_after: Some(Duration::from_secs(4)),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Error,
            auto_dismiss_after: None,
        }
    }
}
