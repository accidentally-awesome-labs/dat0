//! Accessibility attributes, and the format-identity helper that rides along
//! with them.
//!
//! # What changed, and why it matters
//!
//! Under GPUI, `A11yExt::a11y(id, role, label)` and `FocusStopExt::focus_stop`
//! emitted an AccessKit node **only under the `a11y-capture` feature**, i.e.
//! only in tests. Release builds got an identity no-op, which is why
//! deferral **D-015** ("no production accessibility") stayed open: the tree the
//! tests asserted against did not exist in the shipped app.
//!
//! In a WebView the same information is plain DOM — `role`, `aria-label`,
//! `tabindex` — and the platform accessibility API reads it natively, in every
//! build. So these attributes ship, D-015 closes, and `accesskit`,
//! `accesskit_consumer` and `kittest` all leave the tree.
//!
//! `data-a11y-id` is dat0's own stable handle. The headless harness queries by
//! it, and it is what makes a test assertion survive a copy change that
//! `aria-label` would not.

use std::path::Path;

/// The six roles dat0 uses, mapped to ARIA.
///
/// Deliberately small. A larger vocabulary invites a component to pick a role
/// because it sounds right rather than because a screen reader does something
/// useful with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRole {
    /// Anything clickable that is not a link.
    Button,
    /// Static descriptive text a reader should announce with its control.
    Label,
    /// One grid cell.
    Cell,
    /// One grid row.
    Row,
    /// A modal surface. Implies a focus trap; see the shell's key cascade.
    Dialog,
    /// Something that must interrupt: a failure, a destructive confirmation.
    Alert,
    /// One workspace tab. Its parent carries `role="tablist"`.
    Tab,
    /// A landmark a reader can jump to. The catalog sidebar is the only one.
    Navigation,
}

impl AccessRole {
    /// The ARIA `role` string.
    pub const fn aria(self) -> &'static str {
        match self {
            AccessRole::Button => "button",
            // `note` rather than `label`: ARIA's `label` role does not exist,
            // and `note` is what a reader announces as ancillary content.
            AccessRole::Label => "note",
            AccessRole::Cell => "gridcell",
            AccessRole::Row => "row",
            AccessRole::Dialog => "dialog",
            AccessRole::Alert => "alert",
            AccessRole::Tab => "tab",
            AccessRole::Navigation => "navigation",
        }
    }
}

/// Tab-order participation.
///
/// Spelled out rather than passed as a bare `i32` because the three meanings
/// are genuinely different and `-1` versus `0` is exactly the kind of detail
/// that gets copied wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabStop {
    /// In the natural tab order.
    Yes,
    /// Focusable programmatically, but skipped by Tab. Grid cells and menu
    /// items are this: the container owns the tab stop and arrow keys move
    /// within it.
    Programmatic,
    /// Not focusable.
    No,
}

impl TabStop {
    /// The `tabindex` value, or `None` when the attribute should be omitted.
    pub const fn index(self) -> Option<i32> {
        match self {
            TabStop::Yes => Some(0),
            TabStop::Programmatic => Some(-1),
            TabStop::No => None,
        }
    }
}

/// One element's accessibility identity.
///
/// Built with [`A11y::new`] and spread into an rsx element through its
/// accessors. Kept as data rather than an extension trait because Dioxus
/// attributes are values, not builder calls, and a struct is what a test can
/// construct and compare.
#[derive(Debug, Clone)]
pub struct A11y {
    /// Stable query handle. Never localised, never derived from copy.
    pub id: &'static str,
    pub role: AccessRole,
    /// The announced name. Localised.
    pub label: String,
    pub tab: TabStop,
}

impl A11y {
    pub fn new(id: &'static str, role: AccessRole, label: impl Into<String>) -> Self {
        Self {
            id,
            role,
            label: label.into(),
            tab: match role {
                // A cell or a row is reached by arrow keys from its container,
                // never by Tab: 1 M rows of tab stops is not navigation.
                AccessRole::Cell | AccessRole::Row => TabStop::Programmatic,
                AccessRole::Button => TabStop::Yes,
                AccessRole::Label => TabStop::No,
                // A tab strip is one stop: Tab reaches the strip, arrows move
                // within it. Reaching six tabs by Tab is how a keyboard user
                // ends up pressing it thirty times to reach the grid.
                AccessRole::Tab => TabStop::Programmatic,
                // A landmark is reached by the reader's own navigation, not by
                // Tab; its rows are the stops.
                AccessRole::Navigation => TabStop::No,
                AccessRole::Dialog | AccessRole::Alert => TabStop::Programmatic,
            },
        }
    }

