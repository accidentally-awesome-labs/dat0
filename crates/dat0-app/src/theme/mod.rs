mod zed_schema;

use anyhow::{Context, Result};
pub use zed_schema::*;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub style: ZedStyle,
}

impl Theme {
    pub fn load_builtin(name: &str) -> Result<Self> {
        let json = match name {
            "dark" => include_str!("builtins/dark.json"),
            "light" => include_str!("builtins/light.json"),
            "high-contrast" => include_str!("builtins/high-contrast.json"),
            other => anyhow::bail!("unknown built-in theme: {other}"),
        };
        let parsed: ZedTheme = serde_json::from_str(json)
            .with_context(|| format!("parse builtin theme {name}"))?;
        Ok(Self { name: parsed.name, style: parsed.style })
    }
}
