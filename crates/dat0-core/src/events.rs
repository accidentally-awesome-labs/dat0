//! The application event bus.
//!
//! Every cross-thread and cross-window signal travels through here as **data**
//! — no closures, no renderer handles — so a producer can live on any thread
//! and the consumer can be any UI.
//!
//! This replaces `dat0-app`'s `main_bridge::MainClosure`
//! (`Box<dyn FnOnce(&mut gpui::App) + Send>`), which forced every background
//! task that wanted to touch the UI to name a toolkit type. Producers today are
//! the UDS single-instance server, the settings watcher, the update checker,
//! the recovery scan, the action registry, and the native menu bar.
//!
//! ## Why actions are an id, not a closure
//!
//! [`AppEvent::RunAction`] carries a stable action id rather than a callback.
//! Almost every action in dat0 means "do X to the focused window", and the
//! focused window is something only the UI knows. Keeping the id opaque to
//! `dat0-core` is what lets the registry, the command palette and the menu bar
//! all be renderer-free: they name the action, the shell performs it.

use std::path::PathBuf;

use uuid::Uuid;

use crate::error_ux::Banner;
use crate::update::manifest::UpdateManifest;

/// A signal from anywhere in the process to the UI.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Open a new window, optionally opening these paths in it. Sent by the
    /// UDS single-instance handler when a second launch is coalesced into the
    /// running process, and by the `window.new` action.
    OpenWindow { paths: Vec<PathBuf> },
    /// Open paths in an existing window.
    OpenPaths { window: Uuid, paths: Vec<PathBuf> },
    /// `settings.toml` changed on disk (the settings watcher).
    SettingsChanged,
    /// Switch to the named theme.
    ThemeChanged { id: String },
    /// The updater found a newer release.
    UpdateAvailable(UpdateManifest),
    /// The boot-time scan for interrupted promotions finished. The payload is
    /// the workspace roots that look incomplete.
    RecoveryScanFinished(Vec<PathBuf>),
    /// Perform a registered action. `window` names the target window when the
    /// sender knows it; `None` means "the focused one".
    RunAction {
        id: &'static str,
        window: Option<Uuid>,
    },
    /// Surface a banner.
    Banner(Banner),
}

/// The send half of the bus. Cheap to clone; hand one to every producer.
#[derive(Clone)]
pub struct AppEvents(futures::channel::mpsc::UnboundedSender<AppEvent>);

/// The receive half. The shell drains this in a single task.
pub type AppEventRx = futures::channel::mpsc::UnboundedReceiver<AppEvent>;

impl AppEvents {
    /// Create a connected sender/receiver pair.
    pub fn channel() -> (Self, AppEventRx) {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        (Self(tx), rx)
    }

    /// Post an event.
    ///
    /// Failure means the receiver is gone, which happens only during shutdown;
    /// it is logged and dropped rather than propagated, because no producer has
    /// anything useful to do about a UI that has already closed.
    pub fn send(&self, ev: AppEvent) {
        if let Err(e) = self.0.unbounded_send(ev) {
            tracing::debug!("AppEvent dropped, receiver gone: {e}");
        }
    }
}

impl std::fmt::Debug for AppEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppEvents")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;

    #[test]
    fn events_arrive_in_order() {
        let (tx, mut rx) = AppEvents::channel();
        tx.send(AppEvent::SettingsChanged);
        tx.send(AppEvent::ThemeChanged { id: "light".into() });

        assert!(matches!(rx.try_recv(), Ok(AppEvent::SettingsChanged)));
        match rx.try_recv() {
            Ok(AppEvent::ThemeChanged { id }) => assert_eq!(id, "light"),
            other => panic!("unexpected: {other:?}"),
        }
        // Nothing queued, but the channel is still open.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn send_after_receiver_drop_is_silent() {
        let (tx, rx) = AppEvents::channel();
        drop(rx);
        // Must not panic: producers outlive the UI during shutdown.
        tx.send(AppEvent::SettingsChanged);
    }

    #[tokio::test]
    async fn a_clone_feeds_the_same_receiver() {
        let (tx, mut rx) = AppEvents::channel();
        let tx2 = tx.clone();
        std::thread::spawn(move || tx2.send(AppEvent::SettingsChanged))
            .join()
            .unwrap();
        assert!(matches!(rx.next().await, Some(AppEvent::SettingsChanged)));
    }
}
