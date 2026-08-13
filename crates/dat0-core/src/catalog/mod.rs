pub mod nav;
pub mod tree;
pub use tree::{CatalogNode, CatalogTree, PackageNode, packages_from_recents};

/// Empty-state i18n keys, listed so scripts/i18n-check.sh (which only matches
/// string-literal `t("…")` arguments) can resolve keys built from a group name.
pub const CATALOG_EMPTY_KEYS: &[&str] = &[
    "catalog.empty.files",
    "catalog.empty.connections",
    "catalog.empty.packages",
];

/// Group-header i18n keys. Same reason as [`CATALOG_EMPTY_KEYS`]: the header is
/// composed per group in [`panel::render_catalog`]'s table, so no literal
/// `t("catalog.group.files")` exists for the regex extractor to find.
pub const CATALOG_GROUP_KEYS: &[&str] = &[
    "catalog.group.files",
    "catalog.group.connections",
    "catalog.group.packages",
];
