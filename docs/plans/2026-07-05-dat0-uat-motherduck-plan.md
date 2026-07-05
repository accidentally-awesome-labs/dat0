# MotherDuck UAT Slice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add test-only, content + state-display UAT coverage for the three P9b MotherDuck UI surfaces — the Catalog "Cloud" group, the Test-connection result states, and the SQL-console routing chip — via the AccessKit + full-shell-mount harness.

**Architecture:** Full `WorkspaceShell` mount (Slice-3 pattern) in a new `tests/motherduck_window.rs`. Every surface is driven by seeding in-memory state (`ConnectionManager`, a fake catalog tree, the console's `last_elapsed`) through `#[cfg(feature = "a11y-capture")]` `*_for_test` shims, then asserted with `A11ySnapshot`. Click-free inject-and-assert — no live token, keychain, or engine round-trip. Production seams chain `.a11y_label` onto elements that already exist, so release markup is byte-identical (the seams are identity no-ops when `a11y-capture` is off).

**Tech Stack:** Rust, gpui 0.2.2 (`#[gpui::test]` + `VisualTestContext`), gpui-component 0.5.1, kittest 0.3.0 / accesskit 0.21.1 (test-only), the crate's `a11y` module (`A11yExt` / `AccessRole`), `tests/support/mod.rs` (`A11ySnapshot`), `serial_test`, `tempfile`, `tokio`.

## Global Constraints

- **Zero new dependencies** — Cargo.lock and NOTICE must stay unchanged. **D-015 stays open** (`a11y-capture` remains a test-only, release-off feature; no gpui fork).
- **Zero owed human visual glances** — every production seam chains `.a11y_label` onto an element that already exists in the tree (no new wrapper `div`). In release (`a11y-capture` off) `.a11y_label` is `#[inline] fn a11y_label(self, ..) -> Self { self }` → byte-identical markup.
- **Click-free inject-and-assert** — no clicks on any MD button (Test → live `spawn_md_test`; Connect → token prompt). No network, keychain, or engine round-trip. No safety spine needed (no side-effect path is exercised).
- **Shims placement** — all `*_for_test` shims go in the EXISTING `#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block in `window.rs` (opens at `window.rs:6591`), which sits BEFORE `#[cfg(test)] mod tests` (clippy `items-after-test-module` under `-D warnings` rejects items after a test module).
- **Feature is auto-on for integration tests** via the crate's self-dev-dependency (`dat0-app = { path = ".", features = ["a11y-capture"] }` in `[dev-dependencies]`), so `cargo test -p dat0-app --test motherduck_window` runs WITH the feature — no `--features` flag.
- **i18n assertions use the literal English strings** (`dat0_i18n::t` echoes the key when missing, so a literal doubles as a key-exists check). Exact strings: `catalog.cloud`="Cloud", `connections.md.test`="Test connection", `connections.md.connect`="Connect", `connections.md.disconnect`="Disconnect", `connections.md.forget`="Forget token", `connections.md.retry`="Retry", `connections.md.test.ok`="Connection OK", `sql.md`="md", `sql.local`="local".
- **Branch:** `uat-motherduck-slice` off `main` (`02ad054`). Commits: DCO sign-off (`git commit -s`) + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.
- **Anti-loop execution rule:** implementer subagents run ONLY the focused test synchronously (`cargo test -p dat0-app --test motherduck_window`); the controller runs the `cargo test --workspace` + `clippy --workspace --all-targets -D warnings` + `fmt --check` gate. Never background `cargo test --workspace` in an implementer.

## File Structure

- **Create** `crates/dat0-app/tests/motherduck_window.rs` — the slice's test binary. Harness helpers (`set_config_dir` / `build_empty_session` / `open_shell_window` / `init_components` / `AsyncHarness` / `enter_async_harness`) are COPIED verbatim from `tests/chart_uat_window.rs` (which copied them from `tests/onboarding_gpui.rs`) — matching the per-binary-copy precedent; NOT centralized. `mod support;` reuses the shared `A11ySnapshot`.
- **Modify** `crates/dat0-app/src/window.rs` — add 3 `*_for_test` shims to the existing `#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block (~`window.rs:6591`).
- **Modify** `crates/dat0-app/src/catalog/panel.rs` — 2 seams (section header + `catalog_row`) + import.
- **Modify** `crates/dat0-app/src/connections/panel.rs` — 3 seams (`action_button`, Test-result message, Error-arm container) + import. (The Error-arm seam lands in Task 2.)
- **No change** to `view/sql_console.rs` — the routing chip is already seamed at `sql_console.rs:931`.

---

### Task 0: T0 spike (HARD GATE) — prove all three paint paths

**Rationale (Slice-3 lesson):** a T0 spike only proves the surfaces it exercises. This task adds all infrastructure (harness copy + shims + seams) and one proof per surface, so a paint-path gap surfaces HERE, not late. Order: Cloud → Test-connection → Routing. The routing chip (crux R1: `SqlConsole` is lazily built via `toggle_sql_console`) is proven LAST and carries a STOP-and-report clause, so a routing blow-up cannot sink the two surfaces already proven.

**Files:**
- Create: `crates/dat0-app/tests/motherduck_window.rs`
- Modify: `crates/dat0-app/src/window.rs` (shim block ~:6591)
- Modify: `crates/dat0-app/src/catalog/panel.rs`
- Modify: `crates/dat0-app/src/connections/panel.rs`

**Interfaces:**
- Consumes: `A11ySnapshot::{capture, has_label, has_label_contains}` (`tests/support/mod.rs`); `WorkspaceShell::new`; `dat0_engine::{TableInfo, TableOrigin}`; `dat0_app::connections::{ConnectionStatus, ConnectionManager}`; `dat0_app::connections::routing::Routing`; `crate::catalog::CatalogTree::build`.
- Produces (shims other tasks rely on):
  - `WorkspaceShell::seed_catalog_tree_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>)`
  - `WorkspaceShell::open_connections_for_test(&mut self) -> &mut crate::connections::ConnectionManager`
  - `WorkspaceShell::seed_routing_chip_for_test(&mut self, ms: u64, routing: crate::connections::routing::Routing, window: &mut gpui::Window, cx: &mut Context<Self>)`
  - test-file helpers `md_tbl(&str)`, `file_tbl(&str)`.

- [ ] **Step 1: Scaffold the test file (harness copy, no tests yet)**

Create `crates/dat0-app/tests/motherduck_window.rs`:

```rust
//! UAT "MotherDuck UI" slice (Slice 5). Content + state-display coverage for the
//! three P9b surfaces: the Catalog "Cloud" group, the Test-connection result
//! states, and the SQL-console routing chip. Click-free inject-and-assert — no
//! live token/keychain/engine. Production seams chain `.a11y_label` onto existing
//! elements, so release markup is byte-identical (no owed human glance).
//!
//! Harness helpers below are COPIED verbatim from `tests/chart_uat_window.rs`
//! (which copied them from `tests/onboarding_gpui.rs`) — per-binary copy, matching
//! that precedent; NOT centralized.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;
use std::cell::RefCell;
use std::rc::Rc;

