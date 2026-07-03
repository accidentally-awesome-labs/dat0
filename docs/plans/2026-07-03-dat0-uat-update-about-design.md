# dat0 UAT automation — Update + About dialogs slice (design)

**Date:** 2026-07-03
**Branch:** `uat-update-about-dialogs` (off `main` `78f6ff9`)
**Slice:** Update + About dialogs (P10a / P10a-2 UI) — the "NEXT UP" item in the
UAT-backlog-automation effort. Follows the Settings-window slice (PR #39
`78f6ff9`) and reuses its AccessKit + async harness (`tests/support/mod.rs`,
`src/a11y/`).

Prior art / patterns: `docs/plans/2026-07-01-dat0-uat-settings-window-design.md`,
`crates/dat0-app/tests/settings_window.rs`.

---

## 1. Goal

Automate the headless-testable portion of the manual-UAT backlog for the About
box and the in-app updater UI (P10a nudge + P10a-2 auto-updater). Triaged from
`docs/plans/2026-06-17-dat0-p10a-uat.md` (§1.3 About, §1.4 update nudge) and
`docs/plans/2026-06-22-dat0-p10a-2-uat.md` (§4 opt-out, §5 manual menu action).

Close the genuine gap: the **UI content + behavioral layer** of these dialogs.
The pure logic (version/SHA/license text builder, semver compare, manifest
signature verify, SHA-256 download verify, apply/rollback, launch-policy bool,
writable→Swap/Nudge decision) is already unit-tested and is **not re-scoped
here**. The OS-integration and real-external items (Gatekeeper, notarization,
DMG/AppImage, self-swap, relaunch, real browser open, real network round-trip)
**stay human**.

## 2. Triage

| Item | Where | Status |
| --- | --- | --- |
| `about::summary_lines` (version/SHA/license/NOTICE/nudge rows) | `about/mod.rs:25` | ✅ pure-tested `tests/about_summary.rs` |
| `BuildInfo::current` (version/git_sha source) | `about/build_info.rs` | ✅ pure-tested |
| semver `newer_than`, `fetch_latest`, `fetch_update`, sig verify, SHA verify, download, apply+rollback | `update/{mod,check,manifest,download,apply}.rs` | ✅ pure-tested `tests/update_{check,manifest,download,apply}.rs` |
| `should_check_on_launch`, `prompt_action_for` | `update/ui.rs:21,38` | ✅ pure-tested (inline unit) |
| `update_auto_check` **persistence** (opt-out toggle) | `settings_ui/.../updates.rs`, `panel.rs:168` | ✅ already covered — `updates_toggle_click_persists` (Settings slice) + `updates.rs` unit |
| **About dialog renders real content** (up-to-date + newer-nudge variants) | `about/mod.rs:74` `present` | ❌ **gap → this slice** |
| **Update dialog states** (checking / available-prompt / up-to-date / failed) | `update/ui.rs` `show_*` | ❌ **gap → this slice** |
| **`is_manual` gating** (silent background vs. visible manual) | `update/ui.rs:125,137` | ❌ **gap → this slice** (real UI behavior, not the pure bool) |
| self-swap / relaunch / notarization / Gatekeeper / DMG / real browser / real network | — | 🚫 stays human |

## 3. Harness reality (the constraint that shapes everything)

`src/a11y/mod.rs` is a **custom thread-local collector**, not native
gpui/AccessKit. Two facts drive the design:

1. **Only dat0-annotated nodes are captured.** `.a11y(id, role, label)` and
   `.a11y_label(role, text)` `push()` a node into the thread-local `FRAME`
   (`a11y/mod.rs:76`). gpui-component's `Dialog` renders its title/body via plain
   `.title(String)` / `.child(String)` and its OK/Cancel buttons internally —
   **none of it is dat0-annotated**, so today the dialogs contribute nothing to
   the captured tree. To assert dialog *content*, dat0 must wrap the body text
   in `.a11y_label(...)`. The dialog *buttons* are gpui-component-internal and
   remain a11y-invisible (confirmed by the Settings slice) — button labels are
   therefore **not** content-assertable; they are exercised behaviorally only.

