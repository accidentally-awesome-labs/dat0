use dat0_app::theme::Theme;

#[test]
fn dark_loads() {
    let t = Theme::load_builtin("dark").unwrap();
    assert_eq!(t.name, "dark");
}

#[test]
fn light_loads() {
    let t = Theme::load_builtin("light").unwrap();
    assert_eq!(t.name, "light");
}

#[test]
fn high_contrast_loads() {
    let t = Theme::load_builtin("high-contrast").unwrap();
    assert_eq!(t.name, "high-contrast");
}

#[test]
fn unknown_returns_err() {
    let r = Theme::load_builtin("does-not-exist");
    assert!(r.is_err());
}
