//! The recovery panel: everything a previous session left behind.
//!
//! Two kinds of wreckage, from the two boot scans:
//!
//! * **Orphan** — a scratch directory holding a `session.json` from a session
//!   that never exited cleanly. Its restored table names label the row, so the
//!   user is choosing between recognisable sessions rather than between UUIDs.
//! * **Incomplete** — a recent workspace folder whose `.dat0/` is a
//!   half-finished promotion (no `manifest.json`, or no `workspace.duckdb`).
//!
//! # On-disk shape
//!
//! `dat0_core::session` writes tabs as
//! `{"tabs":[{"table_name":…,"source_path":…}],"active_tab":…}`. This surface
//! deliberately does not reuse those types: it needs only the two fields a
//! user sees, never round-trips back into a `Session`, and `serde(rename)`
//! lets the in-memory names match the UX vocabulary (`table`, `path`) without
//! pinning the on-disk schema to them.
//!
//! # Destruction is exact
//!
//! Discarding an orphan removes the scratch directory. Discarding an
//! incomplete workspace removes **only its `.dat0/` subdirectory** — never the
//! user's folder and never the data files sitting beside it. That asymmetry is
//! the whole reason there are two discard functions.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dioxus::prelude::*;
use serde::Deserialize;

use crate::a11y::AccessRole;

/// Dismissable: nothing is destroyed by closing, and everything reappears at
/// the next scan.
pub const SCRIM_DISMISSABLE: bool = true;

/// The header title the modal host should render above [`RecoveryPanel`].
pub fn title() -> String {
    dat0_i18n::t("recovery.sheet.title")
}

/// One restored tab, as the panel surfaces it.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoredTab {
    #[serde(rename = "table_name")]
    pub table: String,
    #[serde(rename = "source_path", default)]
    pub path: Option<PathBuf>,
}

/// The restored session-level state the panel surfaces.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoredSession {
    #[serde(default)]
    pub tabs: Vec<RestoredTab>,
}

/// Read `session.json` out of an orphan scratch directory.
pub fn load_for_open(orphan_dir: &Path) -> Result<RestoredSession> {
    let p = orphan_dir.join("session.json");
    let raw = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", p.display()))
}

/// Permanently remove an orphan scratch directory and everything under it.
pub fn discard(orphan_dir: &Path) -> Result<()> {
    fs::remove_dir_all(orphan_dir).with_context(|| format!("remove {}", orphan_dir.display()))
}

/// Remove only the `.dat0/` subdirectory of an interrupted workspace.
///
/// Never the user's folder and never their source files: a half-written
/// promotion is dat0's mess to clean, and the project folder around it is not.
pub fn discard_incomplete(root: &Path) -> Result<()> {
    let dir = root.join(".dat0");
    fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))
}

/// One recoverable item.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryRow {
    /// An orphan scratch directory and the table names it restores.
    Orphan { dir: PathBuf, tables: Vec<String> },
    /// An interrupted workspace promotion, keyed by the folder root.
    Incomplete { root: PathBuf },
}

/// Collect every recoverable item.
///
/// The orphan test — a `session.json` under a scratch subdirectory — matches
/// the boot scan's definition exactly, and the incomplete half delegates to
/// [`dat0_core::recovery_scan::scan_incomplete_workspaces`], so the panel and
/// the boot banner can never disagree about what counts as recoverable.
///
/// A row's table list is best-effort: an unparseable `session.json` still
/// yields a row with no tables, because it is still recoverable and still
/// discardable, and hiding it would strand the directory forever.
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
    // Stable order: `read_dir` is filesystem order, which is arbitrary and
    // differs between machines. A recovery list that reshuffles between
    // launches is a list whose rows cannot be described to anyone.
    rows.sort_by(|a, b| key_of(a).cmp(key_of(b)));
    for root in dat0_core::recovery_scan::scan_incomplete_workspaces(recent_roots) {
        rows.push(RecoveryRow::Incomplete { root });
    }
    rows
}

fn key_of(row: &RecoveryRow) -> &Path {
    match row {
        RecoveryRow::Orphan { dir, .. } => dir,
        RecoveryRow::Incomplete { root } => root,
    }
}

