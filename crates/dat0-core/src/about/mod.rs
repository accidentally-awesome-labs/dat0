//! Build identity.
//!
//! The About *window* is a UI concern and lives in the UI crate; the version /
//! commit / build metadata it displays, and the rows it displays them as, are
//! not.

pub mod build_info;

use build_info::BuildInfo;

/// The human-facing GitHub Releases page (NOT the API endpoint) opened by the
/// "Download" button when a newer release is available.
pub const RELEASES_PAGE_URL: &str =
    "https://github.com/accidentally-awesome-labs/dat0/releases/latest";

/// Pure, testable text rows for the About box. `newer` = Some(tag) when a newer
/// release exists (drives the nudge line).
pub fn summary_lines(b: &BuildInfo, newer: Option<&str>) -> Vec<String> {
    let mut lines = vec![
        dat0_i18n::t("about.title"),
        format!("{} {}", dat0_i18n::t("about.version"), b.version),
        format!("{} {}", dat0_i18n::t("about.build"), b.git_sha),
        format!("{} Apache-2.0", dat0_i18n::t("about.license")),
        dat0_i18n::t("about.acknowledgements"),
    ];
    match newer {
        Some(tag) => lines.push(format!(
            "{} {}",
            dat0_i18n::t("about.update.available"),
            tag
        )),
        None => lines.push(dat0_i18n::t("about.update.current")),
    }
    lines
}
