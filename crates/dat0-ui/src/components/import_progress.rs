//! The import strip: what a dropped file is doing, and how to stop it.
//!
//! `dat0_core::import_progress` is the whole of the model — one process-wide
//! `ImportProgress` (byte counter + cancel flag), because dat0 runs one import
//! at a time. Under GPUI it had no view at all: `IMPORT_CANCEL` could flip the
//! flag from the palette or the menu, but nothing on screen said an import was
//! running, how far it had got, or that one had failed. The only feedback was
//! a Banner *after* the fact.
//!
//! This is that view. The terminal states are exactly
//! [`dat0_core::file_drop::DropOutcome`]'s — mapped one-for-one by
//! [`ImportState::from_outcome`] rather than re-derived, so a new outcome
//! variant is a compile error here instead of a silently unreported import.

use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use dat0_core::file_drop::DropOutcome;
use dat0_core::import_progress;

use crate::a11y::{AccessRole, format_swatch};

/// What the strip is showing.
///
/// One enum rather than a bag of `Option`s: an import cannot be both running
/// and failed, and the states that share a shape (`Cancelled`, `Unsupported`)
/// still read differently to a user, so they stay distinct.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum ImportState {
    /// Nothing is importing. The strip renders nothing at all.
    #[default]
    Idle,
    /// In flight. `total == 0` means the size is unknown — the bar goes
    /// indeterminate rather than claiming 0 %.
    Running {
        file: PathBuf,
        done: u64,
        total: u64,
    },
    /// Registered, with the table it landed in.
    Done { file: PathBuf, table: String },
    /// The user pressed cancel (or `import.cancel` fired) mid-flight.
    Cancelled { file: PathBuf },
    /// The extension is not one dat0 reads.
    Unsupported {
        file: PathBuf,
        extension: Option<String>,
    },
    /// The engine refused the file.
    Failed { file: PathBuf, error: String },
}

impl ImportState {
    /// The terminal state for one finished drop.
    ///
    /// Exhaustive on purpose: adding a `DropOutcome` variant must force a
    /// decision about how it reads to the user, not default to silence.
    pub fn from_outcome(outcome: &DropOutcome) -> Self {
        match outcome {
            DropOutcome::Registered {
                table_name,
                source_path,
            } => ImportState::Done {
                file: source_path.clone(),
                table: table_name.clone(),
            },
            DropOutcome::Unsupported { path, extension } => ImportState::Unsupported {
                file: path.clone(),
                extension: extension.clone(),
            },
            DropOutcome::EngineError { path, error } => ImportState::Failed {
                file: path.clone(),
                error: error.clone(),
            },
            // The wizard takes over from here; the strip has nothing to say
            // while a modal is asking the user about dialect.
            DropOutcome::OpenWizard { .. } => ImportState::Idle,
            DropOutcome::Cancelled { path } => ImportState::Cancelled { file: path.clone() },
        }
    }

    /// A `Running` state sampled from the process-wide active import.
    ///
    /// Returns `Idle` when nothing is active, so a poll that races the import
    /// finishing clears the strip instead of freezing it at 97 %.
    pub fn from_active(file: &Path) -> Self {
        match import_progress::active() {
            Some(p) => ImportState::Running {
                file: file.to_path_buf(),
                done: p.bytes_done(),
                total: p.total_bytes(),
            },
            None => ImportState::Idle,
        }
    }

    /// The file this state is about, if any.
    pub fn file(&self) -> Option<&Path> {
        match self {
            ImportState::Idle => None,
            ImportState::Running { file, .. }
            | ImportState::Done { file, .. }
            | ImportState::Cancelled { file }
            | ImportState::Unsupported { file, .. }
            | ImportState::Failed { file, .. } => Some(file),
        }
    }

    /// Completion as a whole percent, or `None` when the state is not running
    /// or the total size is unknown.
    ///
    /// Clamped at 100: a `total` read before the file finished growing would
    /// otherwise render a 140 % bar.
    pub fn percent(&self) -> Option<u8> {
        match self {
            ImportState::Running { done, total, .. } if *total > 0 => {
                Some((done.saturating_mul(100) / total).min(100) as u8)
            }
            _ => None,
        }
    }

    /// Can this state still be cancelled?
    pub fn is_cancellable(&self) -> bool {
        matches!(self, ImportState::Running { .. })
    }

