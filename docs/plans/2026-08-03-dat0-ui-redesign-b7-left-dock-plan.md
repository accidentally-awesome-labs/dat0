# UI Redesign B7 — Left Dock + Activity Rail Implementation Plan

> **For agentic workers:** This slice is executed INLINE by the controller (no
> subagents), matching every slice since A5. Steps use checkbox (`- [ ]`) syntax
> for tracking.

**Goal:** Move the Catalog, Connections and AI panels out of the shell's body row
into the `DockArea`'s left dock, selected one-at-a-time by a new VSCode-style
activity rail.

**Architecture:** One `DockItem::tabs` holding three thin panels (B5/B6 template:
each owns a single `WeakEntity<WorkspaceShell>` and delegates rendering back to
the shell). The shell's three existing visibility bools stay the single source of
truth, now under an at-most-one-true invariant with a single writer. A 48 px
hand-rolled rail — a sibling of the `DockArea`, not a dock panel — selects which
panel shows.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`, DuckDB,
`dat0_i18n`, the `a11y-capture` test harness.

**Design:** `docs/plans/2026-08-03-dat0-ui-redesign-b7-left-dock-design.md`

## Global Constraints

- Branch `feat/ui-redesign-b7-left-dock` off main `921dde8`. Every commit
  `git commit -s` (DCO). **Never** write the CI skip marker in any commit
  message, even quoted in prose.
- `cargo fmt --all` before **every** commit.
- Session schema stays **v10** — no `SessionUiState` field added or removed.
  `dock_layout` is B9's.
- `tests/style_lint.rs` ratchet must stay exactly `ALLOW = &[("window.rs", 1)]`.
  **No new colour literals** — every colour via `cx.theme().*` or
  `cx.theme().d0().*`.
- `src/grid/` and `src/session/` must be byte-identical to main at the end
  (`git diff --stat main -- crates/dat0-app/src/grid crates/dat0-app/src/session`
  empty).
- Panel names are frozen from this slice on: `"CatalogPanel"`,
  `"ConnectionsPanel"`, `"AiDockPanel"`.
- `pub const ALL: &'static [T]` fails `clippy::redundant_static_lifetimes` under
  `-D warnings`. Write `&[T; N]`.
- `git commit -m "…"` command-substitutes backticks in zsh. Use `-F -` + heredoc
  for any message containing them.
- `cargo test --workspace` and `cargo bench` are unrunnable on this machine
  (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift, reproduces on `main`).
  Per-task verification uses `-p dat0-app` with an explicit `--test`.
- After reverting a red-first probe, **`touch` the source file** before re-running
  — a backwards-dated file makes cargo reuse the stale binary and report a false
  red (A6).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/dat0-app/assets/icons/{database,plug,sparkles}.svg` | **Create.** Verbatim Lucide copies for the three rail icons. |
| `crates/dat0-app/src/assets.rs` | **Modify.** `Dat0IconName` 5 → 8 variants. |
| `NOTICE.md` | **Modify.** Vendored icon count 5 → 8; `database` recorded as Feather-derived (MIT). |
| `crates/dat0-i18n/src/strings/en.json` | **Modify.** +4 keys: `rail.title`, `rail.show_catalog`, `rail.show_connections`, `rail.show_ai`. |
| `crates/dat0-app/src/window.rs` | **Modify.** `LeftPanel` enum, the exclusivity invariant, three `render_*_body` methods, the left-dock mount, `sync_left_dock`, rail state + wiring, three rewritten test shims, three rewritten menu handlers. |
| `crates/dat0-app/src/panels/{catalog_panel,connections_panel,ai_dock_panel}.rs` | **Create.** Three thin `Panel` implementors. |
| `crates/dat0-app/src/panels/mod.rs` | **Modify.** Register the three new panel names. |
| `crates/dat0-app/src/view/activity_rail.rs` | **Create.** The rail's render function and item table. |
| `crates/dat0-app/src/view/mod.rs` | **Modify.** `pub mod activity_rail;` |
| `crates/dat0-app/src/{catalog,connections,ai}/panel.rs` | **Modify.** Drop each body title row (it moves to `Panel::title`). |
| `crates/dat0-app/tests/left_dock.rs` | **Create.** Dock + invariant + rail suite. |
| `crates/dat0-app/tests/a11y_spike.rs` | **Modify.** Exact click-id count bump, with a comment naming the new nodes. |
| `crates/dat0-app/src/view/sql_console.rs` | **Modify.** Correct two stale comments claiming no tooltip helper exists at this rev. |

---

## Task 0: T0 hard gate — prove the focus migration before building

**Files:**
- Create (throwaway): `crates/dat0-app/tests/left_dock_spike.rs`
- Modify (findings only): the design doc, new §12b

**Interfaces:**
- Consumes: nothing.
- Produces: a go/no-go on `DockItem::tabs`, and the measured `a11y_spike`
  click-id delta that Task 5 uses to set the new constant.

This task answers §12's five probes. It is a **hard gate**: if P1 or P2 fails, stop
and re-plan Task 4 around `DockItem::split` before writing any production code.

- [ ] **Step 1: Write the spike**

Copy the harness verbatim from `tests/right_dock.rs:13-111` (`set_config_dir`,
`init_components`, `build_empty_session`, `open_shell_window`, `boot`, `settle`),
then add a probe that mounts a `DockItem::tabs` left dock holding the *existing*
`GridPanel` type three times — the point is to measure chrome and Tab behaviour,
not the real panels, which do not exist yet.

```rust
//! B7 T0 SPIKE — throwaway. Answers design §12's P1-P5 before any production
//! code is written. Deleted (or promoted into tests/left_dock.rs) at Task 6.

mod support;
// ... harness copied verbatim from tests/right_dock.rs:13-111 ...

/// P3: exactly one visible panel must render a 30px TITLE ROW, not a tab bar.
/// Upstream takes the title branch when `visible_panels.len() == 1 &&
/// panel_style == PanelStyle::default()` (tab_panel.rs:623-625).
#[gpui::test]
#[serial]
fn p3_one_visible_panel_renders_a_title_row_not_a_tab_bar(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    // Catalog is the only left panel a fresh shell can show without seeding.
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| ws.catalog_panel_visible = true);
    });
    settle(vcx);

    let snap = A11ySnapshot::capture(vcx);
    // A tab bar would render EVERY visible panel's name; a title row renders one.
    assert_eq!(
        snap.count_label(&dat0_app::dat0_i18n::t("catalog.title")),
        1,
        "exactly one node should carry the catalog name with one panel visible"
    );
}

/// P1: Tab must still reach `catalog-tree` with the panel inside a `.tab_group()`.
/// Drive the KEYMAP (`simulate_keystrokes`), never `dispatch_action` — the latter
/// bypasses the keymap and a green test can hide a dead production key path.
#[gpui::test]
#[serial]
fn p1_tab_still_reaches_the_catalog_tree(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| ws.catalog_panel_visible = true);
    });
    settle(vcx);

    // A fresh window has NOTHING focused, and with nothing focused the dispatch
    // path is the window root ALONE — not even Root's own `tab` binding matches,
    // so Tab is completely inert (B1). Click into the shell first.
    vcx.simulate_click(gpui::Point::new(gpui::px(400.), gpui::px(400.)), gpui::Modifiers::none());
    settle(vcx);

    // `focused_label()` returns `Option<&str>` BORROWED from the snapshot, so
    // bind both the snapshot and the wanted String — comparing against a
    // temporary `t(..)` is a dropped-while-borrowed error.
    let want = dat0_app::dat0_i18n::t("catalog.title");
    let mut reached = false;
    for _ in 0..24 {
        vcx.simulate_keystrokes("tab");
        settle(vcx);
        let snap = A11ySnapshot::capture(vcx);
        if snap.focused_label() == Some(want.as_str()) {
            reached = true;
            break;
        }
    }
    assert!(reached, "P1 FAILED: `catalog-tree` is unreachable by Tab inside a tab group");
}

/// P4: measure the click-id delta the dock chrome contributes. A MEASUREMENT,
/// not a pass/fail — the number sets `tests/a11y_spike.rs`'s constant at Task 5.
#[gpui::test]
#[serial]
fn p4_measure_click_id_count(cx: &mut TestAppContext) {
    let (_shell, vcx) = boot(cx);
    let snap = A11ySnapshot::capture(vcx);
    println!("P4 hero click_ids = {}", snap.click_ids.len());
}