2. **`push()` fires at element-construction time, not paint time**
   (`a11y/mod.rs:156`). `A11ySnapshot::capture` does `reset()` →
   `window.refresh()` → `run_until_parked()` → `take_tree_update()`. Content is
   captured only if the element carrying `.a11y_label` is **re-constructed
   during that `refresh()`**. A live `Render` view (like `SettingsPanel`)
   re-runs its `render()` every refresh, so its annotations re-fire. Whether a
   gpui-component `Dialog`'s body closure re-runs on refresh is **unverified** —
   the Settings reset-dialog test only asserted `has_active_dialog` (presence),
   never dialog content. **This is the T0 hard-gate unknown (§7).**

3. **Release footprint is zero.** Under `#[cfg(not(feature = "a11y-capture"))]`
   both `.a11y` and `.a11y_label` are `#[inline]` identity no-ops
   (`a11y/mod.rs:181`). The only unconditional release change from annotating a
   dialog body is the wrapper `div()` the method is attached to (the
   `.a11y_label` call itself compiles away) — the same, accepted tradeoff the
   Settings slice made for its input wrappers (§6, Decision D3).

## 4. Architecture — the test seam

Both surfaces render gpui-component `Dialog`s from **`&mut App` free functions**
that reach a window via `cx.active_window()` + `handle.update` +
`window.open_dialog(...)`:

- About: `about::open(cx)` → `present(cx, None)` immediately, then spawns an
  off-thread `fetch_latest` and re-presents `present(cx, Some(tag))` on a newer
  release (`about/mod.rs:48`).
- Update: `run_update_flow(cx, is_manual)` shows "checking…" (manual only), then
  off-thread `fetch_update`, then posts back one of `show_up_to_date` /
  `show_error_banner` / `show_update_prompt` via the main-thread dispatcher
  (`update/ui.rs:74`).

**The seam: call the main-thread render helpers directly**, bypassing the
network + `std::thread::spawn` + dispatcher entirely. Each helper takes `&mut
App` plus deterministic arguments:

- `about::present(cx, None)` / `present(cx, Some("0.2.0"))`
- `update::ui::show_alert_dialog(cx, t("update.checking"))`
- `update::ui::show_up_to_date(cx, is_manual)`
- `update::ui::show_error_banner(cx, is_manual, "network down")`
- `update::ui::show_update_prompt(cx, fake_available_update("0.2.0"))`

`AvailableUpdate { version, artifact: ArtifactEntry }` is `pub` with `pub`
fields and `ArtifactEntry` is already constructed in `tests/update_download.rs`,
so a fake update is buildable with **zero network** (`update/mod.rs:17`).

**Mount pattern** — mirror `settings_window.rs::open_settings_window`: an
`add_window_view` wrapping a trivial placeholder view in `gpui_component::Root`,
`window.activate_window()` so `cx.active_window()` resolves. The dialog opens
into that Root's dialog layer. Assert with the Settings-slice dialog kit:
`window.has_active_dialog(app)` (presence, `root.rs:140`), `advance_clock(1s)` to
settle the open animation, `A11ySnapshot::capture` (content, if T0 permits),
`simulate_keystrokes("enter"|"escape")` to dismiss.

## 5. What each helper needs exposed

`present`, `show_up_to_date`, `show_error_banner`, `show_update_prompt`,
`show_alert_dialog` are private `fn`s. Integration tests are a separate crate and
see only `pub` items, and `#[cfg(test)]` items are invisible to them. Expose via
**feature-gated public shims** (mirrors `SettingsPanel::set_budget_input_value_for_test`,
`panel.rs:394`) — zero release footprint, no widening of the normal API:

```rust
// about/mod.rs
#[cfg(feature = "a11y-capture")]
pub fn present_for_test(cx: &mut App, newer: Option<String>) { present(cx, newer) }

// update/ui.rs
#[cfg(feature = "a11y-capture")]
pub fn show_up_to_date_for_test(cx: &mut App, is_manual: bool) { show_up_to_date(cx, is_manual) }
// …and for show_error_banner / show_update_prompt / show_alert_dialog
```

## 6. Decisions

