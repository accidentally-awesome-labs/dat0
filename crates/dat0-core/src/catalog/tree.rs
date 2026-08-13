//! Pure catalog tree model (P6a; regrouped to the v4 sidebar in SH2).
//!
//! ## Why three groups and not four
//!
//! P6a grouped by *provenance*: Sources / Cloud / Tables / Derived. That is a
//! taxonomy of how a relation came to exist, and it made the sidebar answer a
//! question nobody asks while browsing. The v4 sidebar
//! (`009-redesign-landing-v4/DESIGN-SPEC.md` §2) groups by *where the bytes
//! live*, which is the question that changes what you can do with a row:
//!
//! - [`CatalogTree::files`] — everything resident on this machine. A CSV
//!   registered from disk and a SQL view derived from it are both local; the
//!   view's derivation is the Inspector's lineage panel's job to show, not the
//!   sidebar's. Merging `sources` + `tables` + `derived` loses no *reachable*
//!   information, only a redundant restatement of it.
//! - [`CatalogTree::connections`] — everything behind an attachment, MotherDuck
//!   and SQLite alike. The old Cloud/Sources split put a `md:` attach and a
//!   `/tmp/x.db` attach in different groups even though both are one `ATTACH`
//!   away from being gone; `TableOrigin::Attached` is the honest boundary.
//! - [`CatalogTree::packages`] — `.dat0` packages from the recents store. These
//!   are not in the engine catalog at all (opening one spawns a NEW read-only
//!   window), which is why they arrive through [`packages_from_recents`]
//!   instead of `build`.

use dat0_engine::{TableInfo, TableOrigin};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogNode {
    pub name: String,
    pub schema: String,
    /// For attachment nodes: the tables inside, by name.
    pub children: Vec<String>,
}

/// A `.dat0` package offered by the PACKAGES group.
///
/// Carries the path because activating the row opens a package, and a package
/// is identified by where it is — two workspaces can both hold `analysis.dat0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageNode {
    /// File name, which is what the row paints. Never the full path: a sidebar
    /// 238 px wide cannot show one and truncating a path from the left is the
    /// worst of both.
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogTree {
    /// Local relations: `TableOrigin::File` + every `TableOrigin::Derived`.
    pub files: Vec<CatalogNode>,
    /// One parent node per attachment alias; children are its tables.
    pub connections: Vec<CatalogNode>,
    /// `.dat0` packages from the recents store. Empty unless the caller chains
    /// [`CatalogTree::with_packages`] — `build` sees only the engine catalog.
    pub packages: Vec<PackageNode>,
}

impl CatalogTree {
    /// Group the engine's table list into FILES and CONNECTIONS.
    ///
    /// Pure over `tables`, and deliberately blind to PACKAGES: packages come
    /// from the recents store, which is a process-wide singleton, and folding a
    /// global read into a model constructor would make every unit test below
    /// depend on ambient state.
    pub fn build(tables: &[TableInfo]) -> Self {
        let mut tree = CatalogTree::default();
        // Attached tables group by alias into one PARENT node per attach
        // (catalog-tree slice): parent.name = alias, children = table names.
        // (alias, children), first-seen order; sorted below.
        let mut attaches: Vec<(String, Vec<String>)> = Vec::new();
        for ti in tables {
            let leaf = CatalogNode {
                name: ti.name.clone(),
                schema: ti.schema.clone(),
                children: vec![],
            };
            match &ti.origin {
                TableOrigin::File(_) | TableOrigin::Derived(_) => tree.files.push(leaf),
                TableOrigin::Attached { alias, .. } => {
                    match attaches.iter_mut().find(|(a, _)| a == alias) {
                        Some((_, kids)) => kids.push(ti.name.clone()),
                        None => attaches.push((alias.clone(), vec![ti.name.clone()])),
                    }
                }
            }
        }
        for (alias, mut kids) in attaches {
            kids.sort();
            tree.connections.push(CatalogNode {
                name: alias,
                schema: String::new(),
                children: kids,
            });
        }
        // Deterministic paint/nav order: every group sorted by node name.
        for group in [&mut tree.files, &mut tree.connections] {
            group.sort_by(|a, b| a.name.cmp(&b.name));
        }
        tree
    }

