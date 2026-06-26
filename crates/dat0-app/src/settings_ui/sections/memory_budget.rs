//! Settings → Memory Budget section (P10b). Real InputState control lands in T7.
use super::SettingsSection;

pub struct MemoryBudgetSection;

impl SettingsSection for MemoryBudgetSection {
    fn name_key(&self) -> &'static str {
        "settings.memory_budget"
    }

    fn id(&self) -> &'static str {
        "memory_budget"
    }
}
