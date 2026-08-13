/// Compile-time build identity surfaced in the About box.
pub struct BuildInfo {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub built: Option<&'static str>,
}

impl BuildInfo {
    pub fn current() -> BuildInfo {
        BuildInfo {
            version: env!("CARGO_PKG_VERSION"),
            git_sha: env!("DAT0_GIT_SHA"),
            built: option_env!("DAT0_BUILD_TIME"),
        }
    }
}