    /// Attach the PACKAGES group.
    ///
    /// Chained rather than passed to [`build`] so the engine-catalog half stays
    /// a pure function of `tables` and the recents half can be swapped for a
    /// fixture in tests.
    pub fn with_packages(mut self, packages: Vec<PackageNode>) -> Self {
        self.packages = packages;
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
        self
    }

    /// Token-AND filter over node names (case-insensitive). Leaves survive if
    /// every whitespace token is a substring of their lowercased name. Parents
    /// (attach nodes with children): an ALIAS match keeps the parent with ALL
    /// children; otherwise the children are filtered and the parent survives
    /// iff any child matched. Packages filter on their file name.
    pub fn filter(mut self, query: &str) -> Self {
        let toks: Vec<String> = query.split_whitespace().map(|s| s.to_lowercase()).collect();
        if toks.is_empty() {
            return self;
        }
        let matches = |name: &str| {
            let lc = name.to_lowercase();
            toks.iter().all(|t| lc.contains(t.as_str()))
        };
        let keep = |n: &mut CatalogNode| {
            if n.children.is_empty() {
                return matches(&n.name);
            }
            if matches(&n.name) {
                return true; // alias match keeps all children
            }
            n.children.retain(|c| matches(c));
            !n.children.is_empty()
        };
        self.files.retain_mut(keep);
        self.connections.retain_mut(keep);
        self.packages.retain(|p| matches(&p.name));
        self
    }
}

