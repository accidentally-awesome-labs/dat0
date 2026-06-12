//! Recovery panel + supporting non-UI helpers (P3b T5).
//!
//! The non-UI helpers ([`load_for_open`], [`discard`]) are unit-testable
//! without GPUI; the [`open`] entry point is invoked by the
//! `recovery.review` action descriptor on the GPUI main thread.
//!
//! # On-disk shape
//!
//! P3a's `crate::session::Session` serialises tab state as
//! `{ "tabs": [ { "table_name": "...", "source_path": "..." } ],
//!    "active_tab": <usize|null> }` (see `session.rs:23-29` —
//! `SessionState` + `Tab`). The recovery panel does NOT reuse those
//! types directly because:
//!
//! 1. `Session::Tab` owns a `String` for `table_name` and an
//!    `Option<PathBuf>` for `source_path`. The recovery panel only
//!    needs the surface fields a user sees in the list ("which file?"
//!    / "which table?") and never round-trips back to `Session`.
//! 2. Using `serde(rename)` lets the in-memory field names match the
//!    UX vocabulary (`path`, `table`) without touching the on-disk
//!    schema — the recovery flow stays decoupled from future Session
//!    field renames.
//!
//! # GPUI view
//!
//! [`open`] mounts a gpui-component `Sheet` (P7c T8). The action-dispatch
//! path holds only `&mut App`, so it reaches a `&mut Window` via the
//! proven active-window hop (`cx.active_window()` → `handle.update`,
//! T0 §8.2 / `workspace_in_use_modal.rs`) and calls
//! `WindowExt::open_sheet_at(Placement::Top, …)`. The Sheet draws only
//! because `WorkspaceShell::render` mounts `Root::render_sheet_layer`
//! (added in this task — gpui-component's `Root::render` does NOT
//! auto-mount overlay layers). The rows are built by the pure,
//! unit-tested [`collect_rows`]; the per-row buttons (Open / Resume /
//! Discard) are placeholder no-ops here — wiring their behaviour is T9.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// A single restored tab as surfaced to the recovery panel UI.
///
/// JSON keys (`table_name` / `source_path`) match the on-disk shape
/// owned by `session::SessionState`; the in-memory field names
/// (`table` / `path`) match the UX vocabulary.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoredTab {
    #[serde(rename = "source_path")]
    pub path: PathBuf,
    #[serde(rename = "table_name")]
    pub table: String,
}

/// Restored session-level state surfaced to the recovery panel UI.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoredSession {
    pub tabs: Vec<RestoredTab>,
    pub active_tab: Option<usize>,
}

/// Load `session.json` from an orphan scratch dir into a
/// [`RestoredSession`]. Used by the "Open" row action to populate the
/// new window's tab list.
pub fn load_for_open(orphan_dir: &Path) -> Result<RestoredSession> {
    let session_json = orphan_dir.join("session.json");
    let raw = fs::read_to_string(&session_json)
        .with_context(|| format!("read {}", session_json.display()))?;
    let parsed: RestoredSession =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", session_json.display()))?;
    Ok(parsed)
}

/// Permanently remove an orphan scratch dir and everything under it.
/// Used by the "Discard" row action.
pub fn discard(orphan_dir: &Path) -> Result<()> {
    fs::remove_dir_all(orphan_dir).with_context(|| format!("remove {}", orphan_dir.display()))
}

/// Remove only the `.dat0/` subdir of an interrupted workspace — never the
/// user's folder or their source files. Used by the "Discard" action on an
/// [`RecoveryRow::Incomplete`] row, so the user's project folder (and any
/// data files alongside it) survive while the half-written promotion is
/// cleared.
pub fn discard_incomplete(root: &Path) -> Result<()> {
    let dat0 = crate::workspace::Home::dat0_dir_for(root);
    fs::remove_dir_all(&dat0).with_context(|| format!("remove {}", dat0.display()))
}

