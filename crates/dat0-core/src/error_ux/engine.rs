//! `EngineError` → user-facing [`Banner`].
//!
//! Before EN3 an engine failure reached the user in one of three shapes: raw
//! DuckDB text in the SQL console's error strip, a `tracing::warn!` for a failed
//! grid prefetch, or a `tracing::error!` for a failed view change. Two of those
//! three are invisible to anyone not running with a terminal attached.
//!
//! This module is the single translation point: a localized headline the user
//! can act on, plus the driver's own text kept verbatim as the banner body so a
//! bug report still carries the real message.

use crate::error_ux::Banner;
use dat0_engine::EngineError;

/// Every i18n key [`banner_for`] can emit.
///
/// Listed as a const so the i18n gate can see them. `scripts/i18n-check.sh`
/// resolves keys two ways: a regex over call sites whose key is a string
/// literal, and this `pub const *_KEYS: &[&str]` shape. `banner_for` composes
/// its key from a variant name (`engine.error.<snake_variant>`), so the regex is
/// structurally blind to it — declaring the list is what makes these keys
/// checkable at all.
///
/// (Deliberately no worked example of a literal call site in this doc comment:
/// the extractor regex does not know prose from code and would demand a key for
/// whatever placeholder appeared inside it.)
///
/// One entry per [`EngineError`] variant, in declaration order
/// (`dat0-engine/src/error.rs`). The match in `banner_for` has no `_` arm, so a
/// new variant is a compile error there; this list is kept beside it and pinned
/// by `tests/engine_error_banner.rs`.
pub const ENGINE_ERROR_KEYS: &[&str] = &[
    "engine.error.duck_db",
    "engine.error.arrow",
    "engine.error.io",
    "engine.error.invalid_path",
    "engine.error.unsupported_format",
    "engine.error.invalid_option",
    "engine.error.unknown_attach_scheme",
    "engine.error.not_implemented",
    "engine.error.mother_duck_auth",
    "engine.error.extension_load",
    "engine.error.migration",
    "engine.error.interrupted",
    "engine.error.task_join",
    "engine.error.engine_closed",
    "engine.error.engine_poisoned",
    "engine.error.engine_failed",
];

/// The i18n key for `err`'s variant.
///
/// Deliberately **no `_` arm**: adding a variant to `EngineError` must break this
/// build rather than silently fall back to a generic headline, because a generic
/// headline is exactly the failure mode EN3 exists to remove.
fn key_for(err: &EngineError) -> &'static str {
    match err {
        EngineError::DuckDb(_) => "engine.error.duck_db",
        EngineError::Arrow(_) => "engine.error.arrow",
        EngineError::Io(_) => "engine.error.io",
        EngineError::InvalidPath(_) => "engine.error.invalid_path",
        EngineError::UnsupportedFormat(_) => "engine.error.unsupported_format",
        EngineError::InvalidOption { .. } => "engine.error.invalid_option",
        EngineError::UnknownAttachScheme(_) => "engine.error.unknown_attach_scheme",
        EngineError::NotImplemented { .. } => "engine.error.not_implemented",
        EngineError::MotherDuckAuth => "engine.error.mother_duck_auth",
        EngineError::ExtensionLoad { .. } => "engine.error.extension_load",
        EngineError::Migration { .. } => "engine.error.migration",
        EngineError::Interrupted => "engine.error.interrupted",
        EngineError::TaskJoin(_) => "engine.error.task_join",
        EngineError::EngineClosed => "engine.error.engine_closed",
        EngineError::EnginePoisoned => "engine.error.engine_poisoned",
        EngineError::EngineFailed(_) => "engine.error.engine_failed",
    }
}

/// Map an engine error to a banner: localized headline, raw driver text as body.
///
/// The body is `err`'s own `Display` (the `thiserror` message), unmodified. It is
/// kept because the headline is deliberately generic enough to be translated,
/// and the specific `Catalog Error: Table with name x does not exist!` is the
/// part a user pastes into an issue.
///
/// [`EngineError::Interrupted`] is the one variant that is not a fault: the user
/// pressed Cmd+. and got what they asked for. It renders as an `Info` banner, so
/// a cancel does not paint the window red.
pub fn banner_for(err: &EngineError) -> Banner {
    let headline = dat0_i18n::t(key_for(err));
    let detail = err.to_string();
    match err {
        EngineError::Interrupted => Banner::info(headline),
        _ => Banner::error(headline, detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_key_for_output() {
        // `key_for` is the source of truth; `ENGINE_ERROR_KEYS` is what the i18n
        // gate reads. This pins the two together for the variants constructible
        // without a real duckdb::Error (the full sweep lives in
        // tests/engine_error_banner.rs, which can build all sixteen).
        for err in [
            EngineError::MotherDuckAuth,
            EngineError::Interrupted,
            EngineError::EngineClosed,
            EngineError::EnginePoisoned,
        ] {
            assert!(
                ENGINE_ERROR_KEYS.contains(&key_for(&err)),
                "{} maps to a key absent from ENGINE_ERROR_KEYS",
                key_for(&err)
            );
        }
    }

    #[test]
    fn interrupted_is_info_not_error() {
        let b = banner_for(&EngineError::Interrupted);
        assert_eq!(b.kind, crate::error_ux::BannerKind::Info);
    }
}