/// P5: does `.tooltip(..)` with `gpui_component::Tooltip` compile and render?
/// gpui core exposes the hook (`gpui-0.2.2/src/elements/div.rs:1161`) and
/// `Tooltip::build` returns the `AnyView` it wants (`tooltip.rs:62`).
#[gpui::test]
#[serial]
fn p5_tooltip_compiles_and_renders(cx: &mut TestAppContext) {
    let (_shell, vcx) = boot(cx);
    vcx.update(|window, app| {
        let _probe = gpui::div()
            .id("tooltip-probe")
            .tooltip(|window, app| gpui_component::Tooltip::new("probe").build(window, app));
        let _ = (window, app);
    });
    settle(vcx);
}
```

- [ ] **Step 2: Run the spike**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock_spike -- --nocapture
```

Expected: P3, P4 and P5 pass. **P1 is the gate.**

- [ ] **Step 3: Add P2 — the eight AI handles**

P2 needs a seeded AI panel. Copy the seeding idiom from `tests/ai_nav.rs`
(`seed_ai_panel_for_test`, which bypasses `hydrate_ai_panel`'s keychain probe),
then walk Tab and collect every focused label, asserting all eight `ai-*` stops
appear.

```rust
/// P2: with AI active, all eight `ai-*` handles stay reachable and ordered.
#[gpui::test]
#[serial]
fn p2_ai_handles_stay_reachable(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_ai_panel_for_test(dat0_app::ai::panel::AiPanel::default());
        });
    });
    settle(vcx);
    vcx.simulate_click(gpui::Point::new(gpui::px(400.), gpui::px(400.)), gpui::Modifiers::none());
    settle(vcx);

    let mut seen: Vec<String> = Vec::new();
    for _ in 0..40 {
        vcx.simulate_keystrokes("tab");
        settle(vcx);
        // Bind the snapshot: `focused_label()` borrows it, and it returns
        // `Option<&str>`, so own the string before pushing.
        let snap = A11ySnapshot::capture(vcx);
        if let Some(l) = snap.focused_label() {
            let l = l.to_string();
            if !seen.contains(&l) {
                seen.push(l);
            }
        }
    }
    println!("P2 focus walk = {seen:#?}");
    // The expected label strings come from `tests/ai_nav.rs`'s existing
    // assertions — that suite already knows what each `ai-*` stop announces.
    // Copy them in rather than guessing; this is a measurement plus one gate.
    assert!(
        !seen.is_empty(),
        "P2 FAILED: the Tab walk focused nothing at all"
    );
}
```

Resolve the expected label strings from `tests/ai_nav.rs`'s existing assertions
rather than hardcoding — that suite already knows what each `ai-*` stop announces.

- [ ] **Step 4: Run the full spike and record findings**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock_spike -- --nocapture
```

**STOP CONDITIONS:**
- P1 or P2 fails ⇒ **do not proceed to Task 1.** Switch Task 4 to
  `DockItem::split` (design §3's retained fallback), amend the design doc, and
  re-plan.
- P3 fails (a tab bar with one visible panel) ⇒ the rail model needs re-design;
  stop and surface to the owner.
- P5 fails ⇒ drop tooltips to icon-only a11y labels; amend design §7.4. Nothing
  else in the plan changes.

- [ ] **Step 5: Commit the findings**

Append a §12b "T0 as-built" section to the design doc recording each probe's
actual result and the measured P4 number. Keep the spike file — it is deleted or
promoted at Task 6.

```bash
cargo fmt --all
git add docs/plans/2026-08-03-dat0-ui-redesign-b7-left-dock-design.md \
        crates/dat0-app/tests/left_dock_spike.rs
git commit -s -F - <<'EOF'
test(theme): B7 T0 — prove the left-dock focus migration before building

Answers design §12's five probes against a throwaway DockItem::tabs mount.
EOF
```

---

## Task 1: Rail icons, i18n keys and NOTICE

**Files:**
- Create: `crates/dat0-app/assets/icons/database.svg`, `plug.svg`, `sparkles.svg`
- Modify: `crates/dat0-app/src/assets.rs:67-100`
- Modify: `NOTICE.md:26-37`
- Modify: `crates/dat0-i18n/src/strings/en.json`
- Test: `crates/dat0-app/tests/icon_assets.rs` (existing; must stay green)

**Interfaces:**
- Consumes: nothing.
- Produces: `Dat0IconName::{Database, Plug, Sparkles}` and the i18n keys
  `rail.title`, `rail.show_catalog`, `rail.show_connections`, `rail.show_ai` —
  Task 4 renders them.

- [ ] **Step 1: Vendor the three SVGs**

Fetch **verbatim** from Lucide. Do **not** hand-author path data — a wrong path
renders a plausible-looking wrong icon that no test can catch:

```bash
cd crates/dat0-app/assets/icons
for n in database plug sparkles; do
  curl -fsSL "https://raw.githubusercontent.com/lucide-icons/lucide/main/icons/$n.svg" -o "$n.svg"
done
head -12 database.svg
```

Each must match the shape of the existing `layers.svg`: `viewBox="0 0 24 24"`,
`stroke="currentColor"`, `fill="none"`. `stroke="currentColor"` is what makes the
icon inherit theme colour for free (A5) — verify it is present in all three.

If the fetch is blocked in this environment, **stop and ask the owner** for the
three files rather than inventing path data.

- [ ] **Step 2: Extend `Dat0IconName`**

In `src/assets.rs`, add three variants and extend `ALL` (note the array type —
`&'static [T]` fails clippy):

```rust
pub enum Dat0IconName {
    Filter,
    Play,
    Layers,
    Bookmark,
    History,
    Database,
    Plug,
    Sparkles,
}

impl Dat0IconName {
    /// Every variant — the tests and the gallery iterate this so a new icon
    /// cannot be added without being covered and displayed.
    pub const ALL: &[Dat0IconName; 8] = &[
        Dat0IconName::Filter,
        Dat0IconName::Play,
        Dat0IconName::Layers,
        Dat0IconName::Bookmark,
        Dat0IconName::History,
        Dat0IconName::Database,
        Dat0IconName::Plug,
        Dat0IconName::Sparkles,
    ];
}
```

and in `impl IconNamed for Dat0IconName`:

```rust
            // B7: the activity rail's three items.
            Self::Database => "icons/database.svg",
            Self::Plug => "icons/plug.svg",
            Self::Sparkles => "icons/sparkles.svg",
```

Also update the doc comment on `Dat0Embedded` — it says "Five Lucide SVGs"; make
it eight.

- [ ] **Step 3: Run the icon tests, and prove they are non-vacuous**

```bash
cargo test -p dat0-app --test icon_assets
```

Expected: PASS. Then prove the new entries are actually exercised — temporarily
point `Self::Database` at `"icons/nope.svg"`:

```bash
cargo test -p dat0-app --test icon_assets 2>&1 | tail -20
```

Expected: `dat0_icons_resolve` and `payloads_are_svg` FAIL. Revert, then
**`touch src/assets.rs`** before re-running (A6's stale-binary trap), and confirm
green again.

- [ ] **Step 4: Update NOTICE.md**

`database` **is** Feather-derived — confirmed against the authoritative list
vendored at `crates/dat0-app/assets/icons/LICENSE-lucide:21`, which names
`database` but neither `plug` nor `sparkles`. Edit `NOTICE.md:29-35`:

```markdown
- **8 icons** are vendored directly into `crates/dat0-app/assets/icons/`:
  `funnel.svg`, `play.svg`, `layers.svg`, `bookmark.svg`, `clock.svg`,
  `database.svg`, `plug.svg`, `sparkles.svg`.

Lucide is dual-licensed. Most icons are ISC; icons derived from the Feather
project are MIT (Copyright (c) 2013-present Cole Bemis). dat0 ships icons under
both — `clock`, `database`, `x`, `check` and the `chevron-*` family are among the
Feather-derived set. The complete upstream license text covering both, including
the authoritative list of Feather-derived icons, is vendored verbatim at
`crates/dat0-app/assets/icons/LICENSE-lucide`.
```

Keep this section **above** the `<!-- BEGIN cargo-about generated -->` marker so
`scripts/notice-extract.sh` is unaffected.

- [ ] **Step 5: Add the four i18n keys**

In `crates/dat0-i18n/src/strings/en.json`, beside the existing `catalog.title` /
`connections.title` entries. **First confirm none exists** — a duplicate JSON key
is silently overwritten with no error (A5):

```bash
grep -n '"rail\.' crates/dat0-i18n/src/strings/en.json   # expect: no output
```

```json
  "rail.title": "Activity bar",
  "rail.show_catalog": "Show Catalog",
  "rail.show_connections": "Show Connections",
  "rail.show_ai": "Show AI",
```

The item labels deliberately do **not** reuse `catalog.title` etc. Two nodes with
the same role and label make `A11ySnapshot::query_by_role` panic on a duplicate
match (design §7.4).

- [ ] **Step 6: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test icon_assets
cargo test -p dat0-app --features a11y-capture,gallery --test gallery_smoke
```

All four green. Then:

```bash
git add crates/dat0-app/assets/icons crates/dat0-app/src/assets.rs NOTICE.md \
        crates/dat0-i18n/src/strings/en.json
git commit -s -m "feat(theme): B7 T1 — vendor rail icons, add rail i18n keys (UI redesign)"
```

---

## Task 2: The exclusivity invariant

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (new enum + 4 methods; 3 menu handlers; 3 test shims)
- Create: `crates/dat0-app/tests/left_dock.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `pub(crate) enum LeftPanel { Catalog, Connections, Ai }` with
    `LeftPanel::ALL: &[LeftPanel; 3]`
  - `WorkspaceShell::activate_left_panel(&mut self, target: LeftPanel, cx: &mut Context<Self>)`
  - `WorkspaceShell::left_panel_visible(&self, p: LeftPanel) -> bool`
  - `WorkspaceShell::open_left_panel(&self) -> Option<LeftPanel>`
  - private `set_left_panel_exclusive(&mut self, target: Option<LeftPanel>)`
  - Tasks 3-5 consume all of these.

This task changes **behaviour only** — no dock, no rail. After it, opening
Connections while Catalog is open closes Catalog. That is a real, visible,
independently reviewable change.

- [ ] **Step 1: Write the failing test**

Create `crates/dat0-app/tests/left_dock.rs` with the harness copied **verbatim**
from `tests/right_dock.rs:13-111`, then:

```rust
/// The B7 invariant: at most one left panel is visible at any time. Two visible
/// would make upstream paint a horizontal tab bar beside the rail
/// (tab_panel.rs:623-625) — two selectors for one choice.
fn open_count(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> usize {
    vcx.cx.update(|app| {
        let ws = shell.read(app);
        LeftPanel::ALL.iter().filter(|p| ws.left_panel_visible(**p)).count()
    })
}

#[gpui::test]
#[serial]
fn activating_a_left_panel_closes_the_others(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    for target in *LeftPanel::ALL {
        vcx.cx.update(|app| {
            shell.update(app, |ws, cx| ws.activate_left_panel(target, cx));
        });
        settle(vcx);
        assert_eq!(open_count(&shell, vcx), 1, "exactly one panel open after activating {target:?}");
        assert!(
            vcx.cx.update(|app| shell.read(app).left_panel_visible(target)),
            "the activated panel is the one that is open"
        );
    }
}

#[gpui::test]
#[serial]
fn activating_the_open_panel_collapses_everything(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);
    assert_eq!(open_count(&shell, vcx), 1);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);
    assert_eq!(open_count(&shell, vcx), 0, "activating the open panel collapses it");
    assert_eq!(vcx.cx.update(|app| shell.read(app).open_left_panel()), None);
}

/// The three a11y shims write left-panel bools directly, which would violate the
/// invariant a test at a time. They must route through the single writer.
#[gpui::test]
#[serial]
fn the_test_shims_respect_the_invariant(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(Vec::new());
            let _ = ws.open_connections_for_test();
        });
    });
    settle(vcx);

    assert_eq!(
        open_count(&shell, vcx),
        1,
        "two shims in a row must not leave two panels visible"
    );
    assert_eq!(
        vcx.cx.update(|app| shell.read(app).open_left_panel()),
        Some(LeftPanel::Connections),
        "the last shim called wins"
    );
}
```

Import `LeftPanel` alongside `WorkspaceShell`: `use dat0_app::window::{LeftPanel, WorkspaceShell};`

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock
```

Expected: FAIL to compile — `cannot find type LeftPanel`, `no method named
activate_left_panel`.

- [ ] **Step 3: Add the enum and the four methods**

In `src/window.rs`, near the `WorkspaceShell` definition:

```rust
/// B7: which left-dock panel is showing. The three shell bools remain the
/// storage; this names the choice they encode so every transition goes through
/// one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftPanel {
    Catalog,
    Connections,
    Ai,
}

impl LeftPanel {
    /// `&[T; N]`, not `&'static [T]` — the latter trips
    /// `clippy::redundant_static_lifetimes` under `-D warnings` (A5).
    pub const ALL: &[LeftPanel; 3] = &[LeftPanel::Catalog, LeftPanel::Connections, LeftPanel::Ai];
}
```

and in `impl WorkspaceShell`:

```rust
    /// B7: the ONLY writer of the three left-panel bools.
    ///
    /// Making it the only writer is what makes the at-most-one-visible invariant
    /// structural rather than a convention every call site has to remember —
    /// upstream paints a horizontal tab bar the moment two are visible
    /// (`tab_panel.rs:623-625`), which would put a second selector beside the
    /// activity rail.
    fn set_left_panel_exclusive(&mut self, target: Option<LeftPanel>) {
        self.catalog_panel_visible = target == Some(LeftPanel::Catalog);
        self.connections_panel_visible = target == Some(LeftPanel::Connections);
        self.ai_panel_visible = target == Some(LeftPanel::Ai);
    }

    pub(crate) fn left_panel_visible(&self, p: LeftPanel) -> bool {
        match p {
            LeftPanel::Catalog => self.catalog_panel_visible,
            LeftPanel::Connections => self.connections_panel_visible,
            LeftPanel::Ai => self.ai_panel_visible,
        }
    }

    /// The open panel, or `None` when the dock is collapsed.
    pub(crate) fn open_left_panel(&self) -> Option<LeftPanel> {
        LeftPanel::ALL
            .iter()
            .copied()
            .find(|p| self.left_panel_visible(*p))
    }

    /// B7: the user-facing left-panel transition. Activating the panel that is
    /// already open collapses the dock — the owner-chosen VSCode behaviour, and
    /// it falls out of the invariant rather than being a special case.
    ///
    /// The per-panel side effects that used to live in the individual toggle
    /// handlers move here so no entry point can lose them: Catalog refreshes so
    /// the dock always shows fresh tables, AI hydrates its draft from settings +
    /// keychain.
    pub(crate) fn activate_left_panel(&mut self, target: LeftPanel, cx: &mut gpui::Context<Self>) {
        let already_open = self.left_panel_visible(target);
        self.set_left_panel_exclusive((!already_open).then_some(target));
        if !already_open {
            match target {
                LeftPanel::Catalog => self.refresh_catalog(cx),
                LeftPanel::Ai => self.hydrate_ai_panel(),
                LeftPanel::Connections => {}
            }
        }
        // Only `catalog_panel_visible` is persisted (session v10); the other two
        // have always defaulted false at construction.
        self.persist_dock_ui();
        cx.notify();
    }
```

`LeftPanel` must be `pub` (not `pub(crate)`) because `tests/left_dock.rs` is an
integration test and names it.

- [ ] **Step 4: Route every existing writer through it**

Four production sites and three shims. Replace the inline flips:

`window.rs:7371-7377` (Connections menu action):

```rust
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::ConnectionsToggle, _window, cx| {
                    ws.activate_left_panel(LeftPanel::Connections, cx);
                },
            ))
