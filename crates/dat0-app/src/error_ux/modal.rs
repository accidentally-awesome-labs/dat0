//! Modal dialog state primitive.
//!
//! Holds title/body and optional primary/secondary actions. The `Render` impl
//! is intentionally minimal at P1 — actual dialog presentation will be wired
//! up through gpui-component's `Dialog` / `Sheet` API at use sites (P7+).

use gpui::{IntoElement, div, prelude::*};

/// Boxed action callback. Stored alongside its label for the button.
pub type ModalAction = (String, std::sync::Arc<dyn Fn() + Send + Sync>);

pub struct Modal {
    pub title: String,
    pub message: String,
    pub primary_action: Option<ModalAction>,
    /// Reserved for future use (P7+: confirm/cancel dialogs). Unread today.
    #[allow(dead_code)]
    pub secondary_action: Option<ModalAction>,
}

impl Modal {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            primary_action: None,
            secondary_action: None,
        }
    }

    pub fn with_primary<F: Fn() + Send + Sync + 'static>(
        mut self,
        label: impl Into<String>,
        f: F,
    ) -> Self {
        self.primary_action = Some((label.into(), std::sync::Arc::new(f)));
        self
    }

    /// Reserved for future use (P7+ workspace-in-use confirm/cancel). Unread today.
    #[allow(dead_code)]
    pub fn with_secondary<F: Fn() + Send + Sync + 'static>(
        mut self,
        label: impl Into<String>,
        f: F,
    ) -> Self {
        self.secondary_action = Some((label.into(), std::sync::Arc::new(f)));
        self
    }
}

impl Render for Modal {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        div().child(self.title.clone()).child(self.message.clone())
    }
}