use support::A11ySnapshot;

use dat0_app::connections::routing::Routing;
use dat0_app::connections::ConnectionStatus;
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// Build a real, EMPTY in-memory session inside a dedicated multi-thread tokio
/// runtime (`Session::new` is async + uses `spawn_blocking`).
fn build_empty_session(state_root: &Path) -> Arc<Mutex<Session>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let sess = rt
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}

/// Open a real ACTIVATED window whose root is a `gpui_component::Root` wrapping a
/// fresh `WorkspaceShell` over `session` (mirrors production `open_window_view`).
fn open_shell_window(
    cx: &mut TestAppContext,
    session: Arc<Mutex<Session>>,
) -> (Entity<WorkspaceShell>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<WorkspaceShell>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        *slot2.borrow_mut() = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = slot.borrow().clone().expect("shell captured");
    (shell, vcx)
}

/// Initialise the gpui-component theme global — required before any gpui-component
/// widget renders.
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

/// A tokio runtime kept alive for the whole test so foreground-polled `cx.spawn`
/// futures can call `tokio::task::spawn_blocking`. Needed by the routing-chip test
/// because `toggle_sql_console` may refresh `md_databases` off the engine on open.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
    }
    #[allow(dead_code)]
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

/// A fake `md:`-origin table (→ Catalog "Cloud" group via the `tree.rs` classifier).
fn md_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::Attached {
            alias: "sample_data".into(),
            source: "md:sample_data".into(),
        },
    }
}

