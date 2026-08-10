//! The CSV import wizard: dialect, then column mapping, then confirm.
//!
//! `dat0_core::import_wizard` decides *whether* to open this
//! ([`should_show_wizard`] over the PD-011 dual-sniff summary); under GPUI
//! [`dat0_core::import_wizard::open`] then logged a line and returned — the
//! drawer was never built, so an ambiguous CSV silently imported nothing.
//! This is the drawer.
//!
//! The model is a plain struct with a pure validator, and the component is a
//! function of it. That split is deliberate: the gating rules are the part
//! worth testing, and they are testable without mounting anything.
//!
//! ## The rules, in one place
//!
//! **Dialect** — the delimiter and the quote character must each be exactly
//! one character, and they must differ (DuckDB rejects the pair otherwise, and
//! the error it gives names neither). A sniff that could not read the head as
//! UTF-8 blocks here too: DuckDB's CSV reader errors hard on non-UTF-8 input,
//! so advancing would buy the user a wizard's worth of typing followed by a
//! guaranteed failure.
//!
//! **Columns** — at least one column must be included; every *included*
//! column needs a non-empty name, a name that is unique case-insensitively
//! (DuckDB identifiers are case-insensitive, so `id` and `ID` collide), a name
//! that is not the engine's surrogate [`ROWID_COL`], and a type from
//! [`TYPES`]. Excluded columns are not validated — you cannot be wrong about a
//! column you are not importing — but they still appear in the generated
//! `columns := {…}` map, because DuckDB requires that map to describe the
//! whole file.
//!
//! **Confirm** — adds no rules of its own. `Next` is gated on the current
//! step alone; `Import` is gated on *every* step, so a late edit that breaks
//! step 1 cannot be imported from step 3.

use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use dat0_core::import_wizard::SniffSummary;
use dat0_engine::transform::ROWID_COL;

use crate::a11y::{AccessRole, format_swatch};

/// The types a column may be overridden to.
///
/// DuckDB logical type names, spelled as the CSV reader's `columns := {…}` map
/// wants them. Deliberately short: this is the set that survives a CSV round
/// trip, not everything DuckDB can hold.
pub const TYPES: [&str; 9] = [
    "VARCHAR",
    "BIGINT",
    "INTEGER",
    "DOUBLE",
    "BOOLEAN",
    "DATE",
    "TIMESTAMP",
    "TIME",
    "BLOB",
];

/// The wizard's three steps, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Dialect,
    Columns,
    Confirm,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Dialect, Step::Columns, Step::Confirm];

    pub fn index(self) -> usize {
        match self {
            Step::Dialect => 0,
            Step::Columns => 1,
            Step::Confirm => 2,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Step::Dialect => "dialect",
            Step::Columns => "columns",
            Step::Confirm => "confirm",
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Step::Dialect => "wizard.step.dialect",
            Step::Columns => "wizard.step.columns",
            Step::Confirm => "wizard.step.confirm",
        }
    }

    fn next(self) -> Option<Step> {
        Step::ALL.get(self.index() + 1).copied()
    }

    fn prev(self) -> Option<Step> {
        self.index()
            .checked_sub(1)
            .and_then(|i| Step::ALL.get(i).copied())
    }
}

/// One column, as the user is editing it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ColumnDraft {
    /// The name DuckDB sniffed. Never edited — it is what the generated
    /// `columns := {…}` map keys on, so renaming it would rename the file's
    /// column rather than the imported one.
    pub source: String,
    /// The name to import it as.
    pub name: String,
    /// The type to read it as, from [`TYPES`].
    pub ty: String,
    pub include: bool,
}

impl ColumnDraft {
    pub fn new(source: impl Into<String>, ty: impl Into<String>) -> Self {
        let source: String = source.into();
        let ty: String = ty.into();
        // An unrecognised sniffed type degrades to VARCHAR rather than
        // failing validation on a value the user never chose.
        let ty = if TYPES.contains(&ty.to_ascii_uppercase().as_str()) {
            ty.to_ascii_uppercase()
        } else {
            "VARCHAR".to_string()
        };
        Self {
            name: source.clone(),
            source,
            ty,
            include: true,
        }
    }
}

