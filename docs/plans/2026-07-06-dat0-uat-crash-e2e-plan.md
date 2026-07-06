# Slice 7 — Panic-trigger crash→sentinel write-path e2e — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove end-to-end, through the real `dat0` binary, that a genuine panic runs the production panic hook and stages a redacted `last-crash.json`.

**Architecture:** Add a hidden, `#[cfg(debug_assertions)]`-only operator verb `__crash-test <dir>` to the CLI front-door (mirrors the existing `__telemetry-test`). It calls the real `boot::CrashGuard::arm` (the exact assembly `main.rs:57` runs), `std::mem::forget`s the guard to model an abnormal exit, then `panic!`s with a fake-PII message. A new out-of-process integration test spawns `CARGO_BIN_EXE_dat0` with that verb, lets it crash, and asserts the staged, redacted sentinel on disk. Because `std::panic::set_hook` is process-global, a separate process is the faithful way to test it.

**Tech Stack:** Rust, `std::process::Command`, `std::panic`, `tempfile` (dev-dep), the crate's own `telemetry::crash` module. No new dependencies.

**Design doc:** `docs/plans/2026-07-06-dat0-uat-crash-e2e-design.md`

## Global Constraints

- **No new dependencies.** `tempfile` is already a dev-dep; `std::process` is std. NOTICE / Cargo.lock / Cargo.toml unchanged. D-015 stays open.
- **No production release-code change.** The only new production code is the `__crash-test` verb, `#[cfg(debug_assertions)]`-gated at ALL sites → absent from release. `install_panic_hook` / `payload_from_panic` / `write_staged` / `CrashGuard` are untouched (only *exercised*).
- **Verb is hidden:** never in `VERBS`, never in the clap builder, never in `--help` — recognized only by an early-return branch in `parse`, exactly like `__telemetry-test`.
- **Faithful to production:** the verb calls `boot::CrashGuard::arm`, not a re-implementation.
- **No Cargo.toml change:** standard `tests/*.rs` files auto-discover (only `harness = false` benches are listed). No `[[test]]` entry for `crash_e2e.rs`.
- **Zero owed human glance:** no UI, no rendered markup.
- **Exact panic message:** `dat0 __crash-test sentinel /Users/secretuser/private.csv` — the `/Users/secretuser/private.csv` span is redacted to `<redacted>` by `redact_text` (redaction.rs:64 regex `(/Users/[^/\s]+|…)([\\/][^"'\s,]*)?`); the marker `dat0 __crash-test sentinel` has no redactable span and survives intact.
- **Controller gate:** after each task the controller runs `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` + a `cargo build -p dat0-app --release` (to confirm release-absence compiles). No cross-binary frame-count shift is expected (this slice adds no `.a11y` nodes — unlike Slice 6).

---

## File Structure

- **Modify `crates/dat0-app/src/cli.rs`** — add the `PackageCmd::CrashTest` variant, the `parse` recognition branch, the `run` handling block, the `run_async` exhaustiveness arm (all `#[cfg(debug_assertions)]`), and one fast in-process parse unit test. Responsibility: recognize + execute the hidden crash verb.
- **Create `crates/dat0-app/tests/crash_e2e.rs`** — the out-of-process enabler test. Responsibility: spawn the real binary, crash it, assert the redacted sentinel + surviving marker.

No other files change. `boot.rs`, `telemetry/crash.rs`, `telemetry/redaction.rs` are read-only dependencies.

---

## Task 0: Hidden `__crash-test` verb + hard-gate spawn spike

**This is the go/no-go gate.** It proves the entire risky seam: `CARGO_BIN_EXE_dat0` is set, the `#[cfg(debug_assertions)]` verb is present in the spawned child, `CrashGuard::arm` + real panic fires the hook, `write_staged` lands the sentinel, and the parent can read it. If Step 6 is red, **STOP and escalate** — the out-of-process approach is unviable and the design must be reconsidered (do not paper over a red spike).

**Files:**
- Modify: `crates/dat0-app/src/cli.rs` (enum `PackageCmd` ~lines 29-52; `parse` ~lines 66-111; `run` ~lines 200-228; `run_async` match ~lines 232-299; `#[cfg(test)] mod tests` ~lines 595-666)
- Create: `crates/dat0-app/tests/crash_e2e.rs`

