use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ZedTheme {
    pub name: String,
    pub appearance: String, // "light" or "dark"
    pub style: ZedStyle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZedStyle {
    pub background: String,
    pub foreground: String,
    pub border: String,
    pub accent: String,
    pub error: String,
    pub success: String,
    pub warning: String,
    // Extended: surface variants, syntax highlight slots, etc.
    // Mapped per-component as needed.
}
