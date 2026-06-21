use std::process::Command;

fn main() {
    // Embed the short git SHA. Fall back to "unknown" on a shallow/no-git build.
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=DAT0_GIT_SHA={sha}");

    // Reproducible build time iff SOURCE_DATE_EPOCH is set (never wall-clock).
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        println!("cargo:rustc-env=DAT0_BUILD_TIME={epoch}");
    }
    // Re-run if HEAD moves.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
}
