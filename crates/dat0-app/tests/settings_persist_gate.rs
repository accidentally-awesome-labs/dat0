use dat0_app::settings_ui::panel::changed;

#[test]
fn changed_is_true_only_on_difference() {
    assert!(changed("", "alice"));
    assert!(changed("alice", "bob"));
    assert!(!changed("alice", "alice"));
    assert!(!changed("", ""));
}