**D1 — Content depth: add `.a11y_label` body seams (Fork 1, Option A).**
On-thesis for a "content + behavioral" slice and matches the Settings-slice
precedent. Wrap each dialog's body text in `div().a11y_label(AccessRole::Label,
text).child(text)` so the REAL rendered dialog content is assertable, not just
that *a* dialog opened. **Contingent on T0** proving dialog content is captured
(§7); if not, the content half falls back to D1-fallback (assert via the pure
`summary_lines`/i18n functions already tested; keep all presence + behavioral +
gating tests, which do not depend on content capture). *Rejected — presence-only
(Option B):* would make the slice largely redundant with existing pure tests.

**D2 — Browser/installer `on_ok`: never fire (Fork 2, Option A / YAGNI).**
`platform::open_url` shells out to `open`/`xdg-open` (no test seam), and the
update prompt's Install & Restart `on_ok` spawns a real installer thread that, in
a test context (`install_root()` → None), falls through to the browser nudge.
**No test fires an `on_ok` that reaches `open_url` or the installer.** Dialogs are
dismissed only via safe paths: `.alert()` OK (enter), and `.confirm()` **cancel**
("Later"/Cancel via escape or the cancel keybinding). *Rejected — open_url
recorder seam:* thin value (3-line shell-out), adds a platform seam.

**D3 — Wrapper `div` is unconditional; `.a11y_label` compiles away in release.**
The dialog body gains one `div()` layer in all builds (a real layout-tree
change) → **owes one human visual glance** on the About + update dialogs, exactly
as the Settings slice owed one on its window. Minimal (one div per dialog), and
the `.a11y_label` is a genuine accessibility improvement (dialog bodies should be
screen-reader-readable). D-015 stays open (no gpui fork; test-only capture path).

**D4 — No new deps, feature-gated test surface, `a11y-capture` only.** Zero
release footprint; NOTICE / lock unchanged. Matches the Settings slice.

## 7. T0 — spike hard-gate (STOP-and-report)

Before any content assertion is written, one spike test must prove, against the
REAL `about::present`, all of:

- **(a) Mount + activate** a minimal `Root` window under `TestPlatform` such that
  `cx.active_window()` resolves inside `present`.
- **(b) Presence:** `present_for_test(cx, None)` → after `advance_clock(1s)` +
  `run_until_parked`, `window.has_active_dialog(app) == true`.
- **(c) CONTENT CAPTURE (the make-or-break):** a `.a11y_label` placed in the
  dialog body surfaces in `A11ySnapshot::capture` — e.g.
  `has_label_contains("dat0")` or the version substring. This proves the dialog
  body closure re-runs during `refresh()` (or determines the correct capture
  bracket — a non-`reset()` immediate capture may be required; the spike
  resolves which).
- **(d) Dismiss:** `simulate_keystrokes("enter")` closes the alert →
  `has_active_dialog == false`.

**Go/no-go:**
- (a),(b),(d) green, (c) green → proceed with D1 (content + behavioral) fully.
- (a),(b),(d) green, (c) RED after exhausting capture brackets → proceed with
  **D1-fallback**: behavioral + gating tests as designed (they need only
  `has_active_dialog`), content asserted via the already-tested pure functions.
  Re-surface Fork 1 to the user with the empirical finding.
- (a) or (b) or (d) RED → STOP-and-report; the App-level `open_dialog` seam is
  not reachable headless and the whole slice needs rethinking.

Record the verdict in `.superpowers/sdd/task-0-report.md` (as the Settings slice
did).

## 8. Task breakdown

All tests land in one new binary `crates/dat0-app/tests/update_about_window.rs`
(`mod support;`), mirroring `settings_window.rs`. Test count ≈ 9 (T0 + 8).

- **T0 — spike hard-gate** (§7). Mount helper `open_dialog_host_window(cx)` +
  the four proofs. Gate.
- **T1 — About up-to-date content.** `present_for_test(cx, None)`; assert body
  contains the version (`env!("CARGO_PKG_VERSION")` = `0.1.0`), `Apache-2.0`, the
  NOTICE line, and `about.update.current` ("You're on the latest version."); and
  does **not** contain "Update available". Teeth: a fabricated version absent.
  Dismiss (enter) → closed. *(git SHA is never asserted — non-deterministic.)*
- **T2 — About newer-release content.** `present_for_test(cx, Some("0.2.0"))`;
  assert body contains `about.update.available` + `0.2.0` and does **not**
  contain `about.update.current`. Dismiss via **cancel** (escape) — do NOT fire
  Download (D2). Presence teeth as in T1.
- **T3 — "Checking…" alert.** `show_alert_dialog_for_test(cx, t("update.checking"))`;
  presence + (if T0 allows) content `update.checking`; dismiss → closed.
- **T4 — Up-to-date manual shows.** `show_up_to_date_for_test(cx, true)`;
  `has_active_dialog == true`; content `update.up_to_date`; dismiss → closed.
- **T5 — Up-to-date background SILENT (gating).** `show_up_to_date_for_test(cx,
  false)`; `has_active_dialog == false`. Teeth: assert `true` for the manual
  path in the same test (or rely on T4) so the `false` read is bound to the
  is_manual arg, not an already-empty window.
- **T6 — Error manual shows.** `show_error_banner_for_test(cx, true, "network
  down")`; presence; content contains `update.failed` (and the message
  substring); dismiss → closed.