/// Something that blocks the step it belongs to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Issue {
    DelimiterEmpty,
    DelimiterNotSingleChar,
    QuoteNotSingleChar,
    DelimiterEqualsQuote,
    EncodingUnsupported,
    NoColumnsIncluded,
    EmptyColumnName { row: usize },
    DuplicateColumnName { row: usize, name: String },
    ReservedColumnName { row: usize },
    UnknownType { row: usize, ty: String },
}

impl Issue {
    /// The step this issue blocks.
    pub fn step(&self) -> Step {
        match self {
            Issue::DelimiterEmpty
            | Issue::DelimiterNotSingleChar
            | Issue::QuoteNotSingleChar
            | Issue::DelimiterEqualsQuote
            | Issue::EncodingUnsupported => Step::Dialect,
            _ => Step::Columns,
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Issue::DelimiterEmpty => "wizard.issue.delimiter_empty",
            Issue::DelimiterNotSingleChar => "wizard.issue.delimiter_not_single",
            Issue::QuoteNotSingleChar => "wizard.issue.quote_not_single",
            Issue::DelimiterEqualsQuote => "wizard.issue.delimiter_equals_quote",
            Issue::EncodingUnsupported => "wizard.issue.encoding_unsupported",
            Issue::NoColumnsIncluded => "wizard.issue.no_columns",
            Issue::EmptyColumnName { .. } => "wizard.issue.empty_name",
            Issue::DuplicateColumnName { .. } => "wizard.issue.duplicate_name",
            Issue::ReservedColumnName { .. } => "wizard.issue.reserved_name",
            Issue::UnknownType { .. } => "wizard.issue.unknown_type",
        }
    }

    /// The row this issue is about, 0-based, when it is about one.
    pub fn row(&self) -> Option<usize> {
        match self {
            Issue::EmptyColumnName { row }
            | Issue::DuplicateColumnName { row, .. }
            | Issue::ReservedColumnName { row }
            | Issue::UnknownType { row, .. } => Some(*row),
            _ => None,
        }
    }

    /// The localised line shown under the step.
    pub fn message(&self) -> String {
        let text = dat0_i18n::t(self.key());
        let detail = match self {
            Issue::DuplicateColumnName { name, .. } => Some(name.clone()),
            Issue::UnknownType { ty, .. } => Some(ty.clone()),
            Issue::ReservedColumnName { .. } => Some(ROWID_COL.to_string()),
            _ => None,
        };
        let body = match detail {
            Some(d) => format!("{text}: {d}"),
            None => text,
        };
        match self.row() {
            // 1-based in the message: the user is looking at a numbered list.
            Some(r) => format!("{} {} — {body}", dat0_i18n::t("wizard.column"), r + 1),
            None => body,
        }
    }
}

/// Everything the wizard is editing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WizardModel {
    pub path: PathBuf,
    pub delimiter: String,
    pub quote: String,
    pub has_header: bool,
    /// From the sniff. `false` means the first 8 KB were not UTF-8.
    pub encoding_supported: bool,
    pub columns: Vec<ColumnDraft>,
    pub step: Step,
}

impl WizardModel {
    /// Seed from the sniff that opened the wizard plus the columns DuckDB
    /// described for it.
    ///
    /// `columns` is `(name, duckdb_type)` — see [`describe_csv`], which is how
    /// the caller gets them, and which is re-run when the dialect changes.
    pub fn from_sniff(path: &Path, sniff: &SniffSummary, columns: Vec<(String, String)>) -> Self {
        Self {
            path: path.to_path_buf(),
            delimiter: sniff.top_delimiter.to_string(),
            quote: "\"".to_string(),
            has_header: true,
            encoding_supported: sniff.encoding_supported,
            columns: columns
                .into_iter()
                .map(|(n, t)| ColumnDraft::new(n, t))
                .collect(),
            step: Step::Dialect,
        }
    }

