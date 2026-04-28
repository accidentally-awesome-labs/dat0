# Deferrals & Plan-Defect Register

> **Purpose.** Single source of truth for scope splits and plan defects discovered
> during phase execution. Planning and brainstorming agents reference this document
> on entry to a phase to know what's already promised, what was punted forward, and
> what known plan-level bugs need addressing.

**Scope.** This register tracks two things:

1. **Deferrals (D-NNN)** — work that was originally scoped to one phase but was
   formally split off and re-scheduled to a later phase. Each entry records
   *what*, *from where*, *to where*, and *why*.
2. **Plan defects (PD-NNN)** — bugs in a phase's authored plan that surfaced
   during execution (e.g., wrong API signature in a snippet, stale dependency
   pin, broken filter directive). Recorded so they're fixed deliberately rather
   than accumulating as silent tech debt.

**Out of scope.** Pre-1.0 scope decisions made at spec authoring time (e.g.,
"Mac App Store deferred to v1.x", "Hosted tier deferred to post-PMF") live in
the spec's roadmap sections (§21.4 v1.x, §21.5 v2). They are not deferrals in
the sense tracked here — they were never in v1 scope.

**Status vocabulary**

| Status | Meaning |
|---|---|
| `open` | Not yet addressed. Will land in the target phase. |
| `in-progress` | Active work in the target phase. |
| `closed` | Shipped. Cross-reference the closing commit/PR/phase retro. |
| `cancelled` | Decided not to do. Cross-reference the decision rationale. |

**Update protocol.** Every phase plan must scan this register on entry and add
or close entries during execution. Every phase retro re-scans and updates
status. Treat the register as read-write within the worktree of the phase
that's modifying it; merge conflicts are signals worth investigating.

---

## At-a-glance — Deferrals

| ID    | Title                                              | Status | From | Target |
|-------|----------------------------------------------------|--------|------|--------|
| D-001 | Editable Settings widgets (author identity + theme dropdown) | open | P1 | P3 |
| D-002 | Theme live-switch through running window           | open | P1   | P3     |
| D-003 | Sparkle Objective-C `SUUpdater` bridge             | open | P1   | P10    |
| D-004 | AppImageUpdate subprocess invocation               | open | P1   | P10    |
| D-005 | Linux Secret Service "setup banner" UX             | open | P1   | TBD    |
| D-006 | macOS x86_64 (Intel) CI matrix coverage            | open | P1   | TBD    |
| D-007 | MotherDuck ATTACH end-to-end                       | open | P2   | P5     |
| D-008 | Cancellation-token wiring through `QueryEngine` trait | open | P2 | P5     |
| D-009 | Bundle `sqlite_scanner` static when duckdb-rs exposes a feature | open | P2 | TBD |
| D-010 | Non-UTF-8 file encoding handling                   | open | P2   | TBD    |

## At-a-glance — Plan defects

| ID     | Title                                                              | Status | Severity |
|--------|--------------------------------------------------------------------|--------|----------|
| PD-001 | tracing EnvFilter directive `dat0=debug` doesn't match dat0 crates | open   | low      |
| PD-002 | Settings store atomic-write missing `fsync` before rename          | open   | low      |
| PD-003 | cargo-about NOTICE output not deterministic across host platforms  | open   | low      |
| PD-004 | Linux Secret Service backend not reachable from CI keychain tests  | open   | low      |
| PD-005 | P2 plan T3 snippet uses `&*conn` triggering clippy explicit-auto-deref | closed | trivial |
| PD-006 | P2 plan T12 fixtures snippet uses bare `rng.gen::<…>()` — reserved keyword in Rust 2024 | closed | low      |
| PD-007 | P2 plan T14 snippet calls non-existent `error_ux::banner::push_banner` with mismatched `Banner` shape | closed | low      |

---

## Deferrals

### D-001 — Editable Settings widgets (author identity + theme dropdown)

- **Status:** open
- **Deferred from:** P1 (T7 schema, T16 settings UI)
- **Target phase:** P3
- **Reason:** Depends on form-input primitives (text input, select dropdown) from
  gpui-component that aren't a P1 concern.