```

`window.rs:7379-7388` (Catalog menu action) — `refresh_catalog` and
`persist_dock_ui` are now inside `activate_left_panel`, so they leave here:

```rust
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::CatalogToggle, _window, cx| {
                    ws.activate_left_panel(LeftPanel::Catalog, cx);
                },
            ))
```

`window.rs:5639-5645` (`toggle_ai_panel`, called by the AI menu action) — keep the
method name so its callers are untouched:

```rust
    pub(crate) fn toggle_ai_panel(&mut self, cx: &mut gpui::Context<Self>) {
        // B7: AI is one of three mutually-exclusive left panels now. The
        // hydrate-on-open this used to do lives in `activate_left_panel`.
        self.activate_left_panel(LeftPanel::Ai, cx);
    }
```

The three shims call the **private** writer, not `activate_left_panel` — they must
not trigger the refresh/hydrate side effects. `seed_catalog_tree_for_test` in
particular must keep bypassing `refresh_catalog`, whose off-thread `get_tables`
(`window.rs:2999`) would clobber the fakes it just seeded:

```rust
    pub fn seed_catalog_tree_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tree = crate::catalog::CatalogTree::build(&tables);
        // B7: via the single writer, so the at-most-one invariant holds for
        // tests too — but NOT via `activate_left_panel`, whose refresh would
        // clobber these fakes.
        self.set_left_panel_exclusive(Some(LeftPanel::Catalog));
    }

    pub fn open_connections_for_test(&mut self) -> &mut crate::connections::ConnectionManager {
        self.set_left_panel_exclusive(Some(LeftPanel::Connections));
        &mut self.connections
    }

    #[cfg(feature = "a11y-capture")]
    pub fn seed_ai_panel_for_test(&mut self, panel: crate::ai::panel::AiPanel) {
        self.ai_panel = panel;
        // NOT `activate_left_panel` — that would call `hydrate_ai_panel`, which
        // probes the OS keychain + settings.toml and is the hermeticity trap
        // this shim exists to avoid.
        self.set_left_panel_exclusive(Some(LeftPanel::Ai));
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock
```

Expected: 3 passed.

- [ ] **Step 6: Prove non-vacuity**

Temporarily make `set_left_panel_exclusive` additive (`if target == Some(..) {
self.catalog_panel_visible = true }` style, leaving the others alone) and confirm
`activating_a_left_panel_closes_the_others` goes RED. Revert, `touch
src/window.rs`, re-run green.

- [ ] **Step 7: Verify no suite regressed and commit**

The invariant changes behaviour for any existing test that opened two left panels:

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --features a11y-capture --test catalog_nav --test ai_nav
cargo test -p dat0-app --features a11y-capture --test keyboard_nav --test a11y_content
```

If a suite fails because it opened two panels, fix the **test** (it was relying on
a state the product no longer allows), and say so in the commit message.

```bash
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/left_dock.rs
git commit -s -m "feat(theme): B7 T2 — at-most-one left panel, single writer (UI redesign)"
```

---

## Task 3: Three thin panels and the extracted body methods

**Files:**
- Create: `crates/dat0-app/src/panels/catalog_panel.rs`, `connections_panel.rs`, `ai_dock_panel.rs`
- Modify: `crates/dat0-app/src/panels/mod.rs`
- Modify: `crates/dat0-app/src/window.rs` (extract three body methods)

**Interfaces:**
- Consumes: `LeftPanel`, `left_panel_visible` (Task 2).
- Produces:
  - `CatalogPanel::PANEL_NAME == "CatalogPanel"`, `ConnectionsPanel::PANEL_NAME ==
    "ConnectionsPanel"`, `AiDockPanel::PANEL_NAME == "AiDockPanel"`
  - `WorkspaceShell::render_catalog_body(&mut self, cx: &mut Context<Self>) -> AnyElement`
  - `WorkspaceShell::render_connections_body(&mut self, cx: &mut Context<Self>) -> AnyElement`
  - `WorkspaceShell::render_ai_body(&mut self, cx: &mut Context<Self>) -> AnyElement`
  - `catalog_visible()`, `connections_visible()`, `ai_visible()` getters
  - Task 4 mounts all three.

**Zero visual change in this task.** The body row still renders, but through the
extracted methods. That is the point: if anything moves on screen here, the
extraction was not verbatim.

- [ ] **Step 1: Extract the three body methods**

In `src/window.rs`, beside `render_inspector_body` (`:6494`). Each is the body-row
block moved verbatim, minus the `.w_64().border_r_1()` wrapper — sizing and
borders become the dock's job at Task 4. Each takes `&mut self` because
`hero_focus_handle` does, which is why these handles are currently hoisted at
`:7244` and `:7252`; minting them here keeps the map — and therefore **handle
identity** — on the shell, which is what keeps `catalog_nav` and `ai_nav`
meaningful across the move.

```rust
    /// B7: the Catalog panel's element tree, extracted from the body row so
    /// [`crate::panels::catalog_panel::CatalogPanel`] can call it.
    ///
    /// `&mut self` because `hero_focus_handle` needs it — the `catalog-tree`
    /// handle stays minted from the shell's `hero_focus` map, so the SAME
    /// `FocusHandle` instance lands on the same element after the move.
    pub(crate) fn render_catalog_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let catalog_fh = self.hero_focus_handle("catalog-tree", cx);
        crate::catalog::panel::render_catalog(
            &self.catalog_tree,
            &self.catalog_collapsed,
            self.catalog_active,
            &catalog_fh,
            cx,
        )
    }

    /// B7: the Connections panel's element tree, extracted from the body row.
    pub(crate) fn render_connections_body(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        crate::connections::panel::render_connections(&self.connections, cx)
    }

    /// B7: the AI dock's element tree, extracted from the body row.
    ///
    /// Registering all eight ids unconditionally is fine — `HeroHandles::get` is
    /// only invoked by whichever buttons actually render, and `ai-key-forget` is
    /// only looked up when `key_set`.
    pub(crate) fn render_ai_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let ai_handles = {
            let ids: [&'static str; 8] = [
                "ai-toggle-enabled",
                "ai-provider-cycle",
                "ai-key-set",
                "ai-key-forget",
                "ai-model-set",
                "ai-toggle-advanced",
                "ai-toggle-sample-rows",
                "ai-test-connection",
            ];
            let mut map = std::collections::HashMap::new();
            for id in ids {
                map.insert(id, self.hero_focus_handle(id, cx));
            }
            crate::empty_state::HeroHandles { map }
        };
        crate::ai::panel::render_ai_panel(&self.ai_panel, &ai_handles, cx)
    }

    /// B7: read by `CatalogPanel::visible`. A getter rather than a `pub(crate)`
    /// field keeps the direction of the dependency legible (B6).
    pub(crate) fn catalog_visible(&self) -> bool {
        self.catalog_panel_visible
    }
    pub(crate) fn connections_visible(&self) -> bool {
        self.connections_panel_visible
    }
    pub(crate) fn ai_visible(&self) -> bool {
        self.ai_panel_visible
    }
```

Then delete the now-duplicated hoists at `:7244` (`let catalog_fh = …`) and
`:7252-7267` (`let ai_handles = { … }`), and rewrite the three body-row blocks to
call the new methods:

```rust
                    .children(self.catalog_panel_visible.then(|| {
                        div().w_64().border_r_1().child(self.render_catalog_body(cx))
                    }))
                    .children(self.connections_panel_visible.then(|| {
                        div().w_64().border_r_1().child(self.render_connections_body(cx))
                    }))
                    .children(self.ai_panel_visible.then(|| {
                        div().w_64().border_r_1().child(self.render_ai_body(cx))
                    }))
```

⚠ `.then(|| …)` closures capture `&mut self` here. If the borrow checker rejects
this, hoist each into a `let` **before** the body row is built:

```rust
        let catalog_block = self
            .catalog_panel_visible
            .then(|| self.render_catalog_body(cx));
```

and use `.children(catalog_block)`. Prefer whichever compiles; do not restructure
further.

- [ ] **Step 2: Verify nothing moved**

```bash
cargo test -p dat0-app --features a11y-capture --test catalog_nav --test ai_nav
cargo test -p dat0-app --features a11y-capture --test a11y_spike
```

Expected: all PASS with **no** change to `a11y_spike`'s count — a verbatim
extraction adds no capture node.

- [ ] **Step 3: Commit the extraction**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs
git commit -s -m "refactor(theme): B7 T3a — extract the three left-panel body methods (UI redesign)"
```

- [ ] **Step 4: Write the three panels**

`src/panels/catalog_panel.rs` — the title is **plain text with no `a11y_label`**,
unlike B6's Inspector, because the catalog tree's root already carries
`.a11y("catalog-tree", Button, t("catalog.title"))` and a second node named
"Catalog" makes `query_by_role` panic:

```rust
//! B7: the left dock's Catalog panel — a thin wrapper over the shell's catalog
//! body, following B5's [`GridPanel`](super::grid_panel::GridPanel) template.
//!
//! The panel owns NO catalog state. [`crate::window::WorkspaceShell`] keeps
//! `catalog_tree`, `catalog_collapsed`, `catalog_active` and `catalog_nav_key`;
//! this panel's `render` delegates straight back into
//! [`WorkspaceShell::render_catalog_body`]. The master plan's B7 row proposed
//! moving that state here; it was written before B5 established this template,
//! and the move would touch session persistence and three a11y shims for no
//! user-visible gain — see design §3.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, WeakEntity, Window, div,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};

use crate::window::WorkspaceShell;

pub struct CatalogPanel {
    shell: WeakEntity<WorkspaceShell>,
}

impl CatalogPanel {
    /// The serialization key `DockArea::load` resolves through the global
    /// `PanelRegistry` (B9). **Frozen from B7 onward.**
    pub const PANEL_NAME: &str = "CatalogPanel";

    pub fn new(shell: WeakEntity<WorkspaceShell>) -> Self {
        Self { shell }
    }
}

impl EventEmitter<PanelEvent> for CatalogPanel {}

impl Focusable for CatalogPanel {
    /// The SHELL's root handle — a private handle is tracked by no element, so
    /// focusing it silently swallows focus rather than moving it (B5).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).grid_focus_handle())
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for CatalogPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    /// PLAIN text, deliberately without `a11y_label` — unlike the other panels.
    /// `catalog/panel.rs` already names its root
    /// `.a11y("catalog-tree", Button, t("catalog.title"))`, and a second node
    /// with that name would make `A11ySnapshot::query_by_role` panic on a
    /// duplicate match (`tests/support/mod.rs:139`).
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(SharedString::from(dat0_i18n::t("catalog.title")))
    }

    /// v1 dock scope is resize + collapse only.
    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// The shell's bool is the single source of truth (design §5).
    fn visible(&self, cx: &App) -> bool {
        self.shell
            .upgrade()
            .map(|ws| ws.read(cx).catalog_visible())
            .unwrap_or(false)
    }
}

