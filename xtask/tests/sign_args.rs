use xtask::sign::codesign_args;

#[test]
fn codesign_uses_hardened_runtime_and_timestamp() {
    let args = codesign_args("Developer ID Application: Acme", "target/macos/dat0.app");
    assert!(args.contains(&"--options".to_string()));
    assert!(args.contains(&"runtime".to_string())); // hardened runtime
    assert!(args.contains(&"--timestamp".to_string())); // secure timestamp
    assert!(args.contains(&"--force".to_string()));
    assert!(args.iter().any(|a| a.contains("dat0.app")));
    assert!(args.iter().any(|a| a.contains("Developer ID Application")));
}
