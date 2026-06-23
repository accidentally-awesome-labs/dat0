use dat0_app::update::apply;

#[test]
fn install_root_returns_some_or_none_without_panic() {
    // install_root() should not panic — in a test context the binary is not
    // dat0.app on macOS so it may return None; that is acceptable.
    let _ = apply::install_root();
}

#[test]
fn is_writable_true_for_user_temp() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(apply::is_writable(tmp.path()));
}

#[test]
fn is_writable_false_for_root_owned() {
    // /usr is root-owned on macOS+Linux; a normal CI user cannot write it.
    assert!(!apply::is_writable(std::path::Path::new("/usr")));
}

#[test]
fn apply_then_rollback_restores_on_failure() {
    // Simulate: install dir exists, "downloaded" is a bad/missing payload → apply errors,
    // and the original install is left intact (rollback).
    let tmp = tempfile::tempdir().unwrap();
    let install = tmp.path().join("dat0.app");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("marker"), b"v1").unwrap();
    let bad = tmp.path().join("does-not-exist.tar.gz");
    // bad does NOT exist — apply must fail in Phase 1 (before move-aside)
    assert!(apply::apply_update(&install, &bad).is_err());
    // marker must still be v1 — install must be intact
    let content = std::fs::read(install.join("marker")).unwrap();
    assert_eq!(content, b"v1");
}
