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
