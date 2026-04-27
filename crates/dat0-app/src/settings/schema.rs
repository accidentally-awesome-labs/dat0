use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub profile: Profile,
    pub theme: Theme,
    pub telemetry: Telemetry,
    pub workspace: Workspace,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profile: Profile::default(),
            theme: Theme::default(),
            telemetry: Telemetry::default(),
            workspace: Workspace::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "dark".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Telemetry {
    pub crash_submission_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Workspace {
    pub treat_paths_as_networked: Vec<std::path::PathBuf>,
}