    /// Is this a failure the user must be told about? Drives `role="alert"`,
    /// which is the difference between a screen reader interrupting and the
    /// user never learning the import died.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            ImportState::Failed { .. } | ImportState::Unsupported { .. }
        )
    }

    /// Whether the strip offers a dismiss button — every state that will not
    /// change on its own.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, ImportState::Idle | ImportState::Running { .. })
    }

    /// The line the strip shows, already localised.
    pub fn message(&self) -> String {
        match self {
            ImportState::Idle => String::new(),
            ImportState::Running { done, total, .. } => match self.percent() {
                Some(p) => format!("{} · {p}%", dat0_i18n::t("import.running")),
                // Unknown total: report the bytes seen so far rather than a
                // fabricated fraction.
                None => format!(
                    "{} · {}",
                    dat0_i18n::t("import.running"),
                    human_bytes(if *total == 0 { *done } else { *total })
                ),
            },
            ImportState::Done { table, .. } => format!("{} {table}", dat0_i18n::t("import.done")),
            ImportState::Cancelled { .. } => dat0_i18n::t("import.cancelled"),
            ImportState::Unsupported { extension, .. } => match extension {
                Some(e) => format!("{} .{e}", dat0_i18n::t("import.unsupported")),
                None => dat0_i18n::t("import.unsupported.no_extension"),
            },
            ImportState::Failed { error, .. } => {
                format!("{} {error}", dat0_i18n::t("import.failed"))
            }
        }
    }
}

/// Bytes at one decimal place, binary units. Shared with the AI panel's egress
/// readout so the two never disagree about what "1.5 KB" means.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

/// The file's name, or its whole path when it has none.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[derive(Clone, PartialEq, Props)]
pub struct ImportProgressProps {
    pub state: ImportState,
    /// The user asked to stop. Fired *after* the cancel flag is set, so a host
    /// that only wants to clear its own state need do nothing else.
    pub on_cancel: EventHandler<()>,
    /// The user dismissed a terminal state.
    pub on_dismiss: EventHandler<()>,
}

/// The import strip.
#[component]
pub fn ImportProgress(props: ImportProgressProps) -> Element {
    let state = props.state.clone();
    if state == ImportState::Idle {
        return rsx! {};
    }

    let file = state.file().map(file_label).unwrap_or_default();
    let swatch = state.file().map(format_swatch).unwrap_or("sw-other");
    let percent = state.percent();
    let failure = state.is_failure();
    let message = state.message();

    let kind = match &state {
        ImportState::Failed { .. } | ImportState::Unsupported { .. } => "is-error",
        ImportState::Cancelled { .. } => "is-warning",
        ImportState::Done { .. } => "is-ok",
        _ => "is-running",
    };

    rsx! {
        div {
            class: "d0-import {kind}",
            "data-a11y-id": "import-progress",
            role: if failure { AccessRole::Alert.aria() } else { AccessRole::Label.aria() },
            "aria-label": "{message}",
            // Announce a running import's progress without re-reading the whole
            // strip on every byte.
            "aria-live": if failure { "assertive" } else { "polite" },

            span { class: "d0-sw {swatch}" }
            span { class: "d0-import-file d0-mono", "{file}" }
            span { class: "d0-import-msg d0-mono", "{message}" }

            if state.is_cancellable() {
                div {
                    class: "d0-import-bar",
                    "data-a11y-id": "import-bar",
                    role: "progressbar",
                    "aria-valuemin": "0",
                    "aria-valuemax": "100",
                    // Omitted entirely when the size is unknown: an ARIA
                    // progressbar without `aria-valuenow` is *defined* as
                    // indeterminate, and 0 would read as "no progress".
                    "aria-valuenow": if let Some(p) = percent { p.to_string() },
                    div {
                        class: if percent.is_some() { "d0-import-fill" } else { "d0-import-fill is-indeterminate" },
                        style: if let Some(p) = percent { format!("width: {p}%") } else { String::new() },
                    }
                }
                button {
                    class: "d0-btn",
                    "data-a11y-id": "import-cancel",
                    "aria-label": dat0_i18n::t("import.cancel"),
                    onclick: move |_| {
                        // The same entry point `ids::IMPORT_CANCEL` uses, so the
                        // button and the action cannot diverge: it sets the flag,
                        // pushes the warning Banner and clears the active import.
                        import_progress::cancel_active();
                        props.on_cancel.call(());
                    },
                    {dat0_i18n::t("import.cancel")}
                }
            }

            if state.is_terminal() {
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "import-dismiss",
                    "aria-label": dat0_i18n::t("common.close"),
                    onclick: move |_| props.on_dismiss.call(()),
                    {dat0_i18n::t("common.close")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_total_is_indeterminate_rather_than_zero_percent() {
        let s = ImportState::Running {
            file: PathBuf::from("a.csv"),
            done: 512,
            total: 0,
        };
        assert_eq!(s.percent(), None);
    }

    #[test]
    fn progress_never_exceeds_one_hundred() {
        // `total` is the file size read before the import; a file that grew
        // mid-import would otherwise render past the end of the bar.
        let s = ImportState::Running {
            file: PathBuf::from("a.csv"),
            done: 300,
            total: 100,
        };
        assert_eq!(s.percent(), Some(100));
    }

    #[test]
    fn only_a_running_import_can_be_cancelled() {
        assert!(
            ImportState::Running {
                file: PathBuf::from("a.csv"),
                done: 0,
                total: 1,
            }
            .is_cancellable()
        );
        assert!(!ImportState::Idle.is_cancellable());
        assert!(
            !ImportState::Done {
                file: PathBuf::from("a.csv"),
                table: "a".into(),
            }
            .is_cancellable()
        );
    }
}
