//! EN3: every `EngineError` variant reaches the user as a readable banner.
//!
//! Two independent failures are gated here.
//!
//! 1. **The headline is resolved, not echoed.** `dat0_i18n::t` returns the KEY
//!    on a miss, so a forgotten `en.json` entry does not throw — it paints
//!    `engine.error.duck_db` into the banner. Comparing headline against key is
//!    the only way to see that.
//! 2. **`ENGINE_ERROR_KEYS` is complete.** The list exists so
//!    `scripts/i18n-check.sh` can resolve keys that `banner_for` composes from a
//!    variant name rather than writing as string literals. A list that drifts
//!    from the match arms silently disables that gate, so every key the mapper
//!    can emit must appear in it and every entry must resolve.

use dat0_core::error_ux::{BannerKind, ENGINE_ERROR_KEYS, banner_for};
use dat0_engine::EngineError;

/// One value of EVERY `EngineError` variant (`dat0-engine/src/error.rs`).
///
/// Sixteen variants. `DuckDb`, `Arrow`, `Io` and `Migration` wrap foreign error
/// types, so they are built from real instances of those: a genuinely malformed
/// SQL string for the DuckDB ones, a real `io::Error`, and a real
/// `ArrowError`. This vector is what makes the no-`_`-arm match in
/// `error_ux::engine::key_for` worth having — if a variant is added there and
/// forgotten here, the exhaustive `match` in that module fails to compile first.
fn every_variant() -> Vec<EngineError> {
    let duck_err = duckdb::Connection::open_in_memory()
        .and_then(|c| c.prepare("SELECT * FROM ").map(|_| ()))
        .expect_err("`SELECT * FROM ` must not parse");

    vec![
        EngineError::DuckDb(duck_err),
        EngineError::Arrow(duckdb::arrow::error::ArrowError::SchemaError(
            "column absent".into(),
        )),
        EngineError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        )),
        EngineError::InvalidPath(std::path::PathBuf::from("/nope")),
        EngineError::UnsupportedFormat("xlsx".into()),
        EngineError::InvalidOption {
            field: "delimiter",
            reason: "must be one byte".into(),
        },
        EngineError::UnknownAttachScheme("mysql:".into()),
        EngineError::NotImplemented {
            feature: "geo types",
        },
        EngineError::MotherDuckAuth,
        EngineError::ExtensionLoad {
            name: "sqlite_scanner",
        },
        EngineError::Migration {
            version: 3,
            name: "add_rowid".into(),
            source: duckdb::Connection::open_in_memory()
                .and_then(|c| c.prepare("SELECT * FROM ").map(|_| ()))
                .expect_err("`SELECT * FROM ` must not parse"),
        },
        EngineError::Interrupted,
        EngineError::TaskJoin("worker panicked".into()),
        EngineError::EngineClosed,
        EngineError::EnginePoisoned,
        EngineError::EngineFailed("result stream ended early".into()),
    ]
}

#[test]
fn every_variant_yields_a_resolved_headline() {
    let variants = every_variant();
    assert_eq!(
        variants.len(),
        ENGINE_ERROR_KEYS.len(),
        "one banner key per EngineError variant — the two lists must stay in step"
    );

    for err in &variants {
        let banner = banner_for(err);
        assert!(
            !banner.title.is_empty(),
            "{err:?} produced an empty headline"
        );
        assert!(
            !ENGINE_ERROR_KEYS.contains(&banner.title.as_str()),
            "{err:?} rendered its i18n key ({}) as the headline — the key is missing from en.json",
            banner.title
        );
        assert!(
            !banner.title.starts_with("engine.error."),
            "{err:?} headline looks like an unresolved key: {}",
            banner.title
        );
    }
}

#[test]
fn every_key_resolves_in_en_json() {
    for key in ENGINE_ERROR_KEYS {
        let resolved = dat0_i18n::t(key);
        assert_ne!(
            &resolved, key,
            "`{key}` has no entry in crates/dat0-i18n/src/strings/en.json — t() echoed the key"
        );
        assert!(
            resolved.len() > key.len() / 2,
            "`{key}` resolves to a suspiciously short string: {resolved:?}"
        );
    }
}

#[test]
fn engine_error_keys_has_no_duplicates() {
    let mut sorted: Vec<&&str> = ENGINE_ERROR_KEYS.iter().collect();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(
        before,
        sorted.len(),
        "a duplicated key means a variant maps to another variant's headline"
    );
}

/// Every fault renders red; a cancellation does not.
///
/// `Interrupted` is the user getting exactly what Cmd+. asked for. Painting the
/// window red for it would train people to ignore red.
#[test]
fn faults_are_errors_and_cancellation_is_info() {
    assert_eq!(banner_for(&EngineError::Interrupted).kind, BannerKind::Info);
    for err in every_variant() {
        if matches!(err, EngineError::Interrupted) {
            continue;
        }
        assert_eq!(
            banner_for(&err).kind,
            BannerKind::Error,
            "{err:?} should render as an error banner"
        );
    }
}

/// The driver's own text must survive into the banner body, or a bug report
/// carries only the translated generality.
#[test]
fn body_carries_the_raw_driver_text() {
    let err = EngineError::UnsupportedFormat("xlsx".into());
    let banner = banner_for(&err);
    assert!(
        banner.body.contains("xlsx"),
        "body dropped the specifics: {:?}",
        banner.body
    );
}
