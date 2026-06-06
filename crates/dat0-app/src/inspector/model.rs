//! Pure Inspector state: target + (table,epoch)-keyed profile cache + load supersede.
use std::collections::HashMap;
use dat0_engine::TableProfile;

#[derive(Default)]
pub struct InspectorModel {
    pub target_table: Option<String>,
    pub mode: ProfileTargetMode,   // consumed in T9
    epoch: HashMap<String, u64>,
    cache: HashMap<(String, u64), TableProfile>,
    load_gen: u64,
    pub search: String,            // column-search box; wired in a later slice
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
        self.target_table = Some(table);
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

    fn fake_profile() -> dat0_engine::TableProfile {
        dat0_engine::TableProfile { rows: 1, columns: vec![] }
    }
}
