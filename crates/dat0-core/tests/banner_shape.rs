use dat0_core::error_ux::banner::{Banner, BannerAction, BannerKind, BannerLink};

#[test]
fn construct_full_shape() {
    let b = Banner {
        title: "Recovered previous sessions".into(),
        body: "3 previous sessions found.".into(),
        link: Some(BannerLink {
            label: "Learn more".into(),
            url: "https://example.com".into(),
        }),
        primary: Some(BannerAction {
            label: "Review".into(),
            action_id: "REVIEW_RECOVERY".into(),
        }),
        secondary: Some(BannerAction {
            label: "Dismiss".into(),
            action_id: "DISMISS".into(),
        }),
        kind: BannerKind::Warning,
        dismissible: true,
    };
    assert_eq!(b.title, "Recovered previous sessions");
    assert_eq!(b.primary.as_ref().unwrap().label, "Review");
    assert_eq!(b.secondary.as_ref().unwrap().label, "Dismiss");
}

#[test]
fn construct_minimal_no_actions() {
    let b = Banner::info("hello");
    assert_eq!(b.title, "hello");
    assert!(b.body.is_empty());
    assert!(b.primary.is_none());
    assert!(b.secondary.is_none());
    assert!(matches!(b.kind, BannerKind::Info));
}

#[test]
fn warning_helper_sets_kind() {
    let b = Banner::warning_with_body("Title", "Body text.");
    assert!(matches!(b.kind, BannerKind::Warning));
    assert_eq!(b.body, "Body text.");
}

#[test]
fn primary_only_no_secondary() {
    let b = Banner {
        title: "Network error".into(),
        body: "Fetch failed.".into(),
        link: None,
        primary: Some(BannerAction {
            label: "Retry".into(),
            action_id: "RETRY_FETCH".into(),
        }),
        secondary: None,
        kind: BannerKind::Error,
        dismissible: true,
    };
    assert!(b.primary.is_some());
    assert!(b.secondary.is_none());
}