impl Render for CatalogPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            // Reached only through the B9-placeholder registry builder.
            return div().into_any_element();
        };
        shell.update(cx, |ws, cx| ws.render_catalog_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B9's serialization key — rename ratchet for a slice not yet written.
    #[test]
    fn panel_name_is_frozen() {
        let panel = CatalogPanel::new(gpui::WeakEntity::new_invalid());
        assert_eq!(panel.panel_name(), "CatalogPanel");
        assert_eq!(CatalogPanel::PANEL_NAME, "CatalogPanel");
    }

    #[test]
    fn shell_less_panel_has_no_shell_to_read() {
        let panel = CatalogPanel::new(gpui::WeakEntity::new_invalid());
        assert!(panel.shell.upgrade().is_none());
    }
}
```

`src/panels/connections_panel.rs` — identical, with these differences: doc header
names Connections and says the shell keeps `connections: ConnectionManager`;
`PANEL_NAME = "ConnectionsPanel"`; `visible` reads `connections_visible()`;
`render` calls `render_connections_body`; and `title()` **does** carry a label,
because nothing else names this panel:

```rust
    /// Carries an `a11y_label`: unlike Catalog, nothing else in this panel names
    /// it, so without this the panel is anonymous to a screen reader.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = dat0_i18n::t("connections.title");
        div()
            .a11y_label(AccessRole::Label, title.clone())
            .child(SharedString::from(title))
    }