- **What P1 ships:** TOML schema, atomic-write store, file watcher, sections
  registry, read-only display of current settings.
- **What target phase delivers:** Editable inputs for author-name + author-email
  on the Profile section; selectable theme dropdown on the Theme section. Both
  bound to the existing `SettingsStore`.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P1 exit — "Settings panel opens; changes persist across launches" (full editability)
- **Last touched:** 2026-04-26

### D-002 — Theme live-switch through running window

- **Status:** open
- **Deferred from:** P1 (T10)
- **Target phase:** P3
- **Reason:** Live theme application requires the running UI surfaces to consume
  the active theme through a reactive channel. P1 has too few theme-driven
  surfaces (one window with an empty view) for live-apply to be observable.
- **What P1 ships:** Zed-JSON theme schema, three built-in themes
  (`dark`/`light`/`high-contrast`), `Theme::load_builtin`, selection persistence.
- **What target phase delivers:** Theme observable channel; running window
  re-renders on theme change without restart.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P1 exit — "Theme switch (default ↔ alternate) works without restart"
- **Last touched:** 2026-04-26

### D-003 — Sparkle Objective-C `SUUpdater` bridge

- **Status:** open
- **Deferred from:** P1 (T15)
- **Target phase:** P10
- **Reason:** Cross-language Objective-C bridge needs notarized .app bundle +
  release pipeline before it's testable end-to-end. P1 doesn't sign or notarize.
- **What P1 ships:** HTTP GET smoke against the appcast URL (satisfies the
  exit criterion "makes a network call"); EdDSA public key bundled at build
  time via `include_str!`; `objc`/`cocoa` deps deliberately deferred.
- **What target phase delivers:** Real `SUUpdater` instantiation, appcast
  parsing, signature verification, in-app update prompt UI, restart flow.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P10 exit — "Sparkle 'Check for Updates' finds a test update + applies it"
- **Last touched:** 2026-04-26

### D-004 — AppImageUpdate subprocess invocation

- **Status:** open
- **Deferred from:** P1 (T15)
- **Target phase:** P10
- **Reason:** Real subprocess invocation of `appimageupdatetool` requires a
  packaged AppImage; P1 builds plain Linux binaries.
- **What P1 ships:** `AppImageUpdater` stub conforming to the `Updater` trait;
  logs a placeholder message.
- **What target phase delivers:** Subprocess wiring, progress reporting,
  restart flow.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Last touched:** 2026-04-26

### D-005 — Linux Secret Service "setup banner" UX

- **Status:** open
- **Deferred from:** P1 (T12)
- **Target phase:** TBD (likely P10 hardening)
- **Reason:** Banner UI ships when a feature first instantiates `Banner`. P1
  primitives exist (T17) but no feature consumes them yet.
- **What P1 ships:** Documented error path when Secret Service isn't running on
  Linux; integration tests gate cleanly via `#[cfg]` selection.
- **What target phase delivers:** Visible Banner shown on app start when keychain
  init fails, with link to user-runnable docs for `gnome-keyring-daemon` /
  `kwalletmanager` setup.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Last touched:** 2026-04-26

### D-006 — macOS x86_64 (Intel) CI matrix coverage

- **Status:** open
- **Deferred from:** P1 (T20)
- **Target phase:** TBD — gated on external CI capacity (self-hosted runner,
  paid runner pool, or GitHub queue recovery for `macos-13` images)
- **Reason:** GitHub-hosted Intel-Mac (`macos-13`) runners are heavily
  oversubscribed since Apple's transition to Apple Silicon. PR #1's
  `macos-13` job sat queued for 50+ minutes without ever starting; this
  is reportedly typical for the image. Holding P1's PR merge on its
  scheduling is not productive.
- **What P1 ships:** `macos-14` (Apple Silicon arm64) build + test in
  CI; `linux-x86_64` and `linux-arm64` matrix coverage. Local development
  still builds for any installed target. The Cargo workspace is
  cross-architecture-clean; `macos-13` would be a re-confirmation, not
  a new validation surface.
