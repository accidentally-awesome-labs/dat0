use dat0_core::error_ux::{Banner, BannerKind, Modal, Toast, ToastSeverity};

#[test]
fn modal_builder_round_trip() {
    let m = Modal::new("Title", "Body").with_primary("OK", || {});
    assert_eq!(m.title, "Title");
    assert_eq!(m.message, "Body");
    assert!(m.primary_action.is_some());
    assert_eq!(m.primary_action.as_ref().unwrap().0, "OK");
}

#[test]
fn toast_info_default_dismiss() {
    let t = Toast::info("hello");
    assert!(matches!(t.severity, ToastSeverity::Info));
    assert!(t.auto_dismiss_after.is_some());
}

#[test]
fn toast_error_no_auto_dismiss() {
    let t = Toast::error("oh no");
    assert!(matches!(t.severity, ToastSeverity::Error));
    assert!(t.auto_dismiss_after.is_none());
}

#[test]
fn banner_warning_dismissible() {
    let b = Banner::warning("careful");
    assert!(matches!(b.kind, BannerKind::Warning));
    assert!(b.dismissible);
}