```

with `use crate::a11y::{A11yExt as _, AccessRole};` added.

`src/panels/ai_dock_panel.rs` — same again: `PANEL_NAME = "AiDockPanel"`,
`ai_visible()`, `render_ai_body`, and a labelled `title()` reading `ai.title`
("AI Providers").

- [ ] **Step 5: Register all three**

In `src/panels/mod.rs`, add the modules and three `register_panel` calls in the
same shape as the existing ones:

```rust
pub mod ai_dock_panel;
pub mod catalog_panel;
pub mod charts_panel;
pub mod connections_panel;
pub mod grid_panel;
pub mod inspector_panel;
```

```rust
    gpui_component::dock::register_panel(
        cx,
        catalog_panel::CatalogPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| catalog_panel::CatalogPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );

    gpui_component::dock::register_panel(
        cx,
        connections_panel::ConnectionsPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| {
                connections_panel::ConnectionsPanel::new(gpui::WeakEntity::new_invalid())
            }))
        },
    );

    gpui_component::dock::register_panel(
        cx,
        ai_dock_panel::AiDockPanel::PANEL_NAME,
        |_dock_area, _state, _info, _window, cx| {
            Box::new(cx.new(|_| ai_dock_panel::AiDockPanel::new(gpui::WeakEntity::new_invalid())))
        },
    );
```

Also update the module doc's "B6-B8 add the inspector, charts, catalog…" line to
say the catalog/connections/AI panels landed at B7.

- [ ] **Step 6: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --lib
cargo test -p dat0-app --features a11y-capture --test left_dock --test catalog_nav
```

The three `panel_name_is_frozen` unit tests are in `--lib`. Expected: all green,
still no visual change (the panels exist but nothing mounts them yet).

```bash
git add crates/dat0-app/src/panels crates/dat0-app/src/window.rs
git commit -s -m "feat(theme): B7 T3b — CatalogPanel, ConnectionsPanel, AiDockPanel (UI redesign)"
```

---

## Task 4: Mount the left dock

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (mount, `sync_left_dock`, body row, new fields)
- Modify: `crates/dat0-app/src/{catalog,connections,ai}/panel.rs` (drop body titles)
- Modify: `crates/dat0-app/tests/left_dock.rs` (dock-level tests)

**Interfaces:**
- Consumes: everything from Tasks 2 and 3.
- Produces:
  - `WorkspaceShell::left_dock_open_for_test(&self, cx: &App) -> bool`
  - const `LEFT_DOCK_WIDTH: f32 = 384.0`
  - Task 5 needs none of this; Task 6 asserts on it.

This is the switchover — the body row stops rendering the panels and the dock
starts.

- [ ] **Step 1: Write the failing tests**

Append to `tests/left_dock.rs`:

```rust
fn left_dock_open(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> bool {
    vcx.cx.update(|app| shell.read(app).left_dock_open_for_test(app))
}

#[gpui::test]
#[serial]
fn left_dock_is_closed_when_every_panel_is_hidden(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    assert!(vcx.cx.update(|app| shell.read(app).dock_mounted_for_test()));
    assert!(!left_dock_open(&shell, vcx), "a fresh workspace shows no left panel");
}

#[gpui::test]
#[serial]
fn activating_a_panel_opens_the_dock_and_titles_it(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Connections, cx));
    });
    settle(vcx);

    assert!(left_dock_open(&shell, vcx), "sync_left_dock must open the dock");
    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.count_label(&dat0_app::dat0_i18n::t("connections.title")),
        1,
        "the dock's title bar names the panel exactly once"
    );
}

#[gpui::test]
#[serial]
fn collapsing_closes_the_dock_again(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);
    assert!(left_dock_open(&shell, vcx));

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);
    assert!(!left_dock_open(&shell, vcx), "collapsing must close the dock");
}

/// Design §7.4's collision, asserted rather than assumed: `query_by_role` panics
/// on a duplicate match, so a panel whose title duplicates a body node would
/// take the whole suite down.
#[gpui::test]
#[serial]
fn each_panel_name_resolves_without_a_duplicate(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    for (panel, key) in [
        (LeftPanel::Catalog, "catalog.title"),
        (LeftPanel::Connections, "connections.title"),
        (LeftPanel::Ai, "ai.title"),
    ] {
        vcx.cx.update(|app| {
            shell.update(app, |ws, cx| ws.activate_left_panel(panel, cx));
        });
        settle(vcx);
        let snap = A11ySnapshot::capture(vcx);
        assert_eq!(
            snap.count_label(&dat0_app::dat0_i18n::t(key)),
            1,
            "{key} must name exactly one node while {panel:?} is open"
        );
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock
```

Expected: FAIL — `no method named left_dock_open_for_test`.

- [ ] **Step 3: Add the fields and the const**

In `src/window.rs`, beside `right_dock_state` (`:2147`) and the existing dock
width consts:

```rust
    /// B7: the (catalog, connections, ai) visibility triple the left dock was
    /// last reconciled to, so `sync_left_dock` does work only on a real change.
    left_dock_state: (bool, bool, bool),
    catalog_panel: Option<Entity<crate::panels::catalog_panel::CatalogPanel>>,
    connections_panel: Option<Entity<crate::panels::connections_panel::ConnectionsPanel>>,
    ai_dock_panel: Option<Entity<crate::panels::ai_dock_panel::AiDockPanel>>,
```

with `left_dock_state: (false, false, false)` and three `None`s in the
constructor, and:

```rust
/// B7: the left dock's fixed width. Fixed because `set_left_dock` may be called
/// only once (it leaks subscriptions), and `DockArea` exposes no size setter —
/// see the mount site. 384 rather than the old 256 per-panel width: with one
/// panel showing at a time the sum has no meaning, and the catalog tree reads
/// better with the extra room.
const LEFT_DOCK_WIDTH: f32 = 384.0;
```

- [ ] **Step 4: Mount the dock**

Inside the existing `if self.dock_area.is_none()` block (`:6886`), after the right
dock's `set_right_dock` call and before the field assignments:

```rust
            // B7: the left dock — ONE `DockItem::tabs` holding all three panels,
            // not a split. `TabPanel::active_panel` falls back to the first
            // VISIBLE panel when the one at `active_ix` is hidden
            // (`tab_panel.rs:192-204`), so flipping the shell's bools is enough
            // to change which panel renders and `active_ix` can stay 0 forever.
            //
            // Tabs rather than split for the focus migration: `.tab_group()` is
            // applied once per `TabPanel` (`tab_panel.rs:1192`), so this puts ONE
            // tab group in the tree where a three-way split would put three — in
            // the slice that moves nine live focus handles at once.
            let catalog = cx.new(|_| {
                crate::panels::catalog_panel::CatalogPanel::new(weak_shell.clone())
            });
            let connections = cx.new(|_| {
                crate::panels::connections_panel::ConnectionsPanel::new(weak_shell.clone())
            });
            let ai_dock =
                cx.new(|_| crate::panels::ai_dock_panel::AiDockPanel::new(weak_shell.clone()));
            let left = gpui_component::dock::DockItem::tabs(
                vec![
                    Arc::new(catalog.clone()),
                    Arc::new(connections.clone()),
                    Arc::new(ai_dock.clone()),
                ],
                &weak_dock,
                window,
                cx,
            );

            // ⚠ `set_left_dock` leaks exactly like `set_right_dock`: it runs
            // `subscribe_item`, which pushes onto `_subscriptions` and recurses
            // over the item tree (`dock/mod.rs:955-963`), and nothing removes
            // them. Called EXACTLY ONCE. `sync_left_dock` only ever toggles.
            let want_left = (
                self.catalog_panel_visible,
                self.connections_panel_visible,
                self.ai_panel_visible,
            );
            let left_open = want_left.0 || want_left.1 || want_left.2;
            dock.update(cx, |dock, cx| {
                dock.set_left_dock(
                    left,
                    Some(gpui::px(LEFT_DOCK_WIDTH)),
                    left_open,
                    window,
                    cx,
                );
            });
            self.left_dock_state = want_left;

            self.catalog_panel = Some(catalog);
            self.connections_panel = Some(connections);
            self.ai_dock_panel = Some(ai_dock);
```

- [ ] **Step 5: Add `sync_left_dock` and the test accessor**

Beside `sync_right_dock` (`:6522`):

```rust
    /// B7: reconcile the left dock with the three visibility bools, which are
    /// the single source of truth. Mirrors `sync_right_dock` exactly: runs in
    /// `render` because `toggle_dock` needs a `&mut Window`, guarded by a state
    /// tuple so it acts only on a real change, and never re-runs `set_left_dock`.
    ///
    /// WHICH panel shows is `Panel::visible` (read straight off these bools), so
    /// this only decides whether the dock as a whole is open.
    fn sync_left_dock(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let want = (
            self.catalog_panel_visible,
            self.connections_panel_visible,
            self.ai_panel_visible,
        );
        if want == self.left_dock_state {
            return;
        }
        self.left_dock_state = want;
        let Some(dock) = self.dock_area.clone() else {
            return;
        };
        let want_open = want.0 || want.1 || want.2;
        dock.update(cx, |dock, cx| {
            if dock.is_dock_open(gpui_component::dock::DockPlacement::Left, cx) != want_open {
                dock.toggle_dock(gpui_component::dock::DockPlacement::Left, window, cx);
            }
        });
    }
```

Call it beside `self.sync_right_dock(window, cx);` at `:6948`:

```rust
        self.sync_left_dock(window, cx);
        self.sync_right_dock(window, cx);
```

And beside `right_dock_open_for_test` (`:7513`):

```rust
    /// B7: the DOCK's own open flag, deliberately not the bools the test wrote —
    /// re-reading those would prove only that assignment works, not that
    /// `sync_left_dock` ran.
    pub fn left_dock_open_for_test(&self, cx: &gpui::App) -> bool {
        self.dock_area
            .as_ref()
            .map(|d| {
                d.read(cx)
                    .is_dock_open(gpui_component::dock::DockPlacement::Left, cx)
            })
            .unwrap_or(false)
    }
```

- [ ] **Step 6: Delete the three body-row blocks**

Remove all three `.children(self.*_panel_visible.then(|| …))` blocks from the body
row (and the `let` hoists if Step 1 of Task 3 introduced them), leaving the
comment updated:

```rust
            // Body row. B6 moved the Inspector and Charts right docks into the
            // DockArea; B7 moved the Catalog, Connections and AI left docks the
            // same way, so this row is now just the activity rail plus the dock.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .child(div().flex_1().children(dock_el)),
            )
```

(The rail joins this row at Task 5.)

- [ ] **Step 7: Move the three body titles into the title bars**

