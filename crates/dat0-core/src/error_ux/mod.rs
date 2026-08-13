//! Error & dialog UX primitives — pure-data state structs for Modal / Toast / Banner.
//!
//! These primitives ship as type-only state in P1 (T17). Real GPUI rendering will
//! be wired up at use sites (e.g., `WorkspaceInUseModal` in P7) using
//! gpui-component's first-party `Dialog` / `Sheet` / notification API surface.

pub mod banner;
pub mod engine;
pub mod modal;
pub mod toast;

pub use banner::{Banner, BannerAction, BannerKind, BannerLink, drain_pending, push, push_warning};
pub use engine::{ENGINE_ERROR_KEYS, banner_for};
pub use modal::Modal;
pub use toast::{Toast, ToastSeverity};
