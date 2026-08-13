//! The File → Export… dialog.
//!
//! Ported from `view/export_dialog.rs` plus the half of `window/data_io.rs`
//! that decided the file name. Two radio groups (format, scope) drive one
//! request; the engine `COPY` and the file picker stay with the caller.
//!
//! # What moved, and why
//!
//! GPUI's dialog had no name field at all: it emitted `ExportEvent::Export`
//! and `run_export` built `format!("export.{ext}")` as the *suggestion* for the
//! native save panel, which then owned both the directory and the final name.
//! `dioxus-desktop` ships no save panel, `rfd` is another agent's wiring, and a
//! callback that returns a whole path would put the name outside the dialog
//! that knows which format it is writing.
//!
//! So the dialog owns the name and the caller owns only the directory
//! ([`ExportDialogProps::on_browse`]). The field holds a **stem**; the
//! extension is rendered beside it and derived from the selected format, which
//! makes "Parquet written to `data.csv`" unrepresentable rather than merely
//! discouraged. `default_name` is still the GPUI suggestion, minus its dot.

use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_core::view::export_dialog::ExportScope;
use dat0_engine::types::ExportFormat;

use crate::a11y::AccessRole;

/// The formats, in radio order. Index 0 is the default (CSV), as in GPUI.
pub const FORMATS: [ExportFormat; 3] =
    [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Parquet];

/// The scopes, in radio order. Index 0 is the default (current view).
pub const SCOPES: [ExportScope; 2] = [ExportScope::CurrentView, ExportScope::FullTable];

/// Cycle an index within `len`, wrapping in both directions.
///
/// Radio groups WRAP (the WAI-ARIA radiogroup convention); the list surfaces
/// in this crate deliberately clamp instead. A 2- or 3-item group that
/// dead-ends is worse than one that cycles.
pub fn cycle_ix(cur: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (cur as isize + delta).rem_euclid(len as isize) as usize
}

/// The file extension a format writes. The engine picks the writer from the
/// `ExportFormat`, so this is only ever about what other tools will believe.
pub fn extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "csv",
        ExportFormat::Json => "json",
        ExportFormat::Parquet => "parquet",
    }
}

/// The i18n key naming a format in the radio group.
fn format_key(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "export.format.csv",
        ExportFormat::Json => "export.format.json",
        ExportFormat::Parquet => "export.format.parquet",
    }
}

/// The i18n key naming a scope in the radio group.
fn scope_key(scope: ExportScope) -> &'static str {
    match scope {
        ExportScope::CurrentView => "export.scope.current",
        ExportScope::FullTable => "export.scope.full",
    }
}

/// The default file stem. `run_export` suggested `export.{ext}`; the extension
/// is now derived, so only the stem survives here.
pub const DEFAULT_STEM: &str = "export";

/// Why a file stem was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but whitespace. `run_export` would have opened a save panel
    /// with no name; the dialog says so instead.
    Empty,
    /// Contains `/` or `\`. A save-panel name is a leaf, not a path: silently
    /// accepting one would write somewhere the destination row does not name.
    Separator,
    /// Contains a control character — a newline pasted out of a spreadsheet
    /// cell is the common way this happens, and most filesystems accept it.
    Control,
}

impl NameError {
    /// The localised message shown under the field.
    pub fn message(self) -> String {
        match self {
            NameError::Empty => dat0_i18n::t("export.name.empty"),
            NameError::Separator => dat0_i18n::t("export.name.separator"),
            NameError::Control => dat0_i18n::t("export.name.control"),
        }
    }
}

/// Check a file stem. Trimmed, because the trimmed form is what
/// [`file_name`] builds with.
pub fn validate_stem(stem: &str) -> Result<(), NameError> {
    let t = stem.trim();
    if t.is_empty() {
        return Err(NameError::Empty);
    }
    if t.contains('/') || t.contains('\\') {
        return Err(NameError::Separator);
    }
    if t.chars().any(char::is_control) {
        return Err(NameError::Control);
    }
    Ok(())
}