- **What target phase delivers:** Restore the `macos-13` matrix entry
  when one of: a self-hosted Intel-Mac runner is provisioned, GitHub's
  hosted queue stabilizes for the image, or the team migrates the matrix
  to a paid runner provider. Suggested phase: P10 hardening, since that's
  also when notarization and signing infrastructure lands.
- **Originating doc:** PR #1 first-run timeout; `.github/workflows/ci.yml`
  matrix definition.
- **Closes (partial):** spec §21.2 P1 exit — "Cold-launches on macOS arm64,
  macOS x86_64, Linux x86_64, Linux aarch64" — Apple Silicon + both Linux
  triples covered; macOS Intel coverage deferred.
- **Last touched:** 2026-04-26

### D-007 — MotherDuck ATTACH end-to-end

- **Status:** open
- **Deferred from:** P2
- **Target phase:** P5 (SQL Console)
- **Reason:** The `motherduck` Cargo feature does not exist in duckdb-rs as of
  the P2.T0 spike (verified 2026-04-27 against duckdb-rs `v1.4.4` —
  `crates/duckdb/Cargo.toml` feature graph contains no `motherduck` entry).
  The keychain primitive shipped in P1 has no consumer until a UI surface
  needs the token. Integration testing requires a MotherDuck dev DB credential
  not yet provisioned. The P2 spec exit names only `sqlite:` for end-to-end
  ATTACH coverage.
- **What P2 ships:** generic `attach()` method that parses the DSN prefix;
  `sqlite:` end-to-end (extension lazy-loaded via boot-path
  `INSTALL sqlite_scanner; LOAD sqlite_scanner;`); `md:` returns
  `EngineError::NotImplemented { feature: "MotherDuck" }`.
- **What target phase delivers:** motherduck extension load (boot-path,
  same template as sqlite_scanner); a `MotherDuckTokenStore` consumer of the
  P1 keychain primitive; integration tests against a MotherDuck dev DB;
  per-query timing chip ("local: 38ms / md: 412ms") in the P5 SQL Console
  status bar.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
- **Closes:** spec §6.5 entirely (partial closure — `sqlite:` lands in P2;
  `md:` lands in P5).
- **Last touched:** 2026-04-27

### D-008 — Cancellation-token wiring through `QueryEngine` trait

- **Status:** open
- **Deferred from:** P2
- **Target phase:** P5 (SQL Console)
- **Reason:** P2 has zero callers passing tokens — adding a
  `cancel: CancellationToken` parameter on every `execute*` trait method now
  would ship dead-weight ergonomics that can't be evaluated against a real
  call-site. P5 SQL Console is the first surface that needs `Cmd+.` → cancel
  propagation through a streaming query; trait shape is best evaluated against
  that real call-site rather than guessed twice. Engine internals already
  support cancellation today via `Engine::interrupt()` (backed by
  `Arc<duckdb::InterruptHandle>`, verified at P2.T0) — the trait amendment is
  signature ergonomics, not a behavioral change.
- **What P2 ships:** internal `Arc<duckdb::InterruptHandle>` field on
  `DuckDBEngine`; public `Engine::interrupt(&self)` method callable from
  sibling tasks; `EngineError::Interrupted` variant present in the error enum.
- **What target phase delivers:** trait amendment to add
  `cancel: CancellationToken` parameter on `execute` / `execute_paged` /
  `execute_streaming`; automatic interrupt-on-drop semantics for cancellation
  tokens; structured cancellation propagation through the streaming `mpsc`
  channel; Cmd+. UX wiring in the P5 SQL Console.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
  + §2.2.
- **Last touched:** 2026-04-27

### D-009 — Bundle `sqlite_scanner` static when duckdb-rs exposes a feature

- **Status:** open
- **Deferred from:** P2 (opens unconditionally with the spec; duckdb-rs's
  feature surface determines whether it ever closes)