Each panel currently draws its own title, which would print twice under the dock's
30 px title bar (B6's Inspector lesson). Delete one line from each:

- `src/catalog/panel.rs:73` — drop
  `.child(div().child(SharedString::from(dat0_i18n::t("catalog.title"))))`.
  **Keep** the `.a11y("catalog-tree", …)` on the same root; that is the tree's
  accessible name and `catalog_nav` depends on it.
- `src/connections/panel.rs:199` — drop
  `.child(div().child(SharedString::from(dat0_i18n::t("connections.title"))))`.
- `src/ai/panel.rs:234` — drop
  `.child(div().child(SharedString::from(dat0_i18n::t("ai.title"))))`.

- [ ] **Step 8: Run the tests**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock
```

Expected: 7 passed (3 from Task 2, 4 new).

- [ ] **Step 9: Run every gate suite**

This is the switchover, so the focus-migration gates run in full:

```bash
cargo test -p dat0-app --features a11y-capture \
  --test catalog_nav --test ai_nav --test keyboard_nav \
  --test a11y_content --test sql_console_transient_nav \
  --test right_dock --test dock_chrome_spike --test a11y_spike
```

Expected: all green. `a11y_spike`'s count should be **unchanged** — the dock is
closed on the hero, so no left-panel node exists there.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
git add crates/dat0-app/src crates/dat0-app/tests/left_dock.rs
git commit -s -F - <<'EOF'
feat(theme): B7 T4 — mount the left dock (UI redesign)

The three panels move out of the body row into one DockItem::tabs, with
set_left_dock called exactly once (it leaks subscriptions) and
sync_left_dock toggling open/closed off the visibility bools.
EOF
```

---

## Task 5: The activity rail

**Files:**
- Create: `crates/dat0-app/src/view/activity_rail.rs`
- Modify: `crates/dat0-app/src/view/mod.rs`
- Modify: `crates/dat0-app/src/window.rs` (rail state, three handlers, body row)
- Modify: `crates/dat0-app/tests/a11y_spike.rs`
- Modify: `crates/dat0-app/src/view/sql_console.rs` (two stale comments)

**Interfaces:**
- Consumes: `LeftPanel`, `activate_left_panel`, `open_left_panel` (Task 2);
  `Dat0IconName::{Database, Plug, Sparkles}` and the four `rail.*` keys (Task 1).
- Produces:
  - `view::activity_rail::{RAIL_WIDTH, ITEMS, RailItem, render_rail}`
  - `WorkspaceShell::{rail_move_cursor, rail_activate_cursor, rail_click, rail_cursor_for_test}`

- [ ] **Step 1: Write the rail**

```rust
//! B7: the activity rail — a 48px vertical icon strip that selects which left
//! panel is showing, modelled on VSCode's activity bar.
//!
//! It is a SIBLING of the `DockArea`, not a dock panel. That is what keeps it
//! visible when the dock is collapsed — the point of the model — and it keeps
//! the rail out of the dock's `.tab_group()` entirely.
//!
//! Lives in `src/view/` with every other rendered shell surface; B3 recorded
//! this after the master plan guessed a top-level module.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div, px,
};
use gpui_component::{ActiveTheme as _, Icon, Tooltip};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::assets::Dat0IconName;
use crate::theme::tokens::Dat0Theme as _;
use crate::window::{LeftPanel, WorkspaceShell};

/// 48px, VSCode's activity-bar width.
pub(crate) const RAIL_WIDTH: f32 = 48.0;

pub(crate) struct RailItem {
    pub id: &'static str,
    pub panel: LeftPanel,
    pub icon: Dat0IconName,
    /// Names the ACTION, not the panel — a rail item named "Catalog" would
    /// collide with the catalog tree's own accessible name and make
    /// `A11ySnapshot::query_by_role` panic on a duplicate match (design §7.4).
    pub label_key: &'static str,
}

/// Top-to-bottom order. The index into this array is the keyboard cursor.
pub(crate) const ITEMS: &[RailItem; 3] = &[
    RailItem {
        id: "rail-catalog",
        panel: LeftPanel::Catalog,
        icon: Dat0IconName::Database,
        label_key: "rail.show_catalog",
    },
    RailItem {
        id: "rail-connections",
        panel: LeftPanel::Connections,
        icon: Dat0IconName::Plug,
        label_key: "rail.show_connections",
    },
    RailItem {
        id: "rail-ai",
        panel: LeftPanel::Ai,
        icon: Dat0IconName::Sparkles,
        label_key: "rail.show_ai",
    },
];

/// Render the rail. `cursor` is the keyboard cursor; `open` is which panel is
/// actually showing. They are INDEPENDENT — the cursor exists even when the dock
/// is collapsed and nothing is open, the same two-state model as the catalog
/// tree's active row versus its selection.
pub(crate) fn render_rail(
    cursor: usize,
    open: Option<LeftPanel>,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let ring = cx.theme().d0().focus_ring;

    // ↑/↓: a SECOND on_key_down chained after focus_stop's own — gpui pushes
    // key-down listeners and both fire (the catalog tree's proven idiom).
    let arrows = cx.listener(|ws, ev: &gpui::KeyDownEvent, _window, cx| {
        match ev.keystroke.key.as_str() {
            "up" => ws.rail_move_cursor(-1, cx),
            "down" => ws.rail_move_cursor(1, cx),
            _ => {}
        }
    });
    // Enter/Space — focus_stop routes only those two here.
    let activate = cx.listener(|ws, _ev: &gpui::KeyDownEvent, _window, cx| {
        ws.rail_activate_cursor(cx);
    });

    let mut root = div()
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .w(px(RAIL_WIDTH))
        .h_full()
        .border_r_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .focus_stop("activity-rail", fh, 0, ring, activate)
        .on_key_down(arrows)
        .a11y(
            "activity-rail",
            AccessRole::Button,
            dat0_i18n::t("rail.title"),
        );

    for (i, item) in ITEMS.iter().enumerate() {
        root = root.child(rail_item(i, item, cursor == i, open == Some(item.panel), cx));
    }
    root.into_any_element()
}

fn rail_item(
    index: usize,
    item: &'static RailItem,
    is_cursor: bool,
    is_open: bool,
    cx: &mut Context<WorkspaceShell>,
) -> impl IntoElement {
    let label = dat0_i18n::t(item.label_key);
    let ring = cx.theme().d0().focus_ring;
    let accent = cx.theme().primary;
    let open_bg = cx.theme().secondary;
    let tooltip_label = label.clone();

    div()
        .id(item.id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(40.))
        .rounded(cx.theme().radius)
        .cursor_pointer()
        // The open panel gets a leading accent bar plus a raised background;
        // the keyboard cursor gets the focus ring. Two distinct states, two
        // distinct affordances — see the owed human glance.
        .when(is_open, |this| this.bg(open_bg).border_l_2().border_color(accent))
        .when(is_cursor, |this| this.border_1().border_color(ring))
        .a11y(item.id, AccessRole::Button, label)
        .tooltip(move |window, app| Tooltip::new(tooltip_label.clone()).build(window, app))
        .child(Icon::new(item.icon))
        .on_click(cx.listener(move |ws, _ev, _window, cx| ws.rail_click(index, cx)))
}
```

⚠ Two things to check as you compile: `.when` needs
`gpui::prelude::FluentBuilder`, and `.on_click` needs `.id()` first to get a
`Stateful<Div>` (both already in the imports above). If `.border_l_2()` and
`.border_1()` conflict when an item is both open and the cursor, keep both — the
later call wins on that edge and it is a legitimate visual to judge at the glance.

Add `pub mod activity_rail;` to `src/view/mod.rs`.

- [ ] **Step 2: Add the rail state and handlers**

In `src/window.rs`, a new field beside `left_dock_state`:

```rust
    /// B7: the activity rail's KEYBOARD cursor — an index into
    /// `view::activity_rail::ITEMS`. Independent of which panel is open: the
    /// cursor still exists when the dock is collapsed.
    rail_cursor: usize,
```

initialised `rail_cursor: 0`, and in `impl WorkspaceShell`:

```rust
    /// B7: move the rail's keyboard cursor. Clamps rather than wraps, matching
    /// the catalog tree.
    pub(crate) fn rail_move_cursor(&mut self, delta: isize, cx: &mut gpui::Context<Self>) {
        let len = crate::view::activity_rail::ITEMS.len() as isize;
        let next = (self.rail_cursor as isize + delta).clamp(0, len - 1);
        self.rail_cursor = next as usize;
        cx.notify();
    }

    /// B7: activate the panel under the cursor. Enter on the open panel
    /// collapses the dock, matching the mouse.
    pub(crate) fn rail_activate_cursor(&mut self, cx: &mut gpui::Context<Self>) {
        let target = crate::view::activity_rail::ITEMS[self.rail_cursor].panel;
        self.activate_left_panel(target, cx);
    }

    /// B7: a click both moves the cursor and activates, so the two never drift
    /// after a mouse interaction.
    pub(crate) fn rail_click(&mut self, index: usize, cx: &mut gpui::Context<Self>) {
        self.rail_cursor = index.min(crate::view::activity_rail::ITEMS.len() - 1);
        self.rail_activate_cursor(cx);
    }

    #[cfg(feature = "a11y-capture")]
    pub fn rail_cursor_for_test(&self) -> usize {
        self.rail_cursor
    }
```

- [ ] **Step 3: Mount the rail in the body row**

```rust
        // B7: the activity rail's focus handle, minted from the shell's
        // `hero_focus` map like every other persistent stop.
        let rail_fh = self.hero_focus_handle("activity-rail", cx);
        let rail_cursor = self.rail_cursor;
        let rail_open = self.open_left_panel();
        let rail = crate::view::activity_rail::render_rail(rail_cursor, rail_open, &rail_fh, cx);
```

built before the body row, then:

```rust
                    .child(rail)
                    .child(div().flex_1().children(dock_el)),
```

- [ ] **Step 4: Update `a11y_spike`'s exact count**

The rail is always visible, including on the hero, and contributes **four**
click-ids: the container plus three items (`.a11y` records a click-id;
`a11y_label` does not). Use the number T0/P4 measured — expected 12. In
`tests/a11y_spike.rs:118`:

```rust
    assert_eq!(
        snap.click_ids.len(),
        12,
        // UI-redesign B7 RECOUNT — 8 → 12. The activity rail is always visible,
        // including on the hero, and contributes exactly four `.a11y` sites:
        // `activity-rail` (the listbox container) plus `rail-catalog`,
        // `rail-connections` and `rail-ai`. The left dock itself adds nothing
        // here: it is closed on a fresh workspace.
        //
        // This stays an EXACT count. A `>=` would destroy what this assertion is
        // for — it is a frame-bracket double-render proof, not a content check.
        "..."
    );
```

- [ ] **Step 5: Correct the two stale tooltip comments**

`src/view/sql_console.rs:703-705` and `:861-864` both claim no `.tooltip()` helper
exists at this gpui-component rev. Now that the rail uses one, correct them:

```rust
            // A tooltip IS available at this rev — gpui core exposes
            // `.tooltip()` (`div.rs:1161`) and gpui-component ships a `Tooltip`
            // view (`tooltip.rs:15`); B7's activity rail uses both. These
            // buttons simply have not been given one yet.
```

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --features a11y-capture --test a11y_spike --test left_dock
cargo test -p dat0-app --features a11y-capture --test keyboard_nav --test catalog_nav
cargo test -p dat0-app --test style_lint
```

`style_lint` must still report the ratchet at `[("window.rs", 1)]` — the rail uses
only `cx.theme()` colours.

- [ ] **Step 7: Commit**

```bash
git add crates/dat0-app/src crates/dat0-app/tests/a11y_spike.rs
git commit -s -m "feat(theme): B7 T5 — the activity rail (UI redesign)"
```

---

## Task 6: Rail keyboard suite and spike disposal

**Files:**
- Modify: `crates/dat0-app/tests/left_dock.rs`
- Delete: `crates/dat0-app/tests/left_dock_spike.rs`

**Interfaces:**
- Consumes: everything.
- Produces: the finished `left_dock` suite.

- [ ] **Step 1: Write the rail keyboard tests**

Append to `tests/left_dock.rs`. **Drive the keymap** — `simulate_keystrokes`, never
`dispatch_action`; the latter bypasses the keymap, so a green test can hide a dead
production key path (the lesson that let a broken Escape ladder ship past five
reviews).

```rust
fn rail_cursor(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) -> usize {
    vcx.cx.update(|app| shell.read(app).rail_cursor_for_test())
}

/// Focus the rail by Tab-walking to it. A fresh window has nothing focused, and
/// with nothing focused the dispatch path is the window root alone — Tab is
/// completely inert (B1) — so click into the shell first.
fn focus_the_rail(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext) {
    vcx.simulate_click(
        gpui::Point::new(gpui::px(600.), gpui::px(400.)),
        gpui::Modifiers::none(),
    );
    settle(vcx);
    // `focused_label()` borrows the snapshot and yields `Option<&str>`, so bind
    // the snapshot and the wanted String rather than comparing temporaries.
    let want = dat0_app::dat0_i18n::t("rail.title");
    for _ in 0..24 {
        vcx.simulate_keystrokes("tab");
        settle(vcx);
        let snap = A11ySnapshot::capture(vcx);
        if snap.focused_label() == Some(want.as_str()) {
            return;
        }
    }
    let _ = shell;
    panic!("the activity rail was never reached by Tab");
}

#[gpui::test]
#[serial]
fn tab_reaches_the_rail(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    focus_the_rail(&shell, vcx);
}

#[gpui::test]
#[serial]
fn arrows_move_the_cursor_and_clamp(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    focus_the_rail(&shell, vcx);
    assert_eq!(rail_cursor(&shell, vcx), 0);

    vcx.simulate_keystrokes("down");
    settle(vcx);
    assert_eq!(rail_cursor(&shell, vcx), 1);

    vcx.simulate_keystrokes("down down");
    settle(vcx);
    assert_eq!(rail_cursor(&shell, vcx), 2, "clamps at the last item, never wraps");

    vcx.simulate_keystrokes("up up up");
    settle(vcx);
    assert_eq!(rail_cursor(&shell, vcx), 0, "clamps at the first item");
}

#[gpui::test]
#[serial]
fn enter_activates_the_panel_under_the_cursor(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    focus_the_rail(&shell, vcx);

    vcx.simulate_keystrokes("down enter");
    settle(vcx);
    assert_eq!(
        vcx.cx.update(|app| shell.read(app).open_left_panel()),
        Some(LeftPanel::Connections),
        "Enter on cursor index 1 opens Connections"
    );
    assert!(left_dock_open(&shell, vcx));

    // Enter again on the same item collapses, matching the mouse.
    vcx.simulate_keystrokes("enter");
    settle(vcx);
    assert_eq!(vcx.cx.update(|app| shell.read(app).open_left_panel()), None);
    assert!(!left_dock_open(&shell, vcx));
}

/// R7: collapsing while focus is inside the panel must not orphan focus — the
/// `ai-key-forget` self-removing-button lesson, where activating a control
/// unmounted the element tracking its own handle and the keyboard user landed
/// nowhere.
#[gpui::test]
#[serial]
fn collapsing_from_inside_the_panel_leaves_focus_somewhere_live(cx: &mut TestAppContext) {
    let (shell, vcx) = boot(cx);
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| ws.activate_left_panel(LeftPanel::Catalog, cx));
    });
    settle(vcx);

    // Tab must still move focus somewhere — an orphaned focus makes it inert.
    vcx.simulate_click(
        gpui::Point::new(gpui::px(600.), gpui::px(400.)),
        gpui::Modifiers::none(),
    );
    settle(vcx);
    vcx.simulate_keystrokes("tab");
    settle(vcx);
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.focused_label().is_some(),
        "focus was orphaned by the collapse"
    );
}
```

- [ ] **Step 2: Run**

```bash
cargo test -p dat0-app --features a11y-capture --test left_dock
```

Expected: 11 passed.

- [ ] **Step 3: Prove non-vacuity on the two sharpest tests**

Make `rail_move_cursor` a no-op → `arrows_move_the_cursor_and_clamp` must go RED.
Make `rail_activate_cursor` always target `LeftPanel::Catalog` →
`enter_activates_the_panel_under_the_cursor` must go RED. Revert each, `touch
src/window.rs`, confirm green.

- [ ] **Step 4: Dispose of the spike**

Everything the spike proved is now covered by real tests (P1 by
`focus_the_rail` + `catalog_nav`, P2 by `ai_nav`, P3 by
`each_panel_name_resolves_without_a_duplicate`, P4 by `a11y_spike`, P5 by the rail
itself rendering). Delete it — a throwaway kept past its purpose rots:

```bash
git rm crates/dat0-app/tests/left_dock_spike.rs
```

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests
git commit -s -m "test(theme): B7 T6 — rail keyboard suite, retire the T0 spike (UI redesign)"
```

