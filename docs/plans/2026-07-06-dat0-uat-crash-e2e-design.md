# Slice 7 — Panic-trigger crash→sentinel write-path e2e (design)

**Date:** 2026-07-06
**Effort:** dat0 UAT-backlog automation, Slice 7 (the second capability enabler; keyboard-nav was Slice 6).
**Status:** design — awaiting user review before plan.

## Goal

Prove, end-to-end through the real `dat0` binary, that a genuine panic runs the
production panic hook and stages a **redacted** `last-crash.json` on disk. This
retires the last P10c manual-UAT item ("crash the app, confirm the sentinel is
written + redacted"). Slice 4 already automated the *relaunch/dialog* half
(seeded sentinel → crash dialog → Send/Dismiss); this slice automates the
*write* half (real panic → sentinel).

## Why this needs a new mechanism

`crate::telemetry::crash::install_panic_hook` (crash.rs:67) and its caller
`boot::CrashGuard::arm` (boot.rs:101) are the one crash-path seam **no test
exercises**. They cannot be unit-tested in-process:

- `std::panic::set_hook` is process-global — installing the real hook inside a
  test would clobber the harness's own hook and leak into sibling tests.
- Proving the hook *fires* means actually panicking a process and observing the
  side effect (a file on disk) after it dies.

Every *piece* around the assembly is already covered:

- `payload_from_panic` redaction — `crash_staging.rs::payload_from_panic_redacts_paths_and_keeps_message`
- `write_staged`/`read_staged`/`clear_staged` round-trip — `crash_staging.rs::staged_crash_round_trips_and_clears`
- marker lifecycle (`mark_running`/`clear_running`/`prior_crash_detected`) — `crash_staging.rs::marker_lifecycle_detects_unclean_exit`, `crash_guard.rs::guard_marks_on_arm_and_clears_on_drop`
- `resolve_relaunch_action` gating (incl. privacy opt-out) — Slice 4, `report_logic.rs`
- the crash/report dialog UI — Slice 4, `crash_report_window.rs`

The genuine gap is the **wired assembly**: `CrashGuard::arm` → real
`std::panic` → hook → `write_staged`. The faithful way to test process-global
panic behavior is **out of process**: spawn the real binary, make it crash, and
inspect the sentinel it leaves behind.

## Global constraints

- **No new dependencies.** `tempfile` is already a dev-dependency; `std::process`
  is std. NOTICE / Cargo.lock unchanged. D-015 (OS AccessKit adapter) stays open.
- **No production release-code change.** The only new production code is a
  hidden operator verb that is `#[cfg(debug_assertions)]`-gated → **absent from
  release binaries**. `install_panic_hook`/`payload_from_panic`/`write_staged`/
  `CrashGuard` are untouched — this slice only *exercises* them.
- **Zero owed human glance.** No UI, no rendered markup change.
- **Faithful to production.** The verb calls the exact `boot::CrashGuard::arm`
  that `main.rs:57` uses — not a re-implementation.

## Architecture

Three touch-points, all additive:

### 1. Hidden verb `__crash-test <dir>` (`crates/dat0-app/src/cli.rs`)

Mirrors the existing hidden `__telemetry-test` operator verb, but every piece is
`#[cfg(debug_assertions)]`-gated so it does not exist in a release build.

- **Enum variant** (in `PackageCmd`):
  ```rust
  /// Hidden debug-only trigger: `dat0 __crash-test <dir>`. Arms the real crash
  /// guard against <dir>, then deliberately panics with a fake-PII message so an
  /// out-of-process test can assert the staged, redacted `last-crash.json`.
  /// `#[cfg(debug_assertions)]` → NEVER compiled into a release binary.
  #[cfg(debug_assertions)]
  CrashTest { dir: Option<PathBuf> },
  ```

- **`parse()` recognition** — a `#[cfg(debug_assertions)]` early-return branch
  next to the `__telemetry-test` branch (recognized before the `VERBS` gate, so
  it never appears in `--help`):
  ```rust
  #[cfg(debug_assertions)]
  if verb == "__crash-test" {
      return Some(PackageCmd::CrashTest { dir: args.get(2).map(PathBuf::from) });
  }
  ```

- **`run()` handling** — a `#[cfg(debug_assertions)]` synchronous block beside
  the `TelemetryTest` block (before the tokio runtime is built; no runtime
  needed — the process is about to die):
  ```rust
  #[cfg(debug_assertions)]
  if let PackageCmd::CrashTest { dir } = &cmd {
      let dir = match dir {
          Some(d) => d.clone(),
          None => {
              eprintln!("usage: dat0 __crash-test <dir>");
              return 2;
          }
      };
      // Exact production assembly (main.rs:57): mark_running + install_panic_hook.
      let guard = crate::boot::CrashGuard::arm(&dir).expect("arm crash guard");
      // Model an abnormal exit: the guard's Drop (clear_running) must NOT run —
      // exactly as in a real crash (release `panic = "abort"` skips destructors).
      // `forget` keeps `running.marker` on disk deterministically in BOTH the
      // dev-profile unwind (this test) and the release-profile abort.
      std::mem::forget(guard);
      // Fake-PII payload → proves end-to-end redaction through the real hook.
      panic!("dat0 __crash-test sentinel /Users/secretuser/private.csv");
  }
  ```
  This diverges (never returns); the `std::process::exit(run(cmd))` in `main.rs`
  is never reached because `run` panics first — which is the point.

- **`run_async()` exhaustiveness arm** — a cfg'd unreachable arm (a cfg'd enum
  variant still forces the debug-build `match` to be exhaustive; the existing
  `TelemetryTest => unreachable!()` arm sets the precedent):
  ```rust
  #[cfg(debug_assertions)]
  PackageCmd::CrashTest { .. } => {
      unreachable!("CrashTest is handled in run() before the runtime")
  }
  ```

