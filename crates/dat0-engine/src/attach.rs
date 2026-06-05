//! ATTACH/DETACH. T11a covers DSN dispatch; T11b covers sqlite_scanner end-to-end.

use crate::Result;
use crate::catalog::quote_ident;
use crate::error::EngineError;
use crate::types::AttachOpts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachScheme {
    Sqlite,
    MotherDuck,
}

pub(crate) fn parse_scheme(dsn: &str) -> Result<(AttachScheme, &str)> {
    if let Some(rest) = dsn.strip_prefix("sqlite:") {
        return Ok((AttachScheme::Sqlite, rest));
    }
    if let Some(rest) = dsn.strip_prefix("md:") {
        return Ok((AttachScheme::MotherDuck, rest));
    }
    Err(EngineError::UnknownAttachScheme(
        dsn.split(':').next().unwrap_or("?").to_string(),
    ))
}

pub(crate) fn build_attach_sqlite_sql(path: &str, alias: &str, opts: &AttachOpts) -> String {
    let read_only = if opts.read_only { ", READ_ONLY" } else { "" };
    format!(
        "ATTACH '{}' AS {} (TYPE SQLITE{});",
        path.replace('\'', "''"),
        quote_ident(alias),
        read_only
    )
}

pub(crate) fn build_detach_sql(alias: &str) -> String {
    format!("DETACH {};", quote_ident(alias))
}

// T3 wires the call site; suppress until then.
#[allow(dead_code)]
pub(crate) fn build_attach_md_sql(alias: &str, opts: &AttachOpts) -> String {
    // Caller guarantees opts.token is Some (attach() md-arm checks). Escape the
    // token as a SQL string literal; never log it.
    let token = opts.token.as_deref().unwrap_or_default().replace('\'', "''");
    format!(
        "SET motherduck_token = '{}'; ATTACH 'md:' AS {};",
        token,
        quote_ident(alias)
    )
}

#[cfg(test)]
mod md_sql_tests {
    use crate::types::AttachOpts;

    #[test]
    fn build_md_sql_sets_token_then_attaches() {
        let opts = AttachOpts { token: Some("tok'123".into()), ..Default::default() };
        let sql = super::build_attach_md_sql("md", &opts);
        // Token single-quote escaped; alias quoted; SET precedes ATTACH.
        assert!(sql.contains("SET motherduck_token = 'tok''123';"));
        assert!(sql.contains("ATTACH 'md:' AS \"md\";"));
        assert!(sql.find("SET motherduck_token").unwrap() < sql.find("ATTACH").unwrap());
    }

    #[test]
    fn attach_opts_debug_redacts_token() {
        let opts = AttachOpts { token: Some("SECRET".into()), ..Default::default() };
        let dbg = format!("{opts:?}");
        assert!(!dbg.contains("SECRET"), "token leaked into Debug: {dbg}");
    }
}
