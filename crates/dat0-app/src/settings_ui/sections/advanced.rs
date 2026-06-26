//! Settings → Advanced section (P10b). Real controls land in T10.
use super::SettingsSection;

pub struct AdvancedSection;

impl SettingsSection for AdvancedSection {
    fn name_key(&self) -> &'static str {
        "settings.advanced"
    }

    fn id(&self) -> &'static str {
        "advanced"
    }
}