/// A single recoverable item surfaced in the Recovery Sheet.
///
/// Two kinds, mirroring the two boot-scan sources consolidated by
/// [`crate::window::recovery_scan_emit`]:
/// - [`RecoveryRow::Orphan`] — a scratch subdir holding a `session.json` from a
///   session that didn't exit cleanly, carrying the restored table names so the
///   row can label "which tables".
/// - [`RecoveryRow::Incomplete`] — a recent workspace folder whose `.dat0/` is a
///   half-finished promotion (missing `manifest.json` / `workspace.duckdb`).
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryRow {
    /// Orphan scratch dir + its restored table names (for the row label).
    Orphan { dir: PathBuf, tables: Vec<String> },
    /// Interrupted workspace promotion, keyed by the workspace folder root.
    Incomplete { root: PathBuf },
}

/// Collect recovery rows for the Sheet: orphan scratch dirs (each with its
/// restored table names) + interrupted workspaces.
///
/// Pure given the two roots, so it's unit-testable without GPUI. The orphan
/// test (`session.json` present under a scratch subdir) matches
/// [`crate::window::count_orphan_scratch`]'s definition exactly; the incomplete
/// scan delegates to [`crate::recovery_scan::scan_incomplete_workspaces`] so the
/// Sheet and the boot banner can never drift on what counts as recoverable.
///
/// A row's table list is best-effort: if `session.json` fails to parse we keep
/// the orphan row with an empty `tables` (it's still recoverable / discardable).
pub fn collect_rows(scratch_root: &Path, recent_roots: &[PathBuf]) -> Vec<RecoveryRow> {
    let mut rows = Vec::new();
    if let Ok(read) = fs::read_dir(scratch_root) {
        for entry in read.flatten() {
            let dir = entry.path();
            if dir.join("session.json").is_file() {
                let tables = load_for_open(&dir)
                    .map(|s| s.tabs.into_iter().map(|t| t.table).collect())
                    .unwrap_or_default();
                rows.push(RecoveryRow::Orphan { dir, tables });
            }
        }
    }
    for root in crate::recovery_scan::scan_incomplete_workspaces(recent_roots) {
        rows.push(RecoveryRow::Incomplete { root });
    }
    rows
}

