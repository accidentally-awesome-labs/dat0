//! Platform path contract.
//!
//! These tests assert only what dat0 itself CONTRIBUTES — the `"dat0"` app-name
//! suffix, and the `DAT0_CONFIG_DIR` relocation seam — and never the operating
//! system's own directory layout.
//!
//! # Why they no longer compare against `dirs::home_dir()`
//!
//! They used to assert `config_dir().starts_with(dirs::home_dir())`. That was a
//! claim about the ENVIRONMENT rather than about dat0, in two ways, and it is
//! the same failure class that broke `main` in `26a65ba` (a test asserting
//! `/usr` is unwritable, which stopped holding when the CI runner image rolled):
//!
//! 1. `config_dir()` returns `DAT0_CONFIG_DIR` verbatim when that variable is
//!    set, so the assertion silently required it to be UNSET. The variable is
//!    dat0's own test/portable-install seam and is set constantly by this
//!    repo's own harnesses — exporting it in a shell made `cargo test` fail in a
//!    way that looked like a code bug.
//! 2. On Linux the defaults come from `XDG_CONFIG_HOME` / `XDG_DATA_HOME` /
//!    `XDG_CACHE_HOME`, and the XDG spec permits any absolute path. A container
//!    image pointing those outside `$HOME` broke the assertion without anything
//!    in dat0 changing.
//!
//! It was also close to tautological: on Linux `data_dir()`/`cache_dir()` ARE
//! `dirs::data_dir()`/`dirs::cache_dir()` plus `"dat0"`, so asserting they sit
//! under `dirs::home_dir()` mostly tested the `dirs` crate.
//!
//! Every test here mutates a process-global environment variable, so all of them
//! are `#[serial]`.

use dat0_core::platform;
use serial_test::serial;

/// Run `f` with `DAT0_CONFIG_DIR` set to `value` (or removed when `None`),
/// restoring whatever was there before.
///
/// Taking control of the variable is the point: the old tests inherited it from
/// the ambient environment and assumed it was absent.
fn with_seam<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: every test in this file is `#[serial]`, so no other thread in this
    // process races these writes.
    unsafe {
        match value {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    let out = f();
    unsafe {
        match previous {
            Some(p) => std::env::set_var("DAT0_CONFIG_DIR", p),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

#[test]
#[serial]
fn config_dir_honours_the_relocation_seam() {
    let tmp = tempfile::tempdir().unwrap();
    let want = tmp.path().to_path_buf();
    let got = with_seam(Some(want.to_str().unwrap()), || {
        platform::config_dir().expect("config dir")
    });
    assert_eq!(
        got, want,
        "a non-empty DAT0_CONFIG_DIR must be returned VERBATIM — this is the \
         seam the test harnesses and portable installs depend on"
    );
}

#[test]
#[serial]
fn an_empty_seam_value_falls_through_to_the_default() {
    // The production guard is `.filter(|p| !p.is_empty())`; an empty value must
    // behave exactly as if the variable were unset, not relocate to "".
    let got = with_seam(Some(""), || platform::config_dir().expect("config dir"));
    assert!(
        got.ends_with("dat0"),
        "an empty seam value must fall through to the default location, got {got:?}"
    );
}

#[test]
#[serial]
fn default_dirs_are_suffixed_with_the_app_name() {
    // dat0's actual contribution over the OS-provided base directory is the
    // `dat0` component. The base itself is the platform's business, so it is
    // deliberately NOT asserted.
    with_seam(None, || {
        for (label, path) in [
            ("config", platform::config_dir().expect("config dir")),
            ("data", platform::data_dir().expect("data dir")),
            ("cache", platform::cache_dir().expect("cache dir")),
        ] {
            assert!(
                path.ends_with("dat0"),
                "{label}_dir must be namespaced under a `dat0` component, got {path:?}"
            );
            assert!(
                path.is_absolute(),
                "{label}_dir must be absolute, got {path:?}"
            );
        }
    });
}

/// Pins a real platform asymmetry that is otherwise invisible: the seam is
/// honoured by `config_dir` everywhere, and on macOS `data_dir` delegates to
/// `config_dir` so it follows too — but on Linux `data_dir` and `cache_dir` are
/// plain XDG lookups and do NOT. Asserted so a future edit to either platform
/// module cannot change the blast radius of the seam unnoticed.
#[test]
#[serial]
fn the_seam_reaches_exactly_the_documented_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let relocated = tmp.path().to_path_buf();
    let (data, cache) = with_seam(Some(relocated.to_str().unwrap()), || {
        (
            platform::data_dir().expect("data dir"),
            platform::cache_dir().expect("cache dir"),
        )
    });

    #[cfg(target_os = "macos")]
    assert_eq!(
        data, relocated,
        "on macOS data_dir() delegates to config_dir(), so the seam relocates it"
    );

    #[cfg(not(target_os = "macos"))]
    assert_ne!(
        data, relocated,
        "on Linux data_dir() is a plain XDG_DATA_HOME lookup — the seam must NOT \
         reach it; if this changed, say so deliberately"
    );

    // cache_dir never follows the seam on either platform.
    assert_ne!(
        cache, relocated,
        "cache_dir() is a plain platform lookup on every OS — the seam must not \
         reach it"
    );
}
