use std::process::Command;

#[test]
fn help_lists_all_subcommands() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--help")
        .output()
        .expect("run xtask --help");
    let text = String::from_utf8_lossy(&out.stdout);
    // Every subcommand `main.rs` declares. `gen-manifest` was missing from this
    // list while it existed in the binary, so the assertion proved nothing
    // about it — a subcommand can only be undiscoverable by accident once.
    for sub in [
        "gen-icon",
        "bundle-macos",
        "sign-macos",
        "bundle-linux",
        "verify",
        "gen-manifest",
        "perf",
    ] {
        assert!(text.contains(sub), "help missing subcommand: {sub}\n{text}");
    }
}
