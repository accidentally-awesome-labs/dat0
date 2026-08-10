use dat0_core::update::apply;

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

// ---------------------------------------------------------------------------
// `is_writable` negative cases.
//
// These used to be a single assertion that `/usr` is not writable, which tested
// the ENVIRONMENT rather than the function: `is_writable` write-probes the path,
// so the assertion held only while the CI user happened to lack write access to
// `/usr`. GitHub's ubuntu image `ubuntu24/20260726.254` broke that assumption
// and reddened `main` (macOS was unaffected — SIP keeps `/usr` unwritable
// there). The cases below construct their own unwritable paths instead, so they
// depend on nothing but the filesystem's own rules.
// ---------------------------------------------------------------------------

#[test]
fn is_writable_false_for_missing_directory() {
    // The probe write fails with ENOENT because the parent does not exist.
    // True for every user INCLUDING root, so no privilege assumption.
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("no-such-dir");
    assert!(!apply::is_writable(&missing));
}

#[test]
fn is_writable_false_for_a_regular_file() {
    // `is_writable` joins a probe filename onto the path, so a non-directory
    // target fails with ENOTDIR. Also privilege-independent — root cannot make
    // a regular file behave like a directory either.
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("not-a-dir");
    std::fs::write(&file, b"x").unwrap();
    assert!(!apply::is_writable(&file));
}

#[cfg(unix)]
#[test]
fn is_writable_false_for_a_read_only_directory() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    let ro = tmp.path().join("read-only");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();

    // Mode bits do not restrain a process with CAP_DAC_OVERRIDE (root), so this
    // case cannot demonstrate anything there. Decide that with an INDEPENDENT
    // oracle — a direct `std::fs::write` — never by asking the function under
    // test: gating on `is_writable` itself would make the test unable to fail,
    // since a broken `is_writable` returning true would route into the skip.
    let direct = ro.join(".independent_probe");
    let env_permits_write = std::fs::write(&direct, b"").is_ok();
    if env_permits_write {
        let _ = std::fs::remove_file(&direct);
        eprintln!(
            "SKIP is_writable_false_for_a_read_only_directory: this process can \
             write a 0o555 directory (running as root?), so permission denial \
             cannot be exercised here"
        );
    } else {
        assert!(
            !apply::is_writable(&ro),
            "the filesystem refused a direct write to this 0o555 directory, so \
             `is_writable` must refuse it too"
        );
    }

    // Restore write permission so the tempdir can clean itself up.
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
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
