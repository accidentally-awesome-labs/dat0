use std::process::Command;

#[test]
fn help_lists_all_subcommands() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--help")
        .output()
        .expect("run xtask --help");
    let text = String::from_utf8_lossy(&out.stdout);
    for sub in ["gen-icon", "bundle-macos", "sign-macos", "bundle-linux", "verify"] {
        assert!(text.contains(sub), "help missing subcommand: {sub}\n{text}");
    }
}
