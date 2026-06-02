//! Runtime registration of a single SQL grammar for the console code editor
//! (P5a §0 decision 6). We deliberately avoid gpui-component's all-or-nothing
//! `tree-sitter-languages` feature (~28 grammars). `LanguageRegistry::language`
//! checks the runtime registry before the compile-time `Language` enum
//! (`highlighter/registry.rs:496`), so registering "sql" here drives
//! highlighting for any `InputState::code_editor("sql")`.

use gpui_component::highlighter::{LanguageConfig, LanguageRegistry};

/// Register the `tree-sitter-sequel` SQL grammar under the name "sql".
/// Idempotent: re-registering overwrites. Call once at boot, before windows open.
pub fn register_sql_language() {
    let cfg = LanguageConfig::new(
        "sql",
        tree_sitter_sequel::LANGUAGE.into(),
        vec![],
        tree_sitter_sequel::HIGHLIGHTS_QUERY,
        "",
        "",
    );
    LanguageRegistry::singleton().register("sql", &cfg);
}