**Interfaces:**
- Consumes (existing, verified signatures):
  - `crate::boot::CrashGuard::arm(data_dir: &Path) -> std::io::Result<CrashGuard>` (boot.rs:101) — marks running + installs the panic hook. `CrashGuard` is `pub`, module `boot` is `pub`.
  - `crate::telemetry::crash::read_staged(data_dir: &Path) -> Option<StagedCrash>` (crash.rs:44) — public, re-exported at `dat0_app::telemetry::crash`.
  - `crate::telemetry::crash::prior_crash_detected(data_dir: &Path) -> bool` (crash.rs:35).
  - `PackageCmd` already derives `Debug, Clone, PartialEq, Eq`; `use std::path::PathBuf;` already imported (cli.rs:14).
  - `argv(parts: &[&str]) -> Vec<String>` test helper (cli.rs:599).
- Produces:
  - `PackageCmd::CrashTest { dir: Option<PathBuf> }` (debug-only variant).
  - `tests/crash_e2e.rs::real_panic_stages_redacted_sentinel` (smoke form; expanded in Task 1).

---

- [ ] **Step 1: Add the `CrashTest` enum variant**

In `crates/dat0-app/src/cli.rs`, inside `pub enum PackageCmd { … }`, immediately after the `TelemetryTest,` variant (cli.rs:51) and before the closing `}`:

```rust
    /// Hidden debug-only trigger: `dat0 __crash-test <dir>`.
    ///
    /// Arms the real crash guard against `<dir>`, then deliberately panics with
    /// a fake-PII message so an out-of-process test can assert the staged,
    /// redacted `last-crash.json`. `#[cfg(debug_assertions)]` → NEVER compiled
    /// into a release binary. Recognised only by the early-return path in
    /// [`parse`]; never in `VERBS`, never in `--help`.
    #[cfg(debug_assertions)]
    CrashTest { dir: Option<PathBuf> },
```

- [ ] **Step 2: Add the `parse` recognition branch**

In `pub fn parse`, immediately after the `__telemetry-test` block (cli.rs:70-72) and before the `if !VERBS.contains(&verb.as_str())` gate (cli.rs:73):

```rust
    // Hidden debug-only crash trigger — same early-return discipline as
    // `__telemetry-test`, but compiled out of release builds entirely.
    #[cfg(debug_assertions)]
    if verb == "__crash-test" {
        return Some(PackageCmd::CrashTest {
            dir: args.get(2).map(PathBuf::from),
        });
    }
```

- [ ] **Step 3: Add the `run` handling block**

In `pub fn run`, immediately after the `TelemetryTest` `if let` block closes (cli.rs:219) and before `let rt = match tokio::runtime::Runtime::new()` (cli.rs:220):

```rust
    // Hidden debug-only crash trigger. Arms the REAL crash guard (the exact
    // assembly main.rs uses at boot), models an abnormal exit, and panics — so
    // `tests/crash_e2e.rs` can spawn this binary and assert the staged sentinel.
    #[cfg(debug_assertions)]
    if let PackageCmd::CrashTest { dir } = &cmd {
        let dir = match dir {
            Some(d) => d.clone(),
            None => {
                eprintln!("usage: dat0 __crash-test <dir>");
                return 2;
            }
        };
        let guard = crate::boot::CrashGuard::arm(&dir).expect("arm crash guard");
        // Model an abnormal exit: the guard's Drop (clear_running) must NOT run,
        // exactly as a real crash (release `panic = "abort"` skips destructors).
        // `forget` keeps `running.marker` on disk in BOTH the dev-profile unwind
        // (the test) and the release-profile abort.
        std::mem::forget(guard);
        // Fake-PII payload → proves end-to-end redaction through the real hook.
        panic!("dat0 __crash-test sentinel /Users/secretuser/private.csv");
    }
```

(Match on `&cmd` — a borrow — because the non-crash path still moves `cmd` into `run_async(cmd)` below. The `CrashTest` path always diverges (panic) or returns `2`, so the borrow never outlives that block.)

- [ ] **Step 4: Add the `run_async` exhaustiveness arm**

A `#[cfg(debug_assertions)]` enum variant still forces the debug-build `match cmd` in `run_async` to be exhaustive. In `pub async fn run_async`, immediately after the existing `PackageCmd::TelemetryTest => { unreachable!(…) }` arm (cli.rs:295-297) and before the match's closing `}` (cli.rs:298):

```rust
        // CrashTest is handled synchronously in `run()` before the runtime and
        // diverges (panics); it can never reach this async dispatch.
        #[cfg(debug_assertions)]
        PackageCmd::CrashTest { .. } => {
            unreachable!("CrashTest is handled in run() before the runtime")
        }
```

