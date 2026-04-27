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

## At-a-glance — Plan defects

| ID     | Title                                                              | Status | Severity |
|--------|--------------------------------------------------------------------|--------|----------|
| PD-001 | tracing EnvFilter directive `dat0=debug` doesn't match dat0 crates | open   | low      |
| PD-002 | Settings store atomic-write missing `fsync` before rename          | open   | low      |

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