- **Target phase:** TBD — closes when duckdb-rs adds a `sqlite_scanner`
  Cargo feature for static linking, OR stays open as documented intent if
  upstream never does. The constraint is real: the duckdb-rs README states
  the ICU extension isn't bundled "due to crates.io's 10MB package size
  limit," and `sqlite_scanner` is in the same distribution category.
- **Reason:** Opens unconditionally so we don't lose the intent if upstream
  adds the feature later. The "why" is the crates.io 10MB package size limit
  cited in the duckdb-rs README — the same constraint that keeps the ICU
  extension out of the bundled distribution applies to `sqlite_scanner`. As
  of P2.T0 verification (2026-04-27, duckdb-rs `v1.4.4`), the published
  feature surface contains no `sqlite_scanner` entry — confirmed by
  enumerating `crates/duckdb/Cargo.toml`.
- **What P2 ships:** lazy-load via the `dat0-app` boot path —
  `INSTALL sqlite_scanner; LOAD sqlite_scanner;` executed once before any
  window opens (per spec §2.5). First-run UX uses the P1 `Banner` primitive
  to show extension-download status. Engine `init()` per instance does
  `LOAD sqlite_scanner;` only (extension already installed by boot path).
- **What target phase delivers:** replaces boot-path `INSTALL` with a
  build-time static link, eliminating the first-run extension-download UX
  surface entirely. Boot path simplifies to `LOAD sqlite_scanner;` against
  the statically-linked extension.
- **Re-check trigger for closing:** duckdb-rs release notes mentioning a new
  feature flag for sqlite_scanner, OR the feature-graph row in
  `docs/internal/duckdb-arrow-api-notes.md` § "Extension features" gaining
  a positive entry on monthly upstream-watch refresh.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
  + §2.5; `docs/internal/duckdb-arrow-api-notes.md` § "Extension features".
- **Last touched:** 2026-04-27

### D-010 — Non-UTF-8 file encoding handling

- **Status:** open
- **Deferred from:** P2
- **Target phase:** TBD (likely P3 import wizard or v1.x polish)
- **Reason:** DuckDB's `read_csv` has no encoding parameter; supporting
  non-UTF-8 cleanly requires either Rust-side preconversion or a separate
  read path. Neither is a P2 concern — the engine's job is to expose what
  DuckDB supports natively. UI-side detection / conversion belongs with the
  P3 import wizard surface where the user can confirm the encoding choice.