- [ ] **Step 5: Add the fast in-process parse unit test**

In `crates/dat0-app/src/cli.rs`, inside `#[cfg(test)] mod tests { … }`, after the last test `diff_parses_with_json_flag` (cli.rs:654-665) and before the module's closing `}`:

```rust
    #[test]
    fn crash_test_verb_parses_with_dir() {
        let cmd = parse(&argv(&["dat0", "__crash-test", "/tmp/x"])).unwrap();
        assert_eq!(
            cmd,
            PackageCmd::CrashTest {
                dir: Some(PathBuf::from("/tmp/x")),
            }
        );
    }
```

- [ ] **Step 6: Write the hard-gate spawn spike test**

Create `crates/dat0-app/tests/crash_e2e.rs` with the SMOKE form (Task 1 expands the assertions):

```rust
//! Out-of-process crash e2e (Slice 7): spawn the real `dat0` binary with the
//! hidden debug-only `__crash-test` verb, let it panic, and assert the panic
//! hook staged a `last-crash.json` sentinel. This exercises the ONE crash seam
//! no in-process test can reach: `boot::CrashGuard::arm` → real `std::panic` →
//! `install_panic_hook`'s closure → `write_staged`. `std::panic::set_hook` is
//! process-global, so the faithful test is a separate process.
//!
//! `CARGO_BIN_EXE_dat0` is injected by cargo for integration tests of the
//! `dat0-app` package (bin name `dat0`). The child is built in the dev/test
//! profile → `debug_assertions` on → the `__crash-test` verb is present.

use std::process::Command;

use dat0_app::telemetry::crash;

#[test]
fn real_panic_stages_redacted_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dat0"))
        .arg("__crash-test")
        .arg(dir.path())
        .output()
        .expect("spawn dat0 __crash-test");

    // Dev unwind → exit 101; release abort → SIGABRT. `!success()` covers both.
    assert!(
        !out.status.success(),
        "child must crash, got {:?}",
        out.status
    );

    // WRITE PATH: the real hook staged last-crash.json.
    assert!(
        crash::read_staged(dir.path()).is_some(),
        "last-crash.json must be present + parseable after a real panic"
    );
}
```

- [ ] **Step 7: Run the parse unit test (fast, in-process)**

Run: `cargo test -p dat0-app --lib cli::tests::crash_test_verb_parses_with_dir`
Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 8: Run the hard-gate spawn spike**

Run: `cargo test -p dat0-app --test crash_e2e`
Expected: PASS — `test real_panic_stages_redacted_sentinel ... ok`. (Cargo builds the `dat0` bin first, then runs the test; the child crashes with exit 101 and leaves `last-crash.json` in the tempdir.)

**GATE:** If this is RED — e.g. `CARGO_BIN_EXE_dat0` unset (env var missing at compile time), the verb not present in the child (cfg mis-gated), or `read_staged` returns `None` (hook never fired / write path broken) — STOP. Do not proceed to Task 1 or attempt workarounds. Report the exact failure so the approach can be reconsidered.

- [ ] **Step 9: Confirm release-absence compiles**

Run: `cargo build -p dat0-app --release`
Expected: builds cleanly (the `#[cfg(debug_assertions)]` variant/branch/arm all vanish; the `match` in `run_async` is exhaustive without the `CrashTest` arm; no unused-import or non-exhaustive-match error).

- [ ] **Step 10: Format**