/// A fake local `File`-origin table (→ Catalog "Sources" group, never "Cloud").
fn file_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(std::path::PathBuf::from("/data/local.csv")),
    }
}
```

- [ ] **Step 2: Verify the scaffold compiles (no tests → no run)**

Run: `cargo test -p dat0-app --test motherduck_window --no-run`
Expected: compiles clean (helpers may warn `dead_code` — acceptable until Step 3 uses them; `Routing`/`ConnectionStatus` imports unused until later steps — if the `-D warnings` dev profile rejects, add `#[allow(unused_imports)]` on the two `use` lines TEMPORARILY and remove it in Step 6). If it does not compile, fix before proceeding.

- [ ] **Step 3a: Cloud group — write the failing test**

Append to `tests/motherduck_window.rs`:

```rust
#[gpui::test]
#[serial]
fn cloud_group_renders_md_table_not_file(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    // One md-attached table + one local file table → the classifier must route
    // the md one to "Cloud" and the file one to "Sources".
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_catalog_tree_for_test(vec![md_tbl("md_events"), file_tbl("local_sales")]);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    // The flat a11y tree has no section structure, so the SECTION-HEADER counts are
    // the teeth: a misclassified md table would read "Cloud (0)" / "Sources (2)".
    assert!(snap.has_label("Cloud (1)"), "Cloud section holds exactly the 1 md table");
    assert!(snap.has_label("md_events"), "md-attached table row renders");
    assert!(snap.has_label("Sources (1)"), "file table classified to Sources, not Cloud");
}
```

- [ ] **Step 3b: Run it — expect FAIL (compile: shim missing)**

Run: `cargo test -p dat0-app --test motherduck_window cloud_group_renders_md_table_not_file`
Expected: FAIL — `no method named seed_catalog_tree_for_test found for ... WorkspaceShell`.

- [ ] **Step 3c: Add the `seed_catalog_tree_for_test` shim**

In `crates/dat0-app/src/window.rs`, inside the `#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block (after `seed_catalog_for_test` at ~:6618), add:

```rust
    /// Build the catalog tree DIRECTLY from seeded fakes and show the catalog dock.
    /// Bypasses `refresh_catalog`'s off-thread `get_tables` (window.rs:2999), which
    /// would clobber the fakes with the empty test engine's real (empty) tables.
    /// Seed an `md:`-origin `TableInfo` to populate the "Cloud" group.
    pub fn seed_catalog_tree_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tree = crate::catalog::CatalogTree::build(&tables);
        self.catalog_panel_visible = true;
    }
