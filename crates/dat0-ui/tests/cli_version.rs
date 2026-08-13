//! `dat0 --version` and `dat0 --help` must exit 0 and print, without a window.
//!
//! `release.yml`'s Linux job smoke-tests the AppImage by running
//! `./squashfs-root/AppRun --version` inside a bare `ubuntu:24.04` container
//! ("Verify on clean Ubuntu"). That container has no X11, no Wayland and no
//! GPU, so anything that reaches window creation fails there for reasons that
//! say nothing about the bundle.
//!
//! Out-of-process on purpose: the property is "the PROCESS exits 0 having
//! printed a version and started no window", which an in-process call to
//! `cli::run` cannot demonstrate. `CARGO_BIN_EXE_dat0` is injected by cargo for
//! integration tests of this package, which owns the `dat0` binary.
//!
//! The version line is also load-bearing as the FIRST stdout line: the smoke
//! test greps it, and `launch::main` short-circuits before `init_logging`
//! precisely so the tracing banner cannot get there first.

use std::process::Command;

/// `^dat0 \d+\.\d+\.\d+` by hand — no regex dependency needed for a shape this
/// small, and the hand-rolled version can say which part failed.
fn matches_version_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("dat0 ") else {
        return false;
    };
    let mut parts = rest.split('.');
    let (Some(major), Some(minor), Some(patch_and_rest)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // The patch component may carry a pre-release suffix and is always followed
    // by ` (<git sha>)`, so take its leading digit run.
    let patch: String = patch_and_rest
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    numeric(major) && numeric(minor) && numeric(&patch)
}

/// Run the binary with its boot side effects (config/data dirs, settings load,
/// watcher thread) contained in scratch dirs. `AppContext::boot()` runs before
/// the CLI front door, so it happens even for `--version`.
fn run(flag: &str) -> std::process::Output {
    let scratch = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_dat0"))
        .arg(flag)
        .env("DAT0_CONFIG_DIR", scratch.path())
        .env("XDG_DATA_HOME", scratch.path())
        .output()
        .unwrap_or_else(|e| panic!("spawn dat0 {flag}: {e}"))
}

#[test]
fn the_version_flag_prints_the_build_version_and_exits_zero() {
    let out = run("--version");

    assert!(
        out.status.success(),
        "`dat0 --version` must exit 0, got {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    assert!(
        matches_version_line(first),
        "first stdout line must match `^dat0 \\d+\\.\\d+\\.\\d+`, got {first:?}\nfull stdout:\n{stdout}"
    );

    // Compare against the constant the binary itself prints from, rather than
    // this test crate's own version: they are separate packages.
    let version = dat0_core::about::build_info::BuildInfo::current().version;
    assert!(
        first.contains(version),
        "version line must report the compiled version ({version}), got {first:?}"
    );
}

#[test]
fn the_help_flag_lists_every_package_verb_and_exits_zero() {
    let out = run("--help");

    assert!(
        out.status.success(),
        "`dat0 --help` must exit 0, got {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    // The same five verbs `cli::VERBS` gates on. Rendered from `cli_command()`,
    // so a new verb cannot ship undocumented.
    for verb in ["export", "unpack", "inspect", "replay", "diff"] {
        assert!(
            stdout.contains(verb),
            "`dat0 --help` must list the `{verb}` verb\nfull stdout:\n{stdout}"
        );
    }
}