Run: `cargo fmt --all` (the plan's example code is not pre-formatted; format before committing to pass the CI `--check` gate).

- [ ] **Step 11: Commit**

```bash
git add crates/dat0-app/src/cli.rs crates/dat0-app/tests/crash_e2e.rs
git commit -s -m "feat(crash): hidden debug-only __crash-test verb + spawn spike (Slice 7 T0)" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 1: Full sentinel + redaction + marker assertions

Upgrade the spike's single existence check into the enabler's full, non-vacuous teeth: the staged payload preserves the panic marker, redacts the fake-PII path end-to-end, carries a backtrace and the correct version, and the `running.marker` survives the abnormal exit.

**Files:**
- Modify: `crates/dat0-app/tests/crash_e2e.rs` (the body of `real_panic_stages_redacted_sentinel`)

**Interfaces:**
- Consumes: `crash::read_staged` → `Option<StagedCrash>`, where `StagedCrash { message: String, backtrace: String, version: String }` (crash.rs:12-17); `crash::prior_crash_detected(&Path) -> bool`; `env!("CARGO_PKG_VERSION")` (the test compiles inside `dat0-app`, so this equals the bin's version).
- Produces: nothing downstream (final task).

---

- [ ] **Step 1: Replace the smoke `read_staged` check with the full assertion block**

In `crates/dat0-app/tests/crash_e2e.rs`, replace the final `assert!(crash::read_staged(dir.path()).is_some(), …)` block (everything after the `!out.status.success()` assertion) with:

```rust
    // WRITE PATH: the real hook staged last-crash.json.
    let staged = crash::read_staged(dir.path())
        .expect("last-crash.json present + parseable after a real panic");

    // END-TO-END REDACTION through the real binary + real hook: the marker
    // survives, the fake-PII path does not.
    assert!(
        staged.message.contains("dat0 __crash-test sentinel"),
        "panic marker preserved: {}",
        staged.message
    );
    assert!(
        !staged.message.contains("/Users/secretuser"),
        "absolute path must be redacted end-to-end: {}",
        staged.message
    );
    assert!(!staged.backtrace.is_empty(), "backtrace captured");
    assert_eq!(staged.version, env!("CARGO_PKG_VERSION"));

    // The marker survived the abnormal exit (mem::forget → no clear_running) —
    // the exact precondition Slice 4's seeded-sentinel relaunch test assumes.
    assert!(
        crash::prior_crash_detected(dir.path()),
        "running.marker survives an abnormal exit"
    );
```

- [ ] **Step 2: Run the full e2e test**

Run: `cargo test -p dat0-app --test crash_e2e`
Expected: PASS — `test real_panic_stages_redacted_sentinel ... ok`. All five assertions hold: marker preserved, `/Users/secretuser` absent (redacted to `<redacted>`), backtrace non-empty, version equals `CARGO_PKG_VERSION`, marker present.

- [ ] **Step 3: Sanity-check the redaction teeth are non-vacuous**

Reasoning check (no code change): the message assertion `!contains("/Users/secretuser")` would FALSE-PASS only if the fake path never reached the payload. It does — it is the literal `panic!` argument, and `payload_from_panic` calls `redact_text_pub` on `info.to_string()`, which includes the panic message. If a future refactor dropped the redaction call, the fake path would appear verbatim and this assertion would fail. Confirm the assertion string `/Users/secretuser` exactly matches the panic literal's PII prefix in `cli.rs` Step 3.

- [ ] **Step 4: Format**

Run: `cargo fmt --all`

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/tests/crash_e2e.rs
git commit -s -m "test(crash): full redacted-sentinel + surviving-marker assertions (Slice 7)" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (design doc §Architecture 1/2/3, §Redaction, §CI, §Testing):
- Verb variant + parse + run + run_async arm (design §1) → Task 0 Steps 1-4. ✅
- Debug-only gating at all four sites → `#[cfg(debug_assertions)]` on each; release-absence verified Task 0 Step 9. ✅
- `CrashGuard::arm` + `mem::forget` + fake-PII panic (design §1) → Task 0 Step 3. ✅
- Fast in-process parse test (design §3) → Task 0 Step 5. ✅
- Out-of-process e2e test (design §2) → Task 0 Step 6 (smoke) + Task 1 (full). ✅
- End-to-end redaction, marker survival, version, backtrace (design §2, §Redaction) → Task 1 Step 1. ✅
- No Cargo.toml / dep change (design §Global constraints) → stated; `tempfile` confirmed dev-dep; auto-discovery confirmed. ✅
- No CI workflow change (design §CI) → rides `cargo test --workspace`; no plan step needed. ✅

**2. Placeholder scan:** No TBD/TODO. Every code step shows complete code; every run step shows the exact command + expected output. The `~line` references are navigation hints, not placeholders (exact anchor text — variant names, function names — is quoted). ✅

**3. Type consistency:** `PackageCmd::CrashTest { dir: Option<PathBuf> }` identical in the variant (T0 S1), parse (T0 S2), run match (T0 S3), run_async arm (T0 S4), and both tests (T0 S5, T1 S1). `crash::read_staged`/`prior_crash_detected`/`StagedCrash` field names match crash.rs. `CARGO_BIN_EXE_dat0` matches bin name `dat0`. Panic literal `dat0 __crash-test sentinel /Users/secretuser/private.csv` identical between the run block (T0 S3) and both assertions (T1 S1). ✅

**Task count:** 2 tasks — Task 0 (verb + hard-gate spike, the go/no-go) and Task 1 (full assertions). Right-sized: distinct review gates (release-safety/parse-correctness vs test-faithfulness/non-vacuous teeth).