### 2. Out-of-process test (`crates/dat0-app/tests/crash_e2e.rs`, new)

A plain `#[test]` — no gpui, no `a11y-capture`, no `#[serial]`. The child runs in
its own address space and the parent mutates nothing process-global, so a unique
`tempfile::tempdir()` per test makes it fully parallel-safe.

```rust
use std::process::Command;
use dat0_app::telemetry::crash;

#[test]
fn real_panic_stages_redacted_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    // Cargo sets CARGO_BIN_EXE_<bin> for integration tests of a package that has
    // a [[bin]]; the bin is `dat0` in package `dat0-app`.
    let out = Command::new(env!("CARGO_BIN_EXE_dat0"))
        .args(["__crash-test"])
        .arg(dir.path())
        .output() // swallow the panic's stderr so it doesn't pollute test logs
        .expect("spawn dat0 __crash-test");

    // The child must die abnormally: dev unwind → exit 101; release abort →
    // SIGABRT. `!success()` is robust to both without hard-coding a code.
    assert!(!out.status.success(), "child must crash, got {:?}", out.status);

    // WRITE PATH: the real hook staged last-crash.json.
    let staged = crash::read_staged(dir.path())
        .expect("last-crash.json present + parseable after a real panic");

    // END-TO-END REDACTION through the real binary + real hook.
    assert!(
        staged.message.contains("dat0 __crash-test sentinel"),
        "panic marker preserved: {}",
        staged.message
    );
    assert!(
        !staged.message.contains("/Users/secretuser"),
        "absolute path must be redacted: {}",
        staged.message
    );
    assert!(!staged.backtrace.is_empty(), "backtrace captured");
    assert_eq!(staged.version, env!("CARGO_PKG_VERSION"));

    // The marker survived the abnormal exit (forget → no clear_running) — the
    // exact precondition Slice 4's seeded-sentinel relaunch test assumes.
    assert!(
        crash::prior_crash_detected(dir.path()),
        "running.marker survives an abnormal exit"
    );
}
```

### 3. Fast in-process parse unit test (`cli.rs` `#[cfg(test)] mod tests`)

A cheap guard that the verb is recognized, distinct from the heavy spawn test
(the test module builds with `debug_assertions` on, so `CrashTest` is in scope):

```rust
#[test]
fn crash_test_verb_parses_with_dir() {
    let cmd = parse(&argv(&["dat0", "__crash-test", "/tmp/x"])).unwrap();
    assert_eq!(cmd, PackageCmd::CrashTest { dir: Some(PathBuf::from("/tmp/x")) });
}
```