```

- [ ] **Step 3d: Run again — expect FAIL on the assertion (seam missing)**

Run: `cargo test -p dat0-app --test motherduck_window cloud_group_renders_md_table_not_file`
Expected: FAIL — `assertion failed: snap.has_label("Cloud (1)")` (the header + rows are plain `.child(...)`, invisible to AccessKit). This confirms the seam is required.

- [ ] **Step 3e: Add the two catalog seams**

In `crates/dat0-app/src/catalog/panel.rs`, add the import after the existing `use` block (line ~13):

```rust
use crate::a11y::A11yExt as _;
```

Replace the section-loop body (lines 47–57) so the header and each row carry a content seam:

```rust
    for (label, id, nodes) in &sections {
        let header = section_label(label, nodes.len());
        let mut section = div().flex().flex_col().gap_1().child(
            div()
                .a11y_label(crate::a11y::AccessRole::Label, header.clone())
                .child(SharedString::from(header)),
        );
        for node in nodes.iter() {
            section = section.child(catalog_row(id, &node.name, cx));
        }
        root = root.child(section);
    }
```

In `catalog_row` (lines 74–83), chain the row seam after `.child(...)`:

```rust
    div()
        .id(SharedString::from(format!("cat-{section}-{name}")))
        .px_2()
        .py_1()
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgba(0x80808022)))
        .child(SharedString::from(name.clone()))
        .a11y_label(crate::a11y::AccessRole::Label, name.clone())
        .on_click(cx.listener(move |ws, _ev, window, cx| {
            ws.open_table_tab(name.clone(), window, cx);
        }))
```

- [ ] **Step 3f: Run — expect PASS**

Run: `cargo test -p dat0-app --test motherduck_window cloud_group_renders_md_table_not_file`
Expected: PASS.

- [ ] **Step 4a: Test-connection (Disconnected) — write the failing test**

Append to `tests/motherduck_window.rs`:

```rust
#[gpui::test]
#[serial]
fn test_result_renders_disconnected(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            let mgr = ws.open_connections_for_test();
            mgr.set_md_status(ConnectionStatus::Disconnected);
            mgr.set_md_test_result("Connection OK".into());
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("Connect"), "Disconnected arm shows Connect");
    assert!(snap.has_label("Test connection"), "Disconnected arm shows Test");
    assert!(snap.has_label("Connection OK"), "seeded test-result message renders");
}
```

- [ ] **Step 4b: Run — expect FAIL (compile: shim missing)**

Run: `cargo test -p dat0-app --test motherduck_window test_result_renders_disconnected`
Expected: FAIL — `no method named open_connections_for_test`.

- [ ] **Step 4c: Add the `open_connections_for_test` shim**

In the same `window.rs` shim block, add:

```rust
    /// Show the Connections dock and hand back the `ConnectionManager` so the test
    /// can drive `set_md_status` / `set_md_test_result` / `set_md_databases` (all
    /// already `pub`). No live connection, token, or keychain touched.
    pub fn open_connections_for_test(&mut self) -> &mut crate::connections::ConnectionManager {
        self.connections_panel_visible = true;
        &mut self.connections
    }
```

- [ ] **Step 4d: Run again — expect FAIL on the assertion (seams missing)**

Run: `cargo test -p dat0-app --test motherduck_window test_result_renders_disconnected`
Expected: FAIL — `assertion failed: snap.has_label("Connect")` (buttons + result message are plain, invisible).

- [ ] **Step 4e: Add the two connections seams**

In `crates/dat0-app/src/connections/panel.rs`, add the import after line 17:

```rust
use crate::a11y::A11yExt as _;
```

Seam every button via `action_button` (replace lines 197–213):

```rust
fn action_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    ev: ConnectionsEvent,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .px_2()
        .py_1()
        .border_1()
        .cursor_pointer()
        .a11y_label(crate::a11y::AccessRole::Label, label.to_string())
        .child(label)
        .on_click(cx.listener(move |ws, _ev, window, cx| {
            ws.handle_connections_event(ev.clone(), window, cx);
        }))
}
```

Seam the Test-result message (replace lines 139–142):

```rust
    let md_section = match manager.md_test_result() {
        Some(msg) => md_section.child(
            div()
                .a11y_label(crate::a11y::AccessRole::Label, msg.to_string())
                .child(SharedString::from(msg.to_string())),
        ),
        None => md_section,
    };