/// The file name a stem and a format produce.
pub fn file_name(stem: &str, format: ExportFormat) -> String {
    format!("{}.{}", stem.trim(), extension(format))
}

/// What the dialog asks the caller to do. The engine `COPY` is the caller's:
/// the dialog never touches a query, exactly as the GPUI dialog never did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub scope: ExportScope,
    pub format: ExportFormat,
    /// The full destination, `destination.join(file_name(..))`.
    pub path: PathBuf,
}

#[derive(Clone, Props, PartialEq)]
pub struct ExportDialogProps {
    /// The destination directory, chosen by the caller's file picker. `None`
    /// until [`on_browse`](Self::on_browse) has produced one — Export is
    /// disabled meanwhile, because there is nowhere to write.
    #[props(default)]
    pub destination: Option<PathBuf>,
    /// Ask the caller to run the directory picker. `rfd` is wired outside this
    /// component so the dialog stays testable without a windowing system.
    pub on_browse: EventHandler<()>,
    pub on_export: EventHandler<ExportRequest>,
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn ExportDialog(props: ExportDialogProps) -> Element {
    let mut format_ix = use_signal(|| 0usize);
    let mut scope_ix = use_signal(|| 0usize);
    let mut stem = use_signal(|| DEFAULT_STEM.to_string());

    let format = FORMATS[format_ix()];
    let scope = SCOPES[scope_ix()];
    let ext = extension(format);
    let name_error = validate_stem(&stem()).err();

    let destination = props.destination.clone();
    let ready = destination.is_some() && name_error.is_none();

    let on_export = props.on_export;
    let on_cancel = props.on_cancel;
    let on_browse = props.on_browse;
    let dest_for_export = destination.clone();
    let submit = move |_| {
        // Belt and braces: the button is disabled in this state, but a
        // keyboard activation on a stale render must not write to a directory
        // that is no longer chosen.
        let Some(dir) = dest_for_export.clone() else {
            return;
        };
        let stem_now = stem();
        if validate_stem(&stem_now).is_err() {
            return;
        }
        on_export.call(ExportRequest {
            scope,
            format,
            path: dir.join(file_name(&stem_now, format)),
        });
    };

    rsx! {
        div { class: "d0-export", "data-a11y-id": "export-dialog",

            // ── Format ──────────────────────────────────────────────────
            div { class: "d0-label", "{dat0_i18n::t(\"export.format\")}" }
            div {
                class: "d0-radiogroup",
                "data-a11y-id": "export-format-group",
                role: "radiogroup",
                "aria-label": dat0_i18n::t("export.format"),
                // ONE tab stop for the whole group; Left/Right move the
                // selection. Per-radio tab stops would make a three-item
                // choice cost three Tabs, which is why GPUI set
                // `.tab_stop(false)` on every child radio.
                tabindex: "0",
                onkeydown: move |e| {
                    let delta = match e.key() {
                        Key::ArrowLeft => -1,
                        Key::ArrowRight => 1,
                        _ => return,
                    };
                    e.prevent_default();
                    format_ix.set(cycle_ix(format_ix(), FORMATS.len(), delta));
                },
                for (i , f) in FORMATS.iter().copied().enumerate() {
                    button {
                        key: "{i}",
                        class: if i == format_ix() { "d0-radio is-selected" } else { "d0-radio" },
                        "data-a11y-id": "export-format-{extension(f)}",
                        role: "radio",
                        "aria-checked": if i == format_ix() { "true" } else { "false" },
                        "aria-label": dat0_i18n::t(format_key(f)),
                        tabindex: "-1",
                        onclick: move |_| format_ix.set(i),
                        "{dat0_i18n::t(format_key(f))}"
                    }
                }
            }

            // ── Scope ───────────────────────────────────────────────────
            div { class: "d0-label", "{dat0_i18n::t(\"export.scope\")}" }
            div {
                class: "d0-radiogroup is-vertical",
                "data-a11y-id": "export-scope-group",
                role: "radiogroup",
                "aria-label": dat0_i18n::t("export.scope"),
                tabindex: "0",
                onkeydown: move |e| {
                    let delta = match e.key() {
                        Key::ArrowUp => -1,
                        Key::ArrowDown => 1,
                        _ => return,
                    };
                    e.prevent_default();
                    scope_ix.set(cycle_ix(scope_ix(), SCOPES.len(), delta));
                },
                for (i , s) in SCOPES.iter().copied().enumerate() {
                    button {
                        key: "{i}",
                        class: if i == scope_ix() { "d0-radio is-selected" } else { "d0-radio" },
                        "data-a11y-id": if matches!(s, ExportScope::CurrentView) { "export-scope-current" } else { "export-scope-full" },
                        role: "radio",
                        "aria-checked": if i == scope_ix() { "true" } else { "false" },
                        "aria-label": dat0_i18n::t(scope_key(s)),
                        tabindex: "-1",
                        onclick: move |_| scope_ix.set(i),
                        "{dat0_i18n::t(scope_key(s))}"
                    }
                }
            }

            // ── Destination ─────────────────────────────────────────────
            div { class: "d0-label", "{dat0_i18n::t(\"export.destination\")}" }
            div { class: "d0-form-row",
                span {
                    class: if destination.is_some() { "d0-mono d0-export-dest" } else { "d0-mono d0-export-dest is-unset" },
                    "data-a11y-id": "export-destination",
                    match destination.as_ref() {
                        Some(d) => d.display().to_string(),
                        None => dat0_i18n::t("export.destination.unset"),
                    }
                }
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "export-browse",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("export.browse"),
                    onclick: move |_| on_browse.call(()),
                    "{dat0_i18n::t(\"export.browse\")}"
                }
            }

            // ── File name ───────────────────────────────────────────────
            div { class: "d0-label", "{dat0_i18n::t(\"export.filename\")}" }
            div { class: "d0-form-row",
                input {
                    class: "d0-field",
                    "data-a11y-id": "export-name",
                    role: "textbox",
                    "aria-label": dat0_i18n::t("export.filename"),
                    "aria-invalid": if name_error.is_some() { "true" } else { "false" },
                    value: "{stem}",
                    oninput: move |e| stem.set(e.value()),
                }
                // The extension is shown, never typed: it follows the format,
                // so the written bytes and the name cannot disagree.
                span { class: "d0-mono d0-export-ext", "data-a11y-id": "export-extension", ".{ext}" }
            }
            if let Some(err) = name_error {
                div {
                    class: "d0-form-error d0-mono",
                    "data-a11y-id": "export-name-error",
                    role: AccessRole::Alert.aria(),
                    "aria-label": err.message(),
                    "{err.message()}"
                }
            }

            // ── Actions ─────────────────────────────────────────────────
            div { class: "d0-form-actions",
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "export-run",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("export.run"),
                    disabled: !ready,
                    onclick: submit,
                    "{dat0_i18n::t(\"export.run\")}"
                }
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "export-cancel",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("export.cancel"),
                    onclick: move |_| on_cancel.call(()),
                    "{dat0_i18n::t(\"export.cancel\")}"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_ix_wraps_both_ways() {
        assert_eq!(cycle_ix(0, 3, 1), 1);
        assert_eq!(cycle_ix(2, 3, 1), 0, "last wraps to first");
        assert_eq!(cycle_ix(0, 3, -1), 2, "first wraps to last");
        assert_eq!(cycle_ix(0, 0, 1), 0, "an empty group cannot panic");
    }

    #[test]
    fn the_name_carries_the_format_that_wrote_it() {
        assert_eq!(file_name("export", ExportFormat::Csv), "export.csv");
        assert_eq!(file_name(" q2 ", ExportFormat::Parquet), "q2.parquet");
        assert_eq!(file_name("orders", ExportFormat::Json), "orders.json");
    }
}