/// Build one Sheet row for a recovery item. Orphan rows label the restored
/// table names + offer Open/Discard; incomplete rows label the workspace root
/// (+ a "(promote didn't finish)" suffix) + offer Resume/Discard.
///
/// **Behaviour (T9).** Each handler closes the Sheet first
/// (`window.close_sheet(cx)`), then delegates to an EXISTING flow:
/// - **Open** (orphan) → [`crate::window::spawn_recovered_scratch`], which
///   reuses `Session::recover` + the shared `open_window_view` spawn path to
///   bring the orphan's restored tabs back live.
/// - **Resume** (incomplete) → [`crate::window::spawn_workspace_window`] over
///   the known root, reusing P7a's `recover_workspace` adopt path; on hard
///   failure it already pushes the `workspace.open.failed.title` banner and
///   leaves this row available for retry / discard.
/// - **Discard** → [`discard`] (orphan) / [`discard_incomplete`] (incomplete),
///   then re-opens the Sheet via [`open`] so the freshly re-scanned row set no
///   longer shows the removed entry.
///
/// The `on_click` closures are `Fn`, so each captures its own CLONE of the
/// row's `dir` / `root` (mirroring `workspace_in_use_modal.rs`). `i`
/// disambiguates per-row `ElementId`s (a `String` is NOT `Into<ElementId>`;
/// `SharedString` is — see the T3 ElementId gotcha).
fn render_row(i: usize, row: &RecoveryRow) -> impl gpui::IntoElement {
    use gpui::{ParentElement as _, SharedString, Styled as _, div};
    use gpui_component::WindowExt as _;
    use gpui_component::button::Button;

    let mut buttons = gpui_component::h_flex().gap_2();
    let label: String = match row {
        RecoveryRow::Orphan { dir, tables } => {
            let open_dir = dir.clone();
            let primary = Button::new(SharedString::from(format!("recovery-open-{i}")))
                .label(dat0_i18n::t("recovery.row.open"))
                .on_click(move |_ev, window, cx| {
                    window.close_sheet(cx);
                    crate::window::spawn_recovered_scratch(cx, open_dir.clone());
                });
            let discard_dir = dir.clone();
            let discard = Button::new(SharedString::from(format!("recovery-discard-{i}")))
                .label(dat0_i18n::t("recovery.row.discard"))
                .on_click(move |_ev, window, cx| {
                    window.close_sheet(cx);
                    if let Err(e) = discard(&discard_dir) {
                        tracing::warn!(?e, dir = %discard_dir.display(), "discard orphan failed");
                    }
                    open(cx); // re-scan: the removed row is gone
                });
            buttons = buttons.child(primary).child(discard);
            if tables.is_empty() {
                String::new()
            } else {
                tables.join(", ")
            }
        }
        RecoveryRow::Incomplete { root } => {
            let resume_root = root.clone();
            let primary = Button::new(SharedString::from(format!("recovery-resume-{i}")))
                .label(dat0_i18n::t("recovery.row.resume"))
                .on_click(move |_ev, window, cx| {
                    window.close_sheet(cx);
                    // Best-effort adopt of the partial `.dat0/` via P7a's
                    // recover_workspace path; on hard failure spawn_workspace_window
                    // pushes the open-failed banner and the row stays for retry.
                    crate::window::spawn_workspace_window(cx, resume_root.clone(), None);
                });
            let discard_root = root.clone();
            let discard = Button::new(SharedString::from(format!("recovery-discard-{i}")))
                .label(dat0_i18n::t("recovery.row.discard"))
                .on_click(move |_ev, window, cx| {
                    window.close_sheet(cx);
                    if let Err(e) = discard_incomplete(&discard_root) {
                        tracing::warn!(?e, root = %discard_root.display(), "discard incomplete failed");
                    }
                    open(cx); // re-scan: the removed row is gone
                });
            buttons = buttons.child(primary).child(discard);
            format!(
                "{} {}",
                root.display(),
                dat0_i18n::t("recovery.row.incomplete_suffix")
            )
        }
    };

    gpui_component::h_flex()
        .justify_between()
        .items_center()
        .gap_4()
        .py_1()
        .child(div().flex_1().child(label))
        .child(buttons)
}

/// Entry point invoked by the `recovery.review` action descriptor
/// (see `actions::builtin::ids::RECOVERY_REVIEW`).
///
/// Opens a top-anchored gpui-component `Sheet` listing every recoverable item
/// (orphan scratch sessions + interrupted workspaces). The rows come from the
/// pure [`collect_rows`]; the per-row buttons are placeholder no-ops in T8 —
/// T9 wires Open / Resume / Discard.
///
/// Reaches a `&mut Window` from the `&mut App` action context via the proven
/// active-window hop (T0 §8.2): `cx.active_window()` → `handle.update` →
/// `WindowExt::open_sheet_at`. Requires `WorkspaceShell::render` to mount
/// `Root::render_sheet_layer` (added in this task) or the Sheet paints nothing.
/// No-ops (just logs) when there is nothing to recover or no active window.
pub fn open(app: &mut gpui::App) {
    use gpui::{AnyView, ParentElement as _, Styled as _, Window, div};
    use gpui_component::Placement;
    use gpui_component::WindowExt as _;

    let scratch_root = match crate::window_registry::state_root() {
        Some(p) => p.join("scratch"),
        None => {
            tracing::warn!("recovery_panel::open: no state_root installed");
            return;
        }
    };
    let recents = crate::window_registry::recents_snapshot();
    let rows = collect_rows(&scratch_root, &recents);
    if rows.is_empty() {
        tracing::info!("recovery_panel::open: nothing to recover");
        return;
    }

    let Some(handle) = app.active_window() else {
        tracing::warn!("recovery_panel::open: no active window; cannot show Sheet");
        return;
    };
    let _ = handle.update(app, move |_root: AnyView, window: &mut Window, cx| {
        let rows = rows.clone();
        window.open_sheet_at(Placement::Top, cx, move |sheet, _w, _cx| {
            let mut list = div().flex().flex_col().gap_2().p_3();

            // Partition rows ONCE into the two section groups, keeping each
            // row's original index `i` (a process-stable per-row `ElementId`
            // seed over the FULL row list). Each header is emitted only when
            // its group is non-empty so the Sheet never shows a heading with
            // nothing under it.
            let mut orphans = Vec::new();
            let mut incompletes = Vec::new();
            for (i, row) in rows.iter().enumerate() {
                match row {
                    RecoveryRow::Orphan { .. } => orphans.push((i, row)),
                    RecoveryRow::Incomplete { .. } => incompletes.push((i, row)),
                }
            }

            if !orphans.is_empty() {
                list = list.child(section_header(dat0_i18n::t("recovery.sheet.orphans")));
                for (i, row) in orphans {
                    list = list.child(render_row(i, row));
                }
            }
            if !incompletes.is_empty() {
                list = list.child(section_header(dat0_i18n::t("recovery.sheet.incomplete")));
                for (i, row) in incompletes {
                    list = list.child(render_row(i, row));
                }
            }

            sheet
                .title(dat0_i18n::t("recovery.sheet.title"))
                .size(gpui::px(420.0))
                .child(list)
        });
    });
}