    /// Everything wrong with one step.
    pub fn issues(&self, step: Step) -> Vec<Issue> {
        match step {
            Step::Dialect => self.dialect_issues(),
            Step::Columns => self.column_issues(),
            // Confirm shows the union rather than owning rules of its own,
            // so the reason Import is disabled is visible where Import is.
            Step::Confirm => Vec::new(),
        }
    }

    /// Everything wrong, anywhere.
    pub fn all_issues(&self) -> Vec<Issue> {
        let mut v = self.dialect_issues();
        v.extend(self.column_issues());
        v
    }

    fn dialect_issues(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        // `chars`, not `len`: a multi-byte delimiter (a tab is not, but `;`
        // in some locales' exports is followed by nothing and `·` happens) is
        // one character even though it is three bytes.
        let mut d = self.delimiter.chars();
        match (d.next(), d.next()) {
            (None, _) => out.push(Issue::DelimiterEmpty),
            (Some(_), Some(_)) => out.push(Issue::DelimiterNotSingleChar),
            (Some(_), None) => {}
        }
        let mut q = self.quote.chars();
        if !matches!((q.next(), q.next()), (Some(_), None)) {
            out.push(Issue::QuoteNotSingleChar);
        }
        if !self.delimiter.is_empty() && self.delimiter == self.quote {
            out.push(Issue::DelimiterEqualsQuote);
        }
        if !self.encoding_supported {
            out.push(Issue::EncodingUnsupported);
        }
        out
    }

