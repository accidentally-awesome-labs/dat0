//! Banner (persistent inline notice) state primitive.

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Info/Error reserved for future call sites; full API surface.
pub enum BannerSeverity {
    Info,
    Warning,
    Error,
}

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