- **T7 — Error background SILENT (gating).** `show_error_banner_for_test(cx,
  false, "network down")`; `has_active_dialog == false`.
- **T8 — Update-available prompt.** `show_update_prompt_for_test(cx,
  fake_available_update("0.2.0"))`; presence; content contains
  `update.available` + `0.2.0` (added to an `.a11y_label` body child — the
  version currently lives only in the title, which is a11y-invisible). Dismiss
  via **cancel** ("Later") — do NOT fire Install & Restart (D2). Assert the
  window survives (no panic) and the dialog closed.

Gating tests (T5, T7) and all presence/dismiss assertions depend only on
`has_active_dialog` (proven), so they hold even under the D1-fallback. Content
assertions (T1–T4, T6, T8) are the part contingent on T0(c).

## 9. Source changes required

- `about/mod.rs`: wrap the `present` dialog body in
  `div().a11y_label(AccessRole::Label, body.clone()).child(body)`; add
  `#[cfg(feature="a11y-capture")] pub fn present_for_test`.
- `update/ui.rs`: wrap the `show_update_prompt` body to carry the
  `update.available` + version line under `.a11y_label`; wrap the
  `show_alert_dialog` title/body under `.a11y_label`; add the four
  `#[cfg(feature="a11y-capture")] pub fn *_for_test` shims. (`show_up_to_date`
  and `show_error_banner` already route through `show_alert_dialog`, so
  annotating that one body covers them.)
- No changes to release behavior: `.a11y_label` is a no-op without the feature;
  the shims are feature-gated.
- New: `crates/dat0-app/tests/update_about_window.rs`.

## 10. Anti-loop execution protocol (from the Settings slice)

- **Implementer subagents run ONLY the fast focused test** synchronously:
  `cargo test -p dat0-app --test update_about_window --features a11y-capture`
  (never background `cargo test --workspace`; that stalls sonnet implementers on
  a dead monitor).
- **The controller runs the workspace gate**: `cargo test --workspace` +
  `cargo clippy --workspace --all-targets`, commits, and salvages any
  stalled/dead implementer by committing + gating itself.
- Per-task review after each task; final Opus whole-branch review before merge.
- Poll `gh pr checks`, not `gh run watch`. DCO `-s` + trailer. WATCH the
  post-merge main run (macOS grid-scroll bench is push-to-main-only).

## 11. Risks

- **R1 (primary): dialog content not captured on refresh** — mitigated by the T0
  hard-gate + D1-fallback; behavioral coverage is unaffected.
- **R2: `cx.active_window()` unresolved under TestPlatform** — T0(a) proves it;
  fallback would be refactoring `present`/`show_*` to take a `Window` (larger
  change, would re-surface to user).
- **R3: accidental browser/installer fire** — D2 forbids firing those `on_ok`s;
  reviewers check no test path reaches `open_url`/`perform_install`.
- **R4: multi-line body as one node** — assert via `has_label_contains`
  (substring) not `has_label_any` (exact); the pure test already covers per-row
  composition.

## 12. Owed human UAT after this slice

- One visual glance at the About dialog + the four update dialogs (D3 wrapper
  div) — same category as the Settings-window glance.
- Everything in §11 of the P10a doc and §1–3,§6–8 of the P10a-2 doc that is
  OS/notarization/real-network/self-swap bound remains human and is untouched by
  this slice.

## 13. Success criteria

- T0 spike green (or fallback verdict recorded) — observable go/no-go.
- ~9 tests green under `--features a11y-capture`; `cargo test --workspace` +
  `clippy --workspace --all-targets` clean.
- Zero release footprint: no new deps, NOTICE/lock unchanged, `.a11y_label`
  no-op verified off-feature (release build compiles unchanged).
- `is_manual` silent-background gating covered (T5, T7) — the genuinely new
  behavioral coverage beyond the pure `should_check_on_launch` bool.
- Post-merge main CI green both platforms; macOS bench held.