## Data flow

```
tests/crash_e2e.rs
  └─ Command::new(CARGO_BIN_EXE_dat0) ["__crash-test", <tempdir>]
       └─ main.rs: cli::parse(argv) → Some(CrashTest{dir})   [debug build only]
            └─ cli::run(CrashTest{dir})
                 ├─ CrashGuard::arm(dir)   → mark_running(dir)               → running.marker
                 │                          → install_panic_hook(dir)  (set_hook)
                 ├─ mem::forget(guard)      (Drop::clear_running skipped)
                 └─ panic!("… /Users/secretuser/private.csv")
                      └─ panic runtime invokes hook  [BEFORE unwind/abort decision]
                           └─ payload_from_panic(info, VERSION)
                                ├─ redact_text_pub(message)   → PII span → "<redacted>"
                                └─ Backtrace::force_capture() → redacted
                           └─ write_staged(dir, payload)                     → last-crash.json
                      └─ chain prev hook (default: print to stderr)
                      └─ unwind main thread → exit 101   (release: SIGABRT)
  └─ parent: read_staged(tempdir) → assert marker preserved, PII gone, version, backtrace
  └─ parent: prior_crash_detected(tempdir) → true (marker survived)
```

## Redaction — verified, not assumed

`redact_text` (redaction.rs:64) applies:
```
(/Users/[^/\s]+|/home/[^/\s]+|[A-Z]:\\[^\\\s]+)([\\/][^"'\s,]*)?  →  <redacted>
```
- `/Users/secretuser/private.csv` → the whole span collapses to `<redacted>`
  (`/Users/secretuser` matches the prefix alternative; `/private.csv` matches the
  optional tail). So `staged.message` contains `dat0 __crash-test sentinel <redacted>`.
- The marker `dat0 __crash-test sentinel` contains no `/Users`, `/home`, or
  `C:\` span → never redacted → survives intact.
- The panic **location** prefix (`panicked at <src path>:L:C`) is a compile-time
  *relative* path (`src/cli.rs`) → no PII span → unaffected. If a CI build uses
  an absolute source path it would be redacted, but the test asserts on neither
  the location nor the backtrace contents, so it is robust either way.

## Error handling / edge cases

- **Missing `<dir>` arg** → the verb prints `usage: dat0 __crash-test <dir>` to
  stderr and returns exit 2 (no panic). Not exercised by the happy-path test.
- **`CrashGuard::arm` I/O failure** (e.g. unwritable dir) → `.expect` panics → the
  process still dies, but no sentinel is written → `read_staged` returns `None` →
  the test fails loudly. Acceptable: an arm failure *is* a broken crash path.
- **Debug child death is a clean unwind → exit 101**, not a signal — no core-dump
  noise on the CI runner. Release (`cargo test --release`) → SIGABRT; still
  `!success()`.

## CI

**No workflow change.** The test rides the existing `cargo test --workspace`
gate in `ci.yml`'s build-and-test job on both macOS and Linux. Cargo builds the
`dat0` bin as a prerequisite of any integration test that references
`CARGO_BIN_EXE_dat0`, so the binary is available without an explicit build step.
Option A (debug-only verb) forecloses a release-mode crash job — and such a job
would only be exercising std's abort behavior, not ours. The external
`crash-e2e.yml` (GlitchTip round-trip, secrets-gated, soft) is untouched.

## Testing summary

- `tests/crash_e2e.rs::real_panic_stages_redacted_sentinel` — the enabler: real
  panic → redacted `last-crash.json` + surviving marker, out of process.
- `cli.rs` unit: `crash_test_verb_parses_with_dir` — fast parse guard.

## Scope / non-goals

- No change to `install_panic_hook`, `payload_from_panic`, `write_staged`,
  `CrashGuard`, or any UI.
- No new dependency; no release-code footprint (verb is debug-only).
- Not covered (stays human / out of scope): the real crash *upload* to GlitchTip
  (external round-trip, `crash-e2e.yml`), OS-level crash reporters, and the
  release-profile SIGABRT ordering (std behavior, nothing of ours to catch).

## Deliverable

One new integration test + one debug-only hidden operator verb. First slice with
**both** zero owed human glance **and** zero release-code change.
