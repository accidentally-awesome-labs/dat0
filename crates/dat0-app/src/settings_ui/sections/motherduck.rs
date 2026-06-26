//! Settings → MotherDuck section (P10b). Real controls land in T8.
use super::SettingsSection;

pub struct MotherDuckSection;

impl SettingsSection for MotherDuckSection {
    fn name_key(&self) -> &'static str {
        "settings.motherduck"
    }

    fn id(&self) -> &'static str {
        "motherduck"
    }
}