/// Snapshot the `.dat0` packages the user has opened before.
///
/// Reads the process-wide recents store installed by `main.rs`
/// (`window_registry::install_recents`), which is an in-memory `Vec` of at most
/// 25 entries — NOT a disk read. Returns empty when the singleton is absent
/// (unit tests, sub-module harnesses) or its lock is poisoned, mirroring
/// [`crate::window_registry::recents_snapshot`], which makes the same trade for
/// the same reason: a sidebar group is not worth propagating a `Result` for.
///
/// The mirror image of `recents_snapshot`: that one keeps `Workspace` entries
/// and drops `Package`; this one does the opposite.
pub fn packages_from_recents() -> Vec<PackageNode> {
    let Some(store) = crate::globals::recents() else {
        return Vec::new();
    };
    let Ok(guard) = store.lock() else {
        return Vec::new();
    };
    guard
        .list()
        .iter()
        .filter_map(|e| match e {
            crate::recents::RecentEntry::Package { path } => Some(PackageNode {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string()),
                path: path.clone(),
            }),
            crate::recents::RecentEntry::Workspace { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{DerivedOrigin, TableInfo, TableOrigin};
    use std::path::PathBuf;

    fn t(name: &str, origin: TableOrigin) -> TableInfo {
        TableInfo {
            name: name.into(),
            schema: "main".into(),
            columns: vec![],
            row_count_estimate: None,
            origin,
        }
    }

    fn attached(name: &str, alias: &str, source: &str) -> TableInfo {
        t(
            name,
            TableOrigin::Attached {
                alias: alias.into(),
                source: source.into(),
            },
        )
    }

    #[test]
    fn local_origins_all_land_in_files() {
        let tables = vec![
            t("sales", TableOrigin::File(PathBuf::from("/s.csv"))),
            t(
                "orders",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
            t(
                "orders_open",
                TableOrigin::Derived(DerivedOrigin::Transform {
                    parent: "orders".into(),
                    ops: vec![],
                }),
            ),
            attached("md_events", "md", "md:"),
        ];
        let tree = CatalogTree::build(&tables);
        // File, base-SQL and Transform all live on this machine → one group.
        assert_eq!(
            tree.files
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            vec!["orders", "orders_open", "sales"],
            "files sorted by name, all three local origins present"
        );
        assert_eq!(tree.connections.len(), 1);
        assert_eq!(
            tree.connections[0].name, "md",
            "connection node is the attach ALIAS"
        );
        assert_eq!(tree.connections[0].children, vec!["md_events".to_string()]);
    }

    #[test]
    fn motherduck_and_sqlite_attaches_share_the_connections_group() {
        // The pre-SH2 model split these across Cloud and Sources on an `md:`
        // prefix. Both are one ATTACH away from vanishing, so both are
        // connections; nothing else about them differs to the sidebar.
        let tables = vec![
            t("sales", TableOrigin::File(PathBuf::from("/s.csv"))),
            attached("md_events", "sample_data", "md:"),
            attached("local_sqlite", "sq", "/tmp/x.db"),
        ];
        let tree = CatalogTree::build(&tables);
        assert_eq!(
            tree.connections
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            vec!["sample_data", "sq"],
        );
        assert_eq!(tree.files.len(), 1, "only the file leaf is local");
        assert_eq!(tree.files[0].name, "sales");
    }

    #[test]
    fn same_alias_tables_group_under_one_parent_sorted() {
        let tables = vec![
            attached("zeta", "sq", "/tmp/x.db"),
            attached("alpha", "sq", "/tmp/x.db"),
        ];
        let tree = CatalogTree::build(&tables);
        assert_eq!(tree.connections.len(), 1, "one parent per alias");
        assert_eq!(
            tree.connections[0].children,
            vec!["alpha".to_string(), "zeta".to_string()],
            "children sorted by name"
        );
    }

    #[test]
    fn filter_child_match_keeps_only_the_matching_child() {
        let tables = vec![
            attached("md_orders", "db", "md:"),
            attached("md_events", "db", "md:"),
        ];
        let tree = CatalogTree::build(&tables).filter("ord");
        assert_eq!(tree.connections.len(), 1);
        assert_eq!(tree.connections[0].name, "db");
        assert_eq!(tree.connections[0].children, vec!["md_orders".to_string()]);
    }

    #[test]
    fn filter_alias_match_keeps_all_children() {
        let tables = vec![
            attached("md_orders", "warehouse", "md:"),
            attached("md_events", "warehouse", "md:"),
        ];
        let tree = CatalogTree::build(&tables).filter("ware");
        assert_eq!(tree.connections.len(), 1);
        assert_eq!(
            tree.connections[0].children.len(),
            2,
            "alias match keeps ALL children"
        );
    }

    #[test]
    fn filter_no_match_drops_the_parent() {
        let tree = CatalogTree::build(&[attached("md_orders", "db", "md:")]).filter("zzz");
        assert!(tree.connections.is_empty());
    }

    #[test]
    fn token_and_search_filters_files() {
        let tables = vec![
            t(
                "daily_revenue",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
            t(
                "orders",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
        ];
        let tree = CatalogTree::build(&tables).filter("dai rev");
        assert_eq!(tree.files.len(), 1);
        assert_eq!(tree.files[0].name, "daily_revenue");
    }

    #[test]
    fn packages_are_sorted_and_filtered_by_file_name() {
        let pkgs = vec![
            PackageNode {
                name: "zeta.dat0".into(),
                path: PathBuf::from("/tmp/zeta.dat0"),
            },
            PackageNode {
                name: "alpha.dat0".into(),
                path: PathBuf::from("/tmp/alpha.dat0"),
            },
        ];
        let tree = CatalogTree::default().with_packages(pkgs.clone());
        assert_eq!(
            tree.packages
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha.dat0", "zeta.dat0"],
        );
        let filtered = CatalogTree::default().with_packages(pkgs).filter("zet");
        assert_eq!(filtered.packages.len(), 1);
        assert_eq!(filtered.packages[0].path, PathBuf::from("/tmp/zeta.dat0"));
    }

    #[test]
    fn packages_from_recents_degrades_without_the_singleton() {
        // The store is installed by `main.rs`; a unit test never boots the app,
        // so this returns an empty group rather than panicking. Asserted as a
        // property, not `is_empty()`, so the test stays honest if some other
        // test in this binary ever does install the singleton.
        for p in packages_from_recents() {
            assert!(
                p.path.ends_with(&p.name),
                "a package node's name is its file name: {p:?}"
            );
        }
    }
}