    /// Override the default tab participation for this role.
    pub fn tab(mut self, tab: TabStop) -> Self {
        self.tab = tab;
        self
    }

    /// `tabindex` as a string, or `None` to omit the attribute.
    pub fn tabindex(&self) -> Option<&'static str> {
        match self.tab.index()? {
            0 => Some("0"),
            _ => Some("-1"),
        }
    }
}

/// The CSS class for the 7×7 swatch that precedes a file name.
///
/// One definition, used by the sidebar, chips, tab titles and the grid's source
/// badge — so a `.parquet` is the same purple everywhere, and adding a format
/// is one arm here plus one rule in `app.css`.
pub fn format_swatch(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "csv" | "tsv" => "sw-csv",
        "parquet" | "pq" => "sw-parquet",
        "sqlite" | "sqlite3" | "db" => "sw-sqlite",
        "json" | "jsonl" | "ndjson" => "sw-json",
        "dat0" => "sw-dat0",
        _ => "sw-other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn roles_map_to_real_aria_values() {
        // `gridcell` and `row` only mean anything inside a `grid`; the rest are
        // standalone. This asserts the strings, which is what a screen reader
        // actually consumes.
        assert_eq!(AccessRole::Button.aria(), "button");
        assert_eq!(AccessRole::Label.aria(), "note");
        assert_eq!(AccessRole::Cell.aria(), "gridcell");
        assert_eq!(AccessRole::Row.aria(), "row");
        assert_eq!(AccessRole::Dialog.aria(), "dialog");
        assert_eq!(AccessRole::Alert.aria(), "alert");
    }

    #[test]
    fn cells_and_rows_are_not_tab_stops() {
        // The container owns the tab stop; arrows move within it. A grid whose
        // cells are each a tab stop is unusable with a keyboard.
        assert_eq!(
            A11y::new("cell-0-0", AccessRole::Cell, "1").tabindex(),
            Some("-1")
        );
        assert_eq!(
            A11y::new("row-0", AccessRole::Row, "row 0").tabindex(),
            Some("-1")
        );
    }

    #[test]
    fn buttons_are_tab_stops_and_labels_are_not() {
        assert_eq!(
            A11y::new("run", AccessRole::Button, "Run").tabindex(),
            Some("0")
        );
        assert_eq!(
            A11y::new("hint", AccessRole::Label, "hint").tabindex(),
            None
        );
    }

    #[test]
    fn tab_participation_can_be_overridden() {
        let a = A11y::new("cell-0-0", AccessRole::Cell, "1").tab(TabStop::Yes);
        assert_eq!(a.tabindex(), Some("0"));
    }

    #[test]
    fn every_shipped_format_has_its_own_swatch() {
        let cases = [
            ("sales.csv", "sw-csv"),
            ("SALES.CSV", "sw-csv"),
            ("events.tsv", "sw-csv"),
            ("events.parquet", "sw-parquet"),
            ("chinook.sqlite", "sw-sqlite"),
            ("chinook.db", "sw-sqlite"),
            ("log.jsonl", "sw-json"),
            ("log.ndjson", "sw-json"),
            ("q2.dat0", "sw-dat0"),
        ];
        for (name, want) in cases {
            assert_eq!(format_swatch(&PathBuf::from(name)), want, "{name}");
        }
    }

    #[test]
    fn an_unknown_or_absent_extension_still_gets_a_swatch() {
        // The shell's shape must not depend on recognising the file: a row
        // without a swatch is a different height and the column jitters.
        assert_eq!(format_swatch(&PathBuf::from("notes.xyz")), "sw-other");
        assert_eq!(format_swatch(&PathBuf::from("Makefile")), "sw-other");
        assert_eq!(format_swatch(&PathBuf::from("")), "sw-other");
    }

    #[test]
    fn a_dotfile_is_not_mistaken_for_an_extension() {
        // `Path::extension` returns None for `.csv` (it is all stem), which is
        // the behaviour we want: a file literally named `.csv` is not a CSV.
        assert_eq!(format_swatch(&PathBuf::from(".csv")), "sw-other");
    }
}
