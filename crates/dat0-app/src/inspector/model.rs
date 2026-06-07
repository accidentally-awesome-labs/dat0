//! Pure Inspector state: target + (table,epoch)-keyed profile cache + load supersede.
use dat0_engine::TableProfile;
use std::collections::HashMap;

#[derive(Default)]
pub struct InspectorModel {
    pub target_table: Option<String>,
    pub mode: ProfileTargetMode, // WholeTable ⇄ CurrentView profiling toggle
    epoch: HashMap<String, u64>,
    cache: HashMap<(String, u64), TableProfile>,
    load_gen: u64,
    pub search: String, // column-search box; wired in a later slice
    /// Per-column lazy chart data (T10), keyed by column name. Cleared on any
    /// table change in `set_target` so table A's bars never paint over table B.
    column_extras: HashMap<String, ColumnExtra>,
    /// Live lineage chain for the current target (P6b): ancestors↑ + descendants↓
    /// (full transitive closure). Built by `WorkspaceShell::recompute_lineage`
    /// from `catalog_tables` + the cached `sql_parents` map. Supersedes the P6a
    /// flat Dependents list (descendants now include Sql refs, not only Transforms).
    pub lineage: crate::inspector::lineage::LineageChain,
}

/// Lazily-fetched inline-chart data for one column (P6a T10). Populated after
/// the profile lands (see `WorkspaceShell::load_column_extras`): low-cardinality
/// columns get `topn`, numeric high-cardinality columns get `histogram`.
#[derive(Default, Clone)]
pub struct ColumnExtra {
    pub topn: Option<Vec<(String, u64)>>,
    pub histogram: Option<Vec<crate::charts::Bin>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileTargetMode {
    #[default]
    WholeTable,
    CurrentView,
}

impl InspectorModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_target(&mut self, table: String) {
        // Drop stale per-column chart data when the table actually changes, so
        // tableA's bars can't paint on tableB (extras are keyed by bare column
        // name, which collides across tables).
        if self.target_table.as_deref() != Some(table.as_str()) {
            self.column_extras.clear();
        }
        self.target_table = Some(table);
    }

    /// Lazy chart data for `col`, if it has been fetched for the current table.
    pub fn extra(&self, col: &str) -> Option<&ColumnExtra> {
        self.column_extras.get(col)
    }

    /// Upsert top-N bar data for `col` (low-cardinality columns).
    pub fn put_topn(&mut self, col: &str, data: Vec<(String, u64)>) {
        self.column_extras.entry(col.to_string()).or_default().topn = Some(data);
    }

    /// Upsert histogram bins for `col` (numeric high-cardinality columns).
    pub fn put_histogram(&mut self, col: &str, bins: Vec<crate::charts::Bin>) {
        self.column_extras
            .entry(col.to_string())
            .or_default()
            .histogram = Some(bins);
    }

    /// Drop all per-column chart data. Called on a mode toggle (the new mode's
    /// extras are fetched fresh, or — in CurrentView — not at all), so a stale
    /// WholeTable bar never paints over a CurrentView card with the same name.
    pub fn clear_extras(&mut self) {
        self.column_extras.clear();
    }

    /// Overwrite the live lineage chain (called by `WorkspaceShell::recompute_lineage`).
    pub fn set_lineage(&mut self, chain: crate::inspector::lineage::LineageChain) {
        self.lineage = chain;
    }

    fn epoch_of(&self, t: &str) -> u64 {
        *self.epoch.get(t).unwrap_or(&0)
    }

    pub fn cached(&self) -> Option<&TableProfile> {
        let t = self.target_table.as_ref()?;
        self.cache.get(&(t.clone(), self.epoch_of(t)))
    }

    pub fn put(&mut self, p: TableProfile) {
        if let Some(t) = self.target_table.clone() {
            let e = self.epoch_of(&t);
            self.cache.insert((t, e), p);
        }
    }

    pub fn bump_epoch(&mut self, table: &str) {
        *self.epoch.entry(table.to_string()).or_insert(0) += 1;
    }

    /// Replace a single column's profile in the current cache entry — the D4
    /// single-column patch primitive.
    ///
    /// RESERVED / not on the live write path: in dat0 a cell edit is a display
    /// overlay (`compile_view_sql` emits `SELECT * REPLACE (CASE …)`), not an
    /// in-place base mutation, so T12 routes edits through a full structural
    /// re-profile instead (correct in both WholeTable and CurrentView modes).
    /// This stays as a tested primitive for a future per-column fast path.
    pub fn patch_column(&mut self, col: &str, new: dat0_engine::ColumnProfile) {
        if let Some(t) = self.target_table.clone() {
            let e = self.epoch_of(&t);
            if let Some(p) = self.cache.get_mut(&(t, e)) {
                if let Some(slot) = p.columns.iter_mut().find(|c| c.name == col) {
                    *slot = new;
                }
            }
        }
    }

    pub fn begin_load(&mut self) -> u64 {
        self.load_gen += 1;
        self.load_gen
    }

    pub fn is_current(&self, load_id: u64) -> bool {
        load_id == self.load_gen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_after_put_then_miss_on_epoch_bump() {
        let mut m = InspectorModel::new();
        m.set_target("orders".into());
        assert!(m.cached().is_none(), "cold");
        m.put(fake_profile());
        assert!(m.cached().is_some(), "warm after put");
        m.bump_epoch("orders");
        assert!(m.cached().is_none(), "epoch bump invalidates");
    }

    #[test]
    fn supersede_gen_monotonic() {
        let mut m = InspectorModel::new();
        let g1 = m.begin_load();
        let g2 = m.begin_load();
        assert!(!m.is_current(g1), "older load superseded");
        assert!(m.is_current(g2), "latest load current");
    }

    #[test]
    fn patch_column_replaces_single_column_only() {
        use dat0_engine::{ColumnProfile, TableProfile};
        let mut m = InspectorModel::new();
        m.set_target("orders".into());
        let mk = |name: &str, distinct: u64| ColumnProfile {
            name: name.into(),
            ty: "T".into(),
            null_pct: 0.0,
            approx_distinct: distinct,
            count: 1,
            numeric: None,
            length: None,
        };
        m.put(TableProfile {
            rows: 1,
            columns: vec![mk("a", 1), mk("b", 1)],
        });
        m.patch_column("b", mk("b", 99));
        let cols = &m.cached().unwrap().columns;
        assert_eq!(
            cols.iter().find(|c| c.name == "a").unwrap().approx_distinct,
            1,
            "a untouched"
        );
        assert_eq!(
            cols.iter().find(|c| c.name == "b").unwrap().approx_distinct,
            99,
            "b patched"
        );
    }

    #[test]
    fn set_lineage_overwrites_chain() {
        use crate::inspector::lineage::{ChainStep, EdgeKind, LineageChain, NodeKind};
        let mut m = InspectorModel::new();
        assert!(m.lineage.ancestors.is_empty() && m.lineage.descendants.is_empty());
        m.set_lineage(LineageChain {
            ancestors: vec![ChainStep {
                label: "sales".into(),
                kind: NodeKind::Table,
                edge: EdgeKind::FileImport,
                depth: 1,
                open_name: Some("sales".into()),
            }],
            descendants: vec![],
        });
        assert_eq!(m.lineage.ancestors.len(), 1);
        assert_eq!(m.lineage.ancestors[0].label, "sales");
    }

    fn fake_profile() -> dat0_engine::TableProfile {
        dat0_engine::TableProfile {
            rows: 1,
            columns: vec![],
        }
    }
}