/// The label for a row.
fn label_of(row: &RecoveryRow) -> String {
    match row {
        RecoveryRow::Orphan { tables, .. } => tables.join(", "),
        RecoveryRow::Incomplete { root } => format!(
            "{} {}",
            root.display(),
            dat0_i18n::t("recovery.row.incomplete_suffix")
        ),
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct RecoveryPanelProps {
    /// The scratch root to scan for orphan sessions.
    pub scratch_root: PathBuf,
    /// The recent workspace roots to scan for interrupted promotions.
    pub recent_roots: Vec<PathBuf>,
    /// Bring an orphan session back live. The host owns window spawning.
    pub on_open: EventHandler<PathBuf>,
    /// Adopt an interrupted workspace. The host owns window spawning.
    pub on_resume: EventHandler<PathBuf>,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn RecoveryPanel(props: RecoveryPanelProps) -> Element {
    let scratch = props.scratch_root.clone();
    let recents = props.recent_roots.clone();
    let mut rows = use_signal(|| collect_rows(&scratch, &recents));

    // Discard rescans rather than removing the row locally, so the list can
    // never claim something is recoverable after the directory behind it is
    // gone. The GPUI build got this by closing and re-opening the Sheet.
    let on_close = props.on_close;
    let rescan = use_callback({
        let scratch = props.scratch_root.clone();
        let recents = props.recent_roots.clone();
        move |()| {
            let fresh = collect_rows(&scratch, &recents);
            let empty = fresh.is_empty();
            rows.set(fresh);
            // Nothing left to recover is the same state as never having had
            // anything: the panel does not exist.
            if empty {
                on_close.call(());
            }
        }
    });

    let current = rows.read().clone();
    if current.is_empty() {
        // No panel at all when there is nothing to recover — the GPUI entry
        // point returned early rather than showing an empty Sheet, and an
        // empty recovery list is alarming for no reason.
        return rsx! {};
    }

    let orphans: Vec<(usize, RecoveryRow)> = current
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, r)| matches!(r, RecoveryRow::Orphan { .. }))
        .collect();
    let incompletes: Vec<(usize, RecoveryRow)> = current
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, r)| matches!(r, RecoveryRow::Incomplete { .. }))
        .collect();

    rsx! {
        div { class: "d0-recovery", "data-a11y-id": "recovery",

            // Each heading is emitted only when its group has rows: a heading
            // with nothing under it reads as a failure to load.
            if !orphans.is_empty() {
                div {
                    class: "d0-label",
                    "data-a11y-id": "recovery-section-orphans",
                    {dat0_i18n::t("recovery.sheet.orphans")}
                }
                for (i, row) in orphans {
                    Row { key: "{i}", index: i, row, rescan,
                        on_open: props.on_open, on_resume: props.on_resume,
                        on_close: props.on_close }
                }
            }

            if !incompletes.is_empty() {
                div {
                    class: "d0-label",
                    "data-a11y-id": "recovery-section-incomplete",
                    {dat0_i18n::t("recovery.sheet.incomplete")}
                }
                for (i, row) in incompletes {
                    Row { key: "{i}", index: i, row, rescan,
                        on_open: props.on_open, on_resume: props.on_resume,
                        on_close: props.on_close }
                }
            }
        }
    }
}

/// One recovery row: a label plus its primary verb and Discard.
///
/// `rescan` re-derives the whole list from disk. It is not "remove this row":
/// a discard that failed must leave the row in place, and the only authority
/// on what is still recoverable is the filesystem.
#[component]
fn Row(
    index: usize,
    row: RecoveryRow,
    rescan: EventHandler<()>,
    on_open: EventHandler<PathBuf>,
    on_resume: EventHandler<PathBuf>,
    on_close: EventHandler<()>,
) -> Element {
    let label = label_of(&row);
    let (primary_id, primary_label) = match row {
        RecoveryRow::Orphan { .. } => ("recovery-open", dat0_i18n::t("recovery.row.open")),
        RecoveryRow::Incomplete { .. } => ("recovery-resume", dat0_i18n::t("recovery.row.resume")),
    };

    let restore = {
        let row = row.clone();
        move |_| {
            // Close first, then hand off: the panel must not still be covering
            // the window that is about to come forward.
            on_close.call(());
            match &row {
                RecoveryRow::Orphan { dir, .. } => on_open.call(dir.clone()),
                RecoveryRow::Incomplete { root } => on_resume.call(root.clone()),
            }
        }
    };

    let drop_it = {
        let row = row.clone();
        move |_| {
            let outcome = match &row {
                RecoveryRow::Orphan { dir, .. } => discard(dir),
                RecoveryRow::Incomplete { root } => discard_incomplete(root),
            };
            if let Err(e) = outcome {
                // A failed discard leaves the row in place on the rescan, so
                // the user can retry or restore instead. Losing the row here
                // would strand the directory with no way back to it.
                tracing::warn!(error = %e, "recovery: discard failed");
            }
            rescan.call(());
        }
    };

    rsx! {
        div { class: "d0-recovery-row", "data-a11y-id": "recovery-row-{index}",
            span { class: "d0-mono d0-recovery-label", "{label}" }
            div { class: "d0-recovery-actions",
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "{primary_id}-{index}",
                    role: AccessRole::Button.aria(),
                    "aria-label": "{primary_label}",
                    onclick: restore,
                    "{primary_label}"
                }
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "recovery-discard-{index}",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("recovery.row.discard"),
                    onclick: drop_it,
                    {dat0_i18n::t("recovery.row.discard")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_orphan(scratch: &Path, name: &str, json: &str) -> PathBuf {
        let dir = scratch.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("session.json"), json).unwrap();
        dir
    }

    #[test]
    fn an_unparseable_session_still_yields_a_discardable_row() {
        let tmp = tempfile::TempDir::new().unwrap();
        seed_orphan(tmp.path(), "broken", "{ not json");

        let rows = collect_rows(tmp.path(), &[]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], RecoveryRow::Orphan { tables, .. } if tables.is_empty()));
    }

    #[test]
    fn a_scratch_dir_without_a_session_is_not_recoverable() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("empty")).unwrap();

        assert!(collect_rows(tmp.path(), &[]).is_empty());
    }

    #[test]
    fn discarding_an_incomplete_workspace_spares_the_users_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("project");
        fs::create_dir_all(root.join(".dat0")).unwrap();
        fs::write(root.join(".dat0/workspace.duckdb"), b"db").unwrap();
        let precious = root.join("sales.csv");
        fs::write(&precious, b"a,b\n1,2\n").unwrap();

        discard_incomplete(&root).unwrap();

        assert!(!root.join(".dat0").exists(), "the half-promotion goes");
        assert!(precious.exists(), "the user's data must survive a discard");
        assert!(root.exists(), "so must their folder");
    }

    #[test]
    fn rows_are_ordered_the_same_way_on_every_scan() {
        let tmp = tempfile::TempDir::new().unwrap();
        for n in ["c", "a", "b"] {
            seed_orphan(
                tmp.path(),
                n,
                r#"{"tabs":[{"table_name":"t","source_path":null}]}"#,
            );
        }
        let names: Vec<String> = collect_rows(tmp.path(), &[])
            .iter()
            .map(|r| {
                key_of(r)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["a", "b", "c"]);
    }
}