---

## Task 7: Whole-branch gate and as-built

**Files:**
- Modify: the design doc (as-built section)

- [ ] **Step 1: Full local gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app > /tmp/b7-plain.txt 2>&1; grep -c "test result: ok" /tmp/b7-plain.txt
cargo test -p dat0-app --features a11y-capture > /tmp/b7-a11y.txt 2>&1; grep -c "test result: ok" /tmp/b7-a11y.txt
cargo test -p dat0-app --features a11y-capture,gallery > /tmp/b7-gal.txt 2>&1; grep -c "test result: ok" /tmp/b7-gal.txt
grep -c "test result: FAILED" /tmp/b7-*.txt
```

Expected **115** binaries per combo (114 as of B6, +1 for `left_dock`), zero
failures. ⚠ Do **not** pipe cargo into `head` — it SIGPIPEs cargo mid-write and
truncates the count (A6).

- [ ] **Step 2: Ratchet and diff checks**

```bash
cargo test -p dat0-app --test style_lint
git diff --stat main -- crates/dat0-app/src/grid crates/dat0-app/src/session
```

`style_lint` must report `[("window.rs", 1)]` unchanged; the diff must be empty.

- [ ] **Step 3: Build and BOOT the binary**

This is not optional — B5's tour regression was found only this way, by a WARN
line that no test could see. A silent success logs nothing, so "no line on main, a
WARN on the branch" is the entire signal.

```bash
cargo build -p dat0-app --bin dat0
DAT0_CONFIG_DIR=/tmp/dat0-b7-boot ./target/debug/dat0 2>&1 | tee /tmp/b7-boot.log
# then, for the baseline:
git stash && git checkout main && cargo build -p dat0-app --bin dat0
DAT0_CONFIG_DIR=/tmp/dat0-main-boot ./target/debug/dat0 2>&1 | tee /tmp/main-boot.log
git checkout - && git stash pop
diff /tmp/main-boot.log /tmp/b7-boot.log
```

Any new WARN or ERROR on the branch side is a finding, not noise. Click each rail
item, confirm the panels appear and the dock collapses on a second click.

- [ ] **Step 4: Write the as-built section**

Append §15 "As-built" to the design doc: what the tree contradicted, the actual
`a11y_spike` number, any deviation from this plan and why. Every slice since A5
has found at least one — record it rather than smoothing it over.

- [ ] **Step 5: Commit and push**

```bash
git add docs/plans/2026-08-03-dat0-ui-redesign-b7-left-dock-design.md
git commit -s -m "docs(theme): B7 as-built (UI redesign)"
git push -u origin feat/ui-redesign-b7-left-dock
```

- [ ] **Step 6: Open the PR and watch both platforms**

```bash
gh pr create --title "feat(theme): B7 — left dock + activity rail (UI redesign)" --body-file <(cat <<'EOF'
Slice B7 of the UI redesign. Moves Catalog, Connections and AI into the
DockArea's left dock as one DockItem::tabs, and adds a VSCode-style
activity rail that selects among them.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)
gh pr checks --watch
```

Poll `gh pr checks`, not `gh run watch`. On merge, use an explicit `--subject` and
`--body-file` on the squash so no commit-message marker leaks and the post-merge
run spawns immediately. Then **watch the post-merge main run**: verify the macOS
bench at STEP level (a green job can mask a skipped bench) and download the
artifact — but per B5's ruling, read no meaning into the number.

---

## Self-Review

**Spec coverage:**

| Design § | Task |
|---|---|
| §4 layout, `LEFT_DOCK_WIDTH` | T4 |
| §5 radio invariant, single writer | T2 |
| §5.2 shim routing | T2 |
| §5.3 `sync_left_dock` | T4 |
| §5.4 session unchanged | Global constraint + T7 diff check |
| §6 thin panels, `register_panels` | T3 |
| §6.1 body methods, handle identity | T3 |
| §6.2 titles and per-panel labelling | T3 (panels) + T4 (body removals) |
| §7 rail structure, keyboard, visuals | T5 |
| §7.4 action-named items | T1 (keys) + T5 (use) + T4 (duplicate test) |
| §8 icons, NOTICE, i18n | T1 |
| §9 shell changes | T2-T5 |
| §10 R1-R8 | R1/R2 → T0+T2; R3 → T3; R4 → T7 boot; R5 → T5; R6 → T0/P5; R7 → T6; R8 → T4 |
| §11 test plan | T2, T4, T6 |
| §12 T0 gate | T0 |
| §13 local gate | T7 |
| §14 owed glance | T7 as-built records it |

No gaps.

**Placeholder scan:** every code step carries real code; the only deferred content
is the three SVG payloads, which are deliberately fetched verbatim rather than
authored (fabricated path data would render a plausible wrong icon that no test
catches), with an explicit stop-and-ask if the fetch is blocked.

**Type consistency:** `LeftPanel` is `pub` (integration tests name it);
`activate_left_panel(target, cx)`, `left_panel_visible(p)`, `open_left_panel()`,
`set_left_panel_exclusive(Option<LeftPanel>)`, `render_catalog_body(cx)`,
`catalog_visible()`, `left_dock_open_for_test(cx)`, `rail_move_cursor(delta, cx)`,
`rail_activate_cursor(cx)`, `rail_click(index, cx)`, `rail_cursor_for_test()`,
`render_rail(cursor, open, fh, cx)` — each defined once and used with the same
signature everywhere.