```

- [ ] **Step 4f: Run — expect PASS**

Run: `cargo test -p dat0-app --test motherduck_window test_result_renders_disconnected`
Expected: PASS. (If `has_label` panics on a duplicate, some other visible panel emitted the same label — switch that assertion to `has_label_any`; not expected here.)

- [ ] **Step 5a: Routing chip — write the failing test (CRUX R1)**

Append to `tests/motherduck_window.rs`:

```rust
#[gpui::test]
#[serial]
fn routing_chip_shows_md_not_local(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    // toggle_sql_console may refresh md_databases off the engine on open → runtime.
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.update(|window, app| {
        shell.update(app, |ws, cx| {
            ws.seed_routing_chip_for_test(1234, Routing::Md, window, cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_contains("ms · md"), "routing chip shows the md suffix");
    assert!(!snap.has_label_contains("· local"), "teeth: not the local suffix");
}
```

- [ ] **Step 5b: Run — expect FAIL (compile: shim missing)**

Run: `cargo test -p dat0-app --test motherduck_window routing_chip_shows_md_not_local`
Expected: FAIL — `no method named seed_routing_chip_for_test`.

- [ ] **Step 5c: Add the `seed_routing_chip_for_test` shim**

In the same `window.rs` shim block, add:

```rust
    /// Build + show the SQL console, then seed the timing chip's elapsed + routing
    /// so the chip renders its routing suffix without a real query run. The console
    /// is lazily built by `toggle_sql_console` (needs `&mut Window`); `set_last_elapsed`
    /// (sql_console.rs:340) sets `last_elapsed_ms` + `last_routing`, which is all the
    /// chip's render gate `(running == false, Some(ms))` needs.
    pub fn seed_routing_chip_for_test(
        &mut self,
        ms: u64,
        routing: crate::connections::routing::Routing,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sql_console_visible {
            self.toggle_sql_console(window, cx);
        }
        if let Some(console) = self.sql_console.clone() {
            console.update(cx, |c, cx| c.set_last_elapsed(ms, routing, cx));
        }
    }
```

- [ ] **Step 5d: Run — expect PASS (routing chip already seamed at sql_console.rs:931)**

Run: `cargo test -p dat0-app --test motherduck_window routing_chip_shows_md_not_local`
Expected: PASS.

**⚠ STOP-AND-REPORT CLAUSE (R1).** If this test cannot be made green with reasonable effort — e.g. `toggle_sql_console` panics under `TestPlatform`, hangs, requires driving a cross-thread `spawn_blocking` to completion (see `enter_async_harness` note — `run_until_parked` does not wait for it; a `block_test`/pump loop would be needed), or the chip needs a real run to populate — do NOT sink the slice into it. The user chose the routing chip knowing it is the marginal surface (already seamed; `classify_routing` already unit-tested). Halt, delete this test + the `seed_routing_chip_for_test` shim + its imports, and report to the controller/user that the routing chip is descoped and why. Tasks 1–2 (Cloud + Test-connection, already proven) remain the slice.

- [ ] **Step 6: Clean up + run the whole file**

Remove any temporary `#[allow(unused_imports)]` from Step 2. Run the whole binary:

Run: `cargo test -p dat0-app --test motherduck_window`
Expected: 3 passed (or 2 passed if routing was descoped per the STOP clause).

- [ ] **Step 7: Commit**

```bash
git add crates/dat0-app/tests/motherduck_window.rs \
        crates/dat0-app/src/window.rs \
        crates/dat0-app/src/catalog/panel.rs \
        crates/dat0-app/src/connections/panel.rs
git commit -s -m "test(harness): MotherDuck UI T0 spike — Cloud group + Test-conn + routing chip

Full-shell mount + 3 *_for_test shims + inert-in-release .a11y_label seams
(catalog header/row, connections button/result). Proves all three paint
paths (Slice-3 lesson: spike every asserted surface).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 1: Test-connection — Connected arm

**Files:**
- Modify: `crates/dat0-app/tests/motherduck_window.rs` (add one test)

**Interfaces:**
- Consumes: `WorkspaceShell::open_connections_for_test` (Task 0); `ConnectionStatus::Connected`; `ConnectionManager::{set_md_status, set_md_databases, set_md_test_result}`.
- Produces: nothing downstream.

- [ ] **Step 1: Write the failing test**

Append to `tests/motherduck_window.rs`:

```rust
#[gpui::test]
#[serial]
fn test_result_renders_connected(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            let mgr = ws.open_connections_for_test();
            mgr.set_md_status(ConnectionStatus::Connected); // keeps md_databases
            mgr.set_md_databases(vec!["sample_data".into()]);
            mgr.set_md_test_result("Connection OK".into());
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("Disconnect"), "Connected arm shows Disconnect");
    assert!(snap.has_label("Test connection"), "Connected arm shows Test");
    assert!(snap.has_label("Forget token"), "Connected arm shows Forget");
    assert!(snap.has_label("sample_data"), "attached db name renders under Connected");
    assert!(snap.has_label("Connection OK"), "test-result message renders");
}
```

- [ ] **Step 2: Run — expect PASS (all seams + shim exist from Task 0)**

Run: `cargo test -p dat0-app --test motherduck_window test_result_renders_connected`
Expected: PASS. Note: the db-name list (`md_databases`, `panel.rs:124`) is a plain `.child(...)` on a `div().pl_4()`; if `has_label("sample_data")` FAILS, that row needs a seam — chain `.a11y_label(crate::a11y::AccessRole::Label, name.clone())` on the `div().pl_4()` at `connections/panel.rs:124` (byte-identical release), then re-run.

- [ ] **Step 3: Commit**

```bash
git add crates/dat0-app/tests/motherduck_window.rs crates/dat0-app/src/connections/panel.rs
git commit -s -m "test(harness): MotherDuck Test-connection Connected arm

Connected md_actions arm renders Disconnect/Forget/Test + attached db name
+ seeded test-result message.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Test-connection — Error arm differential

**Files:**
- Modify: `crates/dat0-app/src/connections/panel.rs` (Error-arm container seam)
- Modify: `crates/dat0-app/tests/motherduck_window.rs` (add one test)

**Interfaces:**
- Consumes: `WorkspaceShell::open_connections_for_test` (Task 0); `ConnectionStatus::Error(String)`.
- Produces: nothing downstream.

- [ ] **Step 1: Write the failing test**

Append to `tests/motherduck_window.rs`:

```rust
#[gpui::test]
#[serial]
fn error_arm_hides_test_shows_retry(cx: &mut TestAppContext) {
    init_components(cx);
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            let mgr = ws.open_connections_for_test();
            mgr.set_md_status(ConnectionStatus::Error("Auth failed".into()));
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label("Retry"), "Error arm shows Retry");
    assert!(snap.has_label("Auth failed"), "Error arm shows the error message");
    assert!(!snap.has_label("Test connection"), "teeth: Test button absent in Error arm");
    assert!(!snap.has_label("Disconnect"), "teeth: Disconnect absent in Error arm");
}
```

- [ ] **Step 2: Run — expect FAIL on the error-message assertion**

Run: `cargo test -p dat0-app --test motherduck_window error_arm_hides_test_shows_retry`
Expected: FAIL — `assertion failed: snap.has_label("Auth failed")`. The Retry-present and Test/Disconnect-absent assertions already pass (buttons seamed in Task 0), but the Error-arm message (`connections/panel.rs:109`) is a plain `.child(SharedString::from(msg.clone()))` with no seam.

- [ ] **Step 3: Seam the Error-arm message**

In `crates/dat0-app/src/connections/panel.rs`, in the `ConnectionStatus::Error(msg)` arm (lines 104–115), chain a content seam onto the arm's existing container `div` (no new element):

```rust
        ConnectionStatus::Error(msg) => div()
            .flex()
            .flex_col()
            .gap_1()
            .a11y_label(crate::a11y::AccessRole::Label, msg.clone())
            // The localized error message carried by the status.
            .child(SharedString::from(msg.clone()))
            .child(action_button(
                "connections-md-retry",
                dat0_i18n::t("connections.md.retry"),
                ConnectionsEvent::ConnectMd,
                cx,
            )),
```

- [ ] **Step 4: Run — expect PASS**

Run: `cargo test -p dat0-app --test motherduck_window error_arm_hides_test_shows_retry`
Expected: PASS.

- [ ] **Step 5: Run the whole binary**

Run: `cargo test -p dat0-app --test motherduck_window`
Expected: 5 passed (or 4 if routing was descoped in Task 0).

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/src/connections/panel.rs crates/dat0-app/tests/motherduck_window.rs
git commit -s -m "test(harness): MotherDuck Test-connection Error arm differential

Error arm hides Test/Disconnect, shows Retry + the error message (seamed on
the arm container, byte-identical release).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Final gate (controller-run, after all tasks)

- [ ] **Workspace test + lint + fmt gate**

Run (controller only — never background in an implementer):
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```
Expected: all green; test count up by 5 (or 4 if routing descoped). Confirm `git status` shows Cargo.lock and NOTICE UNCHANGED (zero new deps). Confirm no `.a11y_label` seam introduced a new `div` (grep the diff for added `div()` in the three prod files — there should be exactly one new inner `div` only in the Test-result seam, which the design accepts... NO: re-check — the result-message seam reuses the EXISTING inner `div`, so there should be ZERO new `div()` in the release path; if the diff added any, convert to a chain-on-existing form).

- [ ] **Release-build byte-identical check**

Run: `cargo build -p dat0-app` (no `a11y-capture`) and confirm it compiles clean with no unused-import warnings (the `use crate::a11y::A11yExt as _;` lines are used by the `.a11y_label` calls in both cfgs, so no `#[cfg]`-gate is needed on them). This confirms the seams are inert in release.

## Self-Review

**1. Spec coverage** (design §Scope → tasks):
- Cloud catalog group → Task 0 Step 3 (`cloud_group_renders_md_table_not_file`, with header-count teeth). ✓
- Test-connection result states → Task 0 Step 4 (Disconnected) + Task 1 (Connected) + Task 2 (Error differential). ✓
- Routing chip → Task 0 Step 5 (`routing_chip_shows_md_not_local`, with local-suffix teeth + R1 STOP clause). ✓
- "seams chain onto existing → no glance" → Global Constraints + final-gate grep check. ✓
- "no dup of unit-tested logic" → the plan asserts render/state only; `test_result_message`/`classify_routing`/classifier/token-guard untouched. ✓
- T0 spike hard-gate proving every surface → Task 0 structure. ✓

**2. Placeholder scan:** no "TBD/TODO/handle edge cases"; every step shows exact code or an exact command + expected output. The one conditional ("if `has_label` panics on a duplicate → `has_label_any`" / "if db-name assertion fails → seam `panel.rs:124`") is a stated, coded fallback, not a placeholder.

**3. Type consistency:** shim names identical across tasks (`seed_catalog_tree_for_test`, `open_connections_for_test`, `seed_routing_chip_for_test`); `ConnectionStatus`/`Routing`/`ConnectionManager` paths match `lib.rs:18` exports; `set_last_elapsed(ms, routing, cx)` matches `sql_console.rs:340`; `CatalogTree::build(&tables)` matches `catalog/tree.rs`. Seam count consistent: catalog ×2 (Task 0), connections ×2 (Task 0) + ×1 (Task 2, Error container) + possibly ×1 (Task 1 fallback db-name row).

No gaps found.
