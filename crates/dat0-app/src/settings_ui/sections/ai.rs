//! Settings → AI section (P10b). Real controls land in T9.
use super::SettingsSection;

pub struct AiSection;

impl SettingsSection for AiSection {
    fn name_key(&self) -> &'static str {
        "settings.ai"
    }

    fn id(&self) -> &'static str {
        "ai"
    }
}
