//! Theme: the design token set and the contrast maths that keeps it honest.
//!
//! There is no widget-library config here and no Rust-side styling vocabulary.
//! A theme is 40 CSS values plus a mode flag; `ThemeTokens::css_vars` turns one
//! into a `:root{…}` block and the UI renders it into a single `<style>`
//! element, so switching theme is a signal write.

pub mod contrast;
pub mod tokens;

pub use tokens::{BUILTIN_IDS, DEFAULT_ID, ThemeMode, ThemeTokens, builtin, builtin_or_default};