    fn column_issues(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        let included: Vec<(usize, &ColumnDraft)> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.include)
            .collect();
        if included.is_empty() {
            out.push(Issue::NoColumnsIncluded);
            return out;
        }
        let mut seen: Vec<String> = Vec::with_capacity(included.len());
        for (row, c) in included {
            let name = c.name.trim();
            if name.is_empty() {
                out.push(Issue::EmptyColumnName { row });
                continue;
            }
            let folded = name.to_ascii_lowercase();
            if folded == ROWID_COL.to_ascii_lowercase() {
                // The engine adds this column itself; a file column of the
                // same name makes every projection ambiguous.
                out.push(Issue::ReservedColumnName { row });
            }
            if seen.contains(&folded) {
                out.push(Issue::DuplicateColumnName {
                    row,
                    name: name.to_string(),
                });
            } else {
                seen.push(folded);
            }
            if !TYPES.contains(&c.ty.as_str()) {
                out.push(Issue::UnknownType {
                    row,
                    ty: c.ty.clone(),
                });
            }
        }
        out
    }

    /// May the user leave the current step forwards?
    pub fn can_advance(&self) -> bool {
        self.step.next().is_some() && self.issues(self.step).is_empty()
    }

    /// May the user go back? Always, except from the first step — a wizard
    /// that traps you on an invalid step you cannot leave is worse than one
    /// that lets you retreat.
    pub fn can_go_back(&self) -> bool {
        self.step.prev().is_some()
    }

    /// May the import run? Only from the last step, and only when *every*
    /// step is clean.
    pub fn can_import(&self) -> bool {
        self.step == Step::Confirm && self.all_issues().is_empty()
    }

    /// Advance, if the gate allows. Returns whether it moved.
    pub fn advance(&mut self) -> bool {
        if !self.can_advance() {
            return false;
        }
        match self.step.next() {
            Some(s) => {
                self.step = s;
                true
            }
            None => false,
        }
    }

    /// Retreat, if there is anywhere to retreat to.
    pub fn go_back(&mut self) -> bool {
        match self.step.prev() {
            Some(s) => {
                self.step = s;
                true
            }
            None => false,
        }
    }

    /// The columns that will be imported.
    pub fn included(&self) -> impl Iterator<Item = &ColumnDraft> {
        self.columns.iter().filter(|c| c.include)
    }

    /// The `read_csv` projection this wizard describes.
    ///
    /// `columns := {…}` must name **every** column in the file — DuckDB reads
    /// it as the file's schema, not as a filter — so excluded columns appear
    /// there and are dropped by the SELECT list instead.
    ///
    /// Returns `None` when the model does not validate, so a caller cannot
    /// build SQL out of a half-filled wizard.
    pub fn read_csv_sql(&self) -> Option<String> {
        if !self.all_issues().is_empty() {
            return None;
        }
        let projection = self
            .included()
            .map(|c| {
                format!(
                    "{} AS {}",
                    quote_ident(&c.source),
                    quote_ident(c.name.trim())
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let columns = self
            .columns
            .iter()
            .map(|c| format!("{}: {}", quote_str(&c.source), quote_str(&c.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "SELECT {projection} FROM read_csv({path}, delim = {delim}, quote = {quote}, \
             header = {header}, columns = {{{columns}}})",
            path = quote_str(&self.path.to_string_lossy()),
            delim = quote_str(&self.delimiter),
            quote = quote_str(&self.quote),
            header = self.has_header,
        ))
    }
}

/// A SQL single-quoted literal.
fn quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A SQL double-quoted identifier.
fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Ask DuckDB what columns a CSV has under a given dialect.
///
/// A side-channel in-memory connection, for the same reason
/// `dat0_core::import_wizard::sniff` opens one: `DESCRIBE` is read-only and
/// contention-free this way, and it never touches the workspace engine's
/// mutex.
///
/// Blocking. Callers on the tokio runtime must wrap it in `spawn_blocking`.
pub fn describe_csv(
    path: &Path,
    delimiter: &str,
    quote: &str,
    has_header: bool,
) -> anyhow::Result<Vec<(String, String)>> {
    use anyhow::Context as _;
    let conn =
        duckdb::Connection::open_in_memory().context("open in-memory duckdb for DESCRIBE")?;
    let path_str = path.to_str().context("describe_csv: path is not UTF-8")?;
    let mut stmt = conn.prepare(
        "DESCRIBE SELECT * FROM read_csv(?, delim := ?, quote := ?, header := ?, \
         all_varchar := false)",
    )?;
    let rows = stmt.query_map(
        duckdb::params![path_str, delimiter, quote, has_header],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[derive(Clone, PartialEq, Props)]
pub struct ImportWizardProps {
    /// Owned by the host so a re-render cannot reset the user's edits.
    pub model: Signal<WizardModel>,
    /// The user confirmed. The model is guaranteed to validate.
    pub on_import: EventHandler<WizardModel>,
    pub on_cancel: EventHandler<()>,
}

/// The wizard.
#[component]
pub fn ImportWizard(props: ImportWizardProps) -> Element {
    let mut model = props.model;
    let m = model.read().clone();
    let step = m.step;
    let issues = m.issues(step);
    let blocking = if step == Step::Confirm {
        m.all_issues()
    } else {
        issues.clone()
    };
    let file = m
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    rsx! {
        div {
            class: "d0-wizard",
            "data-a11y-id": "import-wizard",
            role: AccessRole::Dialog.aria(),
            "aria-label": dat0_i18n::t("wizard.title"),

            div { class: "d0-wizard-head",
                span { class: "d0-sw {format_swatch(&m.path)}" }
                span { class: "d0-head-title", "{dat0_i18n::t(\"wizard.title\")}" }
                span { class: "d0-label", "{file}" }
            }

            // The step rail. `aria-current` rather than a class alone: it is
            // what tells a reader which of the three you are on.
            div { class: "d0-wizard-steps", role: "list",
                for s in Step::ALL {
                    div {
                        key: "{s.id()}",
                        role: "listitem",
                        class: if s == step { "d0-wizard-step is-active" } else { "d0-wizard-step" },
                        "data-a11y-id": "wizard-step-{s.id()}",
                        "aria-current": if s == step { "step" },
                        "{s.index() + 1}. {dat0_i18n::t(s.title_key())}"
                    }
                }
            }

            div { class: "d0-wizard-body",
                match step {
                    Step::Dialect => rsx! { DialectStep { model } },
                    Step::Columns => rsx! { ColumnsStep { model } },
                    Step::Confirm => rsx! { ConfirmStep { model } },
                }
            }

            if !blocking.is_empty() {
                ul {
                    class: "d0-wizard-issues",
                    "data-a11y-id": "wizard-issues",
                    role: AccessRole::Alert.aria(),
                    "aria-label": dat0_i18n::t("wizard.issues"),
                    for (i, issue) in blocking.iter().enumerate() {
                        li { key: "{i}", class: "d0-mono", "{issue.message()}" }
                    }
                }
            }

            div { class: "d0-wizard-foot",
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "wizard-cancel",
                    "aria-label": dat0_i18n::t("common.cancel"),
                    onclick: move |_| props.on_cancel.call(()),
                    {dat0_i18n::t("common.cancel")}
                }
                span { class: "d0-spacer" }
                button {
                    class: "d0-btn",
                    "data-a11y-id": "wizard-back",
                    "aria-label": dat0_i18n::t("common.back"),
                    "aria-disabled": if m.can_go_back() { "false" } else { "true" },
                    onclick: move |_| { model.write().go_back(); },
                    {dat0_i18n::t("common.back")}
                }
                if step == Step::Confirm {
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "wizard-import",
                        "aria-label": dat0_i18n::t("wizard.import"),
                        "aria-disabled": if m.can_import() { "false" } else { "true" },
                        onclick: move |_| {
                            let m = model.read().clone();
                            // Re-checked at the click rather than trusted from
                            // the last render: the button is aria-disabled, not
                            // disabled, so it is still clickable by design.
                            if m.can_import() {
                                props.on_import.call(m);
                            }
                        },
                        {dat0_i18n::t("wizard.import")}
                    }
                } else {
                    button {
                        class: "d0-btn is-primary",
                        "data-a11y-id": "wizard-next",
                        "aria-label": dat0_i18n::t("wizard.next"),
                        "aria-disabled": if m.can_advance() { "false" } else { "true" },
                        onclick: move |_| { model.write().advance(); },
                        {dat0_i18n::t("wizard.next")}
                    }
                }
            }
        }
    }
}

#[component]
fn DialectStep(model: Signal<WizardModel>) -> Element {
    let m = model.read().clone();
    rsx! {
        div { class: "d0-wizard-fields",
            label { class: "d0-wizard-field",
                span { class: "d0-label", "{dat0_i18n::t(\"wizard.delimiter\")}" }
                input {
                    class: "d0-field d0-mono",
                    "data-a11y-id": "wizard-delimiter",
                    "aria-label": dat0_i18n::t("wizard.delimiter"),
                    value: "{m.delimiter}",
                    oninput: move |e| model.write().delimiter = e.value(),
                }
            }
            label { class: "d0-wizard-field",
                span { class: "d0-label", "{dat0_i18n::t(\"wizard.quote\")}" }
                input {
                    class: "d0-field d0-mono",
                    "data-a11y-id": "wizard-quote",
                    "aria-label": dat0_i18n::t("wizard.quote"),
                    value: "{m.quote}",
                    oninput: move |e| model.write().quote = e.value(),
                }
            }
            label { class: "d0-wizard-field is-inline",
                input {
                    r#type: "checkbox",
                    "data-a11y-id": "wizard-header",
                    "aria-label": dat0_i18n::t("wizard.header"),
                    checked: m.has_header,
                    onchange: move |e| model.write().has_header = e.checked(),
                }
                span { class: "d0-mono", "{dat0_i18n::t(\"wizard.header\")}" }
            }
        }
    }
}

#[component]
fn ColumnsStep(model: Signal<WizardModel>) -> Element {
    let m = model.read().clone();
    rsx! {
        div { class: "d0-wizard-cols", "data-a11y-id": "wizard-columns",
            div { class: "d0-wizard-col is-head d0-colhead",
                span { "{dat0_i18n::t(\"wizard.column.include\")}" }
                span { "{dat0_i18n::t(\"wizard.column.source\")}" }
                span { "{dat0_i18n::t(\"wizard.column.name\")}" }
                span { "{dat0_i18n::t(\"wizard.column.type\")}" }
            }
            for (i, c) in m.columns.iter().enumerate() {
                div { key: "{i}", class: "d0-wizard-col",
                    input {
                        r#type: "checkbox",
                        "data-a11y-id": "wizard-include-{i}",
                        "aria-label": "{dat0_i18n::t(\"wizard.column.include\")} {c.source}",
                        checked: c.include,
                        onchange: move |e| {
                            if let Some(col) = model.write().columns.get_mut(i) {
                                col.include = e.checked();
                            }
                        },
                    }
                    span { class: "d0-mono d0-wizard-source", "{c.source}" }
                    input {
                        class: "d0-field d0-mono",
                        "data-a11y-id": "wizard-name-{i}",
                        "aria-label": "{dat0_i18n::t(\"wizard.column.name\")} {c.source}",
                        value: "{c.name}",
                        oninput: move |e| {
                            if let Some(col) = model.write().columns.get_mut(i) {
                                col.name = e.value();
                            }
                        },
                    }
                    select {
                        class: "d0-field d0-mono",
                        "data-a11y-id": "wizard-type-{i}",
                        "aria-label": "{dat0_i18n::t(\"wizard.column.type\")} {c.source}",
                        value: "{c.ty}",
                        onchange: move |e| {
                            if let Some(col) = model.write().columns.get_mut(i) {
                                col.ty = e.value();
                            }
                        },
                        for t in TYPES {
                            option { key: "{t}", value: "{t}", selected: t == c.ty, "{t}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ConfirmStep(model: Signal<WizardModel>) -> Element {
    let m = model.read().clone();
    let count = m.included().count();
    rsx! {
        div { class: "d0-wizard-confirm", "data-a11y-id": "wizard-confirm",
            div { class: "d0-chip",
                "{dat0_i18n::t(\"wizard.delimiter\")} {m.delimiter}"
            }
            div { class: "d0-chip",
                "{dat0_i18n::t(\"wizard.quote\")} {m.quote}"
            }
            div { class: "d0-chip",
                "{dat0_i18n::t(\"wizard.confirm.columns\")} {count}"
            }
            if let Some(sql) = m.read_csv_sql() {
                pre {
                    class: "d0-wizard-sql d0-mono",
                    "data-a11y-id": "wizard-sql",
                    "{sql}"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sniff(encoding_supported: bool) -> SniffSummary {
        SniffSummary {
            top_delimiter: ',',
            top_score: 0.55,
            next_score: 0.53,
            encoding_supported,
            any_low_confidence_column: false,
        }
    }

    fn model() -> WizardModel {
        WizardModel::from_sniff(
            Path::new("/tmp/a.csv"),
            &sniff(true),
            vec![
                ("id".into(), "BIGINT".into()),
                ("name".into(), "VARCHAR".into()),
            ],
        )
    }

    #[test]
    fn a_sniffed_type_outside_the_vocabulary_degrades_to_varchar() {
        // DuckDB can infer types the CSV writer cannot round-trip; landing on
        // one must not present the user with an invalid form they did not fill.
        let c = ColumnDraft::new("x", "DECIMAL(18,3)");
        assert_eq!(c.ty, "VARCHAR");
    }

    #[test]
    fn read_csv_sql_is_none_until_the_model_validates() {
        let mut m = model();
        m.delimiter = String::new();
        assert!(m.read_csv_sql().is_none());
    }

    #[test]
    fn excluded_columns_stay_in_the_columns_map_but_leave_the_projection() {
        // DuckDB reads `columns` as the file's schema, so dropping an entry
        // would shift every later column by one.
        let mut m = model();
        m.columns[1].include = false;
        let sql = m.read_csv_sql().expect("valid");
        assert!(sql.contains("'name': 'VARCHAR'"), "{sql}");
        assert!(!sql.contains("AS \"name\""), "{sql}");
    }

    #[test]
    fn identifiers_and_literals_are_escaped() {
        let mut m = model();
        m.columns[0].name = "we\"ird".into();
        m.path = PathBuf::from("/tmp/o'brien.csv");
        let sql = m.read_csv_sql().expect("valid");
        assert!(sql.contains("\"we\"\"ird\""), "{sql}");
        assert!(sql.contains("'/tmp/o''brien.csv'"), "{sql}");
    }
}