- **What P2 ships:** `RegisterOpts` has no `encoding` field. CSV / TSV / JSON
  / NDJSON files are assumed UTF-8 (matches DuckDB's `read_csv` default).
  Non-UTF-8 input fails at parse time with a DuckDB error surfaced through
  `EngineError`.
- **What target phase delivers:** either (a) Rust-side preconversion via
  `encoding_rs` for CSV inputs flagged as non-UTF-8 by `chardet` /
  heuristic, with a banner ("We detected this file is encoded as <X>; do
  you want to convert?"), or (b) explicit user override in the import wizard
  with a conversion preview. P3 picks one or escalates to v1.x.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7.
- **Last touched:** 2026-04-27

---

## Plan defects

### PD-001 — tracing EnvFilter directive `dat0=debug` doesn't match dat0 crates

- **Status:** open
- **Severity:** low
- **Affected files:** `crates/dat0-app/src/boot.rs:init_logging`
- **Symptom:** `EnvFilter::new("info,dat0=debug")` matches by *module path*.
  All dat0 crates compile to module paths with underscores: `dat0_app::*`,
  `dat0_engine::*`, `dat0_format::*`, `dat0_i18n::*`, `dat0_keychain::*`. The
  directive `dat0=debug` matches `dat0::*` (which doesn't exist) and not any
  of the actual crates. Net effect: dev runs only show `INFO` events; nothing
  emits at `DEBUG` despite the apparent intent.
- **Discovered:** P1 T3 code review
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` Step 3.3
  + plan inheritance into T21 boot rewrite
- **Suggested fix:** Replace with explicit per-crate directives —
  `info,dat0_app=debug,dat0_engine=debug,dat0_format=debug,dat0_i18n=debug,dat0_keychain=debug`
  — or use a shared helper that enumerates dat0 crate prefixes.
- **Last touched:** 2026-04-26

### PD-002 — Settings store atomic-write missing `fsync` before rename

- **Status:** open
- **Severity:** low (durability, not correctness)
- **Affected files:** `crates/dat0-app/src/settings/store.rs:save`
- **Symptom:** Comment claims "Atomic write: write to .tmp, fsync, rename." but
  the code uses `std::fs::write` (which only writes + closes, no fsync). On
  macOS the rename is atomic at the directory-entry level, but the new file's
  data blocks may not be durable before the rename completes if the kernel
  hasn't flushed the page cache. On power loss the user could see a
  zero-length `settings.toml`.
- **Discovered:** P1 T8 implementer report
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` Step 8.3
- **Suggested fix:**
  ```rust
  let tmp = self.path.with_extension("toml.tmp");
  let mut f = std::fs::OpenOptions::new()
      .write(true).create(true).truncate(true).open(&tmp)?;
  use std::io::Write;
  f.write_all(serialized.as_bytes())?;
  f.sync_all()?;
  drop(f);
  std::fs::rename(&tmp, &self.path)?;
  // Optional: fsync the parent directory for durable rename:
  // std::fs::File::open(self.path.parent().unwrap_or(Path::new("/")))?.sync_all()?;
  ```
- **Acceptable when:** dat0 begins storing recoverable state in settings (e.g.,
  workspace pointers a user would lose if the file zeroed). Until then the cost
  of a corrupt settings file is "user re-enters preferences," which is low.
- **Last touched:** 2026-04-26

---

### PD-003 — cargo-about NOTICE output not deterministic across host platforms

- **Status:** open
- **Severity:** low (warn-only CI gate, not a blocker)
- **Affected files:** `about.toml`, `.github/workflows/notice.yml`, `NOTICE.md`
- **Symptom:** `cargo about generate` produces different output on macOS vs Linux
  hosts even with `targets = [all-4-targets]` in `about.toml`. Specifically,
  dual-licensed crates (e.g., `ureq` with `Apache-2.0 OR MIT`) get assigned
  different licenses depending on host platform's tiebreak — moving them
  between license sections in the rendered NOTICE. The `notice` CI job, which
  diffs the committed NOTICE against a fresh CI-generated one, fires on this
  one-line ordering noise.
- **Discovered:** P1 CI first run on PR #1 (2026-04-26) — committed NOTICE was
  generated on macOS arm64; Linux x86_64 runner regenerated and saw `ureq`
  in a different license section.
- **Mitigation in P1:** `notice` job set to `continue-on-error: true`. Failure
  reported as `::warning::` instead of `::error::`. Drift is still surfaced
  but doesn't block PRs.
- **Suggested fix:** Either (a) pin tie-broken licenses explicitly in
  `about.toml` per crate (e.g., `[crates.ureq] accepted = ["MIT"]`), or
  (b) restrict `targets` to a single canonical platform for the gate, or
  (c) compare a normalized form of the output (sort lines per section).
  Option (a) is the most-honest if the dual-license selection is genuinely a
  policy choice; option (b) is the simplest if NOTICE is intended as a
  generic union.
- **Last touched:** 2026-04-26

---

### PD-004 — Linux Secret Service backend not reachable from CI keychain tests

- **Status:** open
- **Severity:** low (tests skipped on Linux CI; macOS coverage still authoritative for keychain primitive)
- **Affected files:** `crates/dat0-keychain/tests/round_trip.rs`,
  `crates/dat0-app/tests/p1_exit_smoke.rs`, `.github/workflows/ci.yml`
- **Symptom:** Keychain round-trip tests on Linux runners panic with
  `SS error: result not returned from SS API`. The CI job sets up
  `dbus-launch` + `gnome-keyring-daemon --unlock --start --components=secrets`
  in one step and propagates `DBUS_SESSION_BUS_ADDRESS` via `$GITHUB_ENV`,
  but the daemon process does not survive cleanly across step boundaries
  on GitHub-hosted Ubuntu images. By the time `cargo test` runs, the
  Secret Service bus is unreachable.
- **Discovered:** P1 CI first run on PR #1 (2026-04-26) — both
  `ubuntu-latest` and `ubuntu-22.04-arm` failed.
- **Mitigation in P1:** keychain tests gated `#[cfg_attr(target_os = "linux", ignore = "...")]`
  so they're skipped on Linux runners. macOS keychain coverage (T12) is
  authoritative for the round-trip contract; Linux-side correctness is
  established only by compilation under `#[cfg(target_os = "linux")]`.
- **Suggested fix paths:**
  - **(a) Run tests under `dbus-run-session`:**
    ```yaml
    - name: Test (Linux)
      if: runner.os == 'Linux'
      run: |
        dbus-run-session -- bash -c '
          gnome-keyring-daemon --unlock --start --components=secrets <<<"" >/dev/null
          cargo test --workspace --target ${{ matrix.target.triple }}
        '
    ```
    Self-contained per-step session bus; daemon and tests live in the same
    invocation. Most reliable.
  - **(b) Use the OS-keyring crate's "mock" feature** for unit tests; keep
    integration tests macOS-only. Cleaner separation but less coverage.
  - **(c) Self-hosted Linux runner with persistent gnome-keyring** —
    overkill for what's tested.
- **Last touched:** 2026-04-26

---

### PD-005 — P2 plan T3 snippet uses `&*conn` triggering clippy explicit-auto-deref

- **Status:** closed
- **Severity:** trivial (style-only; auto-deref handles it)
- **Affected files:** `crates/dat0-engine/src/duckdb_engine.rs::apply_migrations_real`
- **Symptom:** Plan §"Task 3, Step 3.4" provides a verbatim snippet that calls
  `crate::migrations::apply_migrations(&*conn, …)` where `conn` is a
  `MutexGuard<duckdb::Connection>`. Under `clippy -D warnings` (Rust 1.95+
  toolchain pinned in `rust-toolchain.toml`), this fires
  `clippy::explicit_auto_deref` because `&conn` would coerce identically.
- **Discovered:** P2 T3 implementation (2026-04-27) — `cargo clippy
  --workspace --all-targets -- -D warnings` failed compile on first attempt.
- **Fix applied:** Replaced `&*conn` with `&conn` in the wired-in
  `apply_migrations_real` body. Behavior identical; clippy clean.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md` Step 3.4
  code block.
- **Closed by:** P2 T3 commit on branch `p2-engine`.
- **Last touched:** 2026-04-27

---

### PD-006 — P2 plan T12 fixtures snippet uses bare `rng.gen::<…>()` — reserved keyword in Rust 2024

- **Status:** closed
- **Severity:** low (mechanical fix; both raw-identifier escape and rand 0.9 upgrade are clean alternatives)
- **Affected files:** `crates/dat0-fixtures/src/main.rs`
- **Symptom:** Plan §"Task 12, Step 12.3" provides verbatim code with
  `rng.gen::<u32>()`, `rng.gen::<f64>()`, and friends. The workspace is on
  edition 2024 (`rust-version = "1.85"` in root `Cargo.toml`), where `gen` is a
  reserved keyword for the eventual `gen` block syntax. rand 0.8 still names
  its `Rng` trait method `gen`, so bare `rng.gen::<T>()` calls fail to parse
  with `error: expected identifier, found reserved keyword 'gen'`.
- **Discovered:** P2 T12 implementation (2026-04-27) — first
  `cargo build -p dat0-fixtures` produced 7 parse errors; rustc itself emits a
  `help: escape gen to use it as an identifier` hint suggesting `rng.r#gen::<T>()`.
- **Fix applied:** Replaced bare `rng.gen::<T>()` calls with the raw-identifier
  form `rng.r#gen::<T>()`. `rng.gen_bool(...)`, `rng.gen_range(...)` are
  unaffected (suffixed identifiers, not the `gen` keyword). Determinism
  preserved — `r#gen` is purely lexical and resolves to the identical method.
- **Alternative considered:** Upgrade to rand 0.9, which renames `Rng::gen` →
  `Rng::random`. Rejected as a larger change for T12; staying on rand 0.8
  matches the plan's pinned version and the existing transitive resolution in
  `Cargo.lock`. If a future task adopts rand 0.9 workspace-wide, T12 can drop
  the `r#gen` escapes.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md` Step 12.3
  code block (lines 3503-3601).
- **Closed by:** P2 T12 commit on branch `p2-engine`.
- **Last touched:** 2026-04-27

---

### PD-007 — P2 plan T14 snippet calls non-existent `error_ux::banner::push_banner` with mismatched `Banner` shape

- **Status:** closed
- **Severity:** low (P1 Banner is intentionally a pure-data type; the plan
  predicted this in its inline note and asked the implementer to adapt)
- **Affected files:** `crates/dat0-app/src/error_ux/banner.rs`,
  `crates/dat0-app/src/boot.rs`
- **Symptom:** Plan §"Task 14, Step 14.2" supplies verbatim code calling
  `crate::error_ux::banner::push_banner(crate::error_ux::banner::Banner { severity: …::Severity::Warning, title_key, body_key, link })`.
  None of those identifiers match P1's actual API:
  - No `push_banner` function exists — P1 ships `Banner` as a pure-data
    state struct with no in-process registry or service layer.
  - The severity enum is `BannerSeverity` (not `Severity`).
  - `Banner`'s fields are `{ message: String, severity, dismissible, action_label }`
    — there are no `title_key`, `body_key`, or `link` fields.
  - `Banner` does not exist as a render-attached primitive yet either; the
    intent in P1 was that real GPUI rendering wires it up at use sites in P7.
- **Discovered:** P2 T14 implementation (2026-04-27) on first attempt to
  compile boot.rs against the verbatim plan snippet. The plan itself
  flags the risk (line 3799) and instructs sub-agents to consult the
  actual file and adapt — so this is more "expected drift" than a defect,
  but logging per protocol since the verbatim code does not compile.
- **Fix applied:**
  1. Added a minimal in-process pending-banner queue to
     `error_ux::banner` (`push(Banner)` + `drain_pending() -> Vec<Banner>`)
     so boot-time call sites can stash banners before any window exists.
     The render layer (P7+) drains on first window open.
  2. Boot calls `banner::push(Banner::warning(format!("{title}: {body}")))`
     using the existing P1 constructor. The `link` field has no slot yet;
     the link target is documented in the i18n body for now and can be
     promoted to a structured field when `Banner` grows an action surface.
  3. i18n keys `boot.sqlite_scanner_install_failed.title` /
     `boot.sqlite_scanner_install_failed.body` added to
     `crates/dat0-i18n/src/strings/en.json` per plan.
- **Alternative considered:** Defer all banner UX to P7 and only
  `tracing::error!` at boot. Rejected — the queue-based solution is ~30
  lines, preserves the user-facing surface the plan promised, and is
  exactly the API shape P7 will need anyway.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md`
  Step 14.2 code block (lines 3775-3796).
- **Closed by:** P2 T14 commit on branch `p2-engine`.
- **Last touched:** 2026-04-27

---

## How to add an entry

1. Grab the next ID in sequence (`D-NNN` for deferrals, `PD-NNN` for plan defects).
2. Add a row to the at-a-glance table at top.
3. Add a full entry under the appropriate section, following the field schema
   above. Keep sections in numeric order.
4. Cross-link the originating phase plan and (if applicable) the spec exit
   criterion the deferral closes.
5. Update `Last touched:` to today's date in YYYY-MM-DD form.

## How to close an entry

1. Set `Status: closed`.
2. Append a `**Closed by:**` line citing the commit SHA, PR number, and phase
   retro that documented completion.
3. Move the at-a-glance row to a "Closed" sub-table at the bottom of each
   section once the count of closed entries grows beyond ~5; until then keep
   them inline so closure is visible at a glance.