/// A small section heading inside the Recovery Sheet (e.g. "Orphaned sessions").
fn section_header(text: String) -> impl gpui::IntoElement {
    use gpui::{ParentElement as _, Styled as _, div};
    div().pt_2().text_sm().child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `collect_rows` returns exactly one Orphan (from a seeded scratch subdir
    /// holding a `session.json`, with its restored table names parsed) plus one
    /// Incomplete (from a partial `.dat0/` recent missing `manifest.json`).
    #[test]
    fn collect_rows_returns_one_orphan_and_one_incomplete() {
        let tmp = tempfile::TempDir::new().unwrap();

        // --- Orphan scratch dir: one subdir with a session.json (on-disk shape
        //     owned by `session::SessionState`: table_name / source_path keys). ---
        let scratch_root = tmp.path().join("scratch");
        let orphan = scratch_root.join("session-00");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(
            orphan.join("session.json"),
            r#"{"tabs":[{"table_name":"sales","source_path":"/data/sales.csv"},{"table_name":"orders","source_path":"/data/orders.csv"}],"active_tab":0}"#,
        )
        .unwrap();
        // A scratch subdir WITHOUT a session.json must NOT count as an orphan.
        fs::create_dir_all(scratch_root.join("session-01")).unwrap();

        // --- Incomplete workspace recent: `.dat0/` exists but manifest.json is
        //     missing (a half-finished promotion). ---
        let incomplete_root = tmp.path().join("proj-a");
        let dat0 = incomplete_root.join(".dat0");
        fs::create_dir_all(&dat0).unwrap();
        fs::write(dat0.join("workspace.duckdb"), b"db").unwrap();
        // A complete recent (both files present) must NOT count.
        let complete_root = tmp.path().join("proj-b");
        let cdat0 = complete_root.join(".dat0");
        fs::create_dir_all(&cdat0).unwrap();
        fs::write(cdat0.join("manifest.json"), "{}").unwrap();
        fs::write(cdat0.join("workspace.duckdb"), b"db").unwrap();

        let rows = collect_rows(&scratch_root, &[incomplete_root.clone(), complete_root]);

        assert_eq!(rows.len(), 2, "one Orphan + one Incomplete: {rows:?}");

        let orphan_row = rows
            .iter()
            .find(|r| matches!(r, RecoveryRow::Orphan { .. }))
            .expect("an Orphan row");
        match orphan_row {
            RecoveryRow::Orphan { dir, tables } => {
                assert_eq!(dir, &orphan);
                assert_eq!(tables, &vec!["sales".to_string(), "orders".to_string()]);
            }
            _ => unreachable!(),
        }

        let incomplete_row = rows
            .iter()
            .find(|r| matches!(r, RecoveryRow::Incomplete { .. }))
            .expect("an Incomplete row");
        assert_eq!(
            incomplete_row,
            &RecoveryRow::Incomplete {
                root: incomplete_root
            }
        );
    }
}
