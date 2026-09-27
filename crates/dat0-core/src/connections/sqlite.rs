//! SQLite files, attached read-only (PD-023, step 5.4e).
//!
//! A SQLite file is attached to a session's engine under an alias, read-only:
//! dat0 lays its edits over a table and never writes one, so it has nothing to
//! write there. Its tables are listed by [`QueryEngine::attached_tables`]. One
//! opened as a tab becomes a view of the session's own over the attached table
//! ([`open_table`]), so everything that reads a table by name reads it.
//!
//! An attachment lasts as long as the engine's connection, and a view over an
//! attached table cannot be read without it. So a session opened again
//! attaches its files again ([`reattach`]) before anything reads its tables.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use dat0_engine::{AttachOpts, QueryEngine, quote_ident};

use crate::session::{PersistedAttachment, PersistedAttachmentKind};

/// The first 16 bytes of every SQLite database file.
const HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Names an attachment must not take: DuckDB's own catalogs, the names a
/// session's database takes (its file's stem), and MotherDuck's.
const RESERVED: &[&str] = &[
    "main",
    "memory",
    "system",
    "temp",
    "scratch",
    "workspace",
    super::MD_ALIAS,
];

/// Whether the file at `path` is a SQLite database, by its header rather than
/// its name: a `.db` file may be anything, a DuckDB database included.
pub fn is_sqlite(path: &Path) -> bool {
    let mut head = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && &head == HEADER
}

/// An identifier made of `text`: ASCII letters, digits and `_`, the rest `_`.
fn identifier(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// The alias to attach `path` under: its stem as a lower-case identifier, and
/// not one of `taken` nor one [`RESERVED`].
pub fn alias_for(path: &Path, taken: &[String]) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut base = identifier(&stem).to_ascii_lowercase();
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
        base.insert_str(0, "db_");
    }
    let free = |name: &str| {
        !RESERVED.contains(&name) && !taken.iter().any(|t| t.eq_ignore_ascii_case(name))
    };
    if free(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|name| free(name))
        .expect("some suffix is free")
}

/// The name of the view [`open_table`] makes for `table` of `alias`: both, as
/// one identifier, so it says where its rows live and is the same each time.
pub fn view_name(alias: &str, table: &str) -> String {
    format!("{alias}_{}", identifier(table))
}

/// Attach the SQLite file at `path` to `engine` as `alias`, read-only, unless
/// that alias is attached already. Returns the file's tables.
pub async fn attach(engine: &dyn QueryEngine, path: &Path, alias: &str) -> Result<Vec<String>> {
    if !is_attached(engine, alias).await? {
        engine
            .attach(
                &format!("sqlite:{}", path.display()),
                alias,
                AttachOpts {
                    read_only: true,
                    schema_filter: None,
                    token: None,
                },
            )
            .await
            .with_context(|| format!("attach {}", path.display()))?;
    }
    Ok(engine.attached_tables(alias).await?)
}

/// Whether a database is attached to `engine` as `alias`.
async fn is_attached(engine: &dyn QueryEngine, alias: &str) -> Result<bool> {
    let sql = format!(
        "SELECT 1 FROM duckdb_databases() WHERE database_name = '{}'",
        alias.replace('\'', "''")
    );
    let found = engine.execute(&sql).await?;
    Ok(found.batches.iter().any(|b| b.num_rows() > 0))
}

/// Open `table` of the database attached as `alias` as a table of the
/// session's own: a view over it, named by [`view_name`]. Opening it again
/// makes the same view. Returns the view's name.
pub async fn open_table(engine: &dyn QueryEngine, alias: &str, table: &str) -> Result<String> {
    let name = view_name(alias, table);
    engine
        .execute(&format!(
            "CREATE OR REPLACE VIEW {} AS SELECT * FROM {}.{}",
            quote_ident(&name),
            quote_ident(alias),
            quote_ident(table)
        ))
        .await
        .with_context(|| format!("open {alias}.{table}"))?;
    Ok(name)
}

/// The SQLite attachment in `attachments` for the file at `path`, when there
/// is one.
pub fn recorded<'a>(attachments: &'a [PersistedAttachment], path: &Path) -> Option<&'a str> {
    attachments.iter().find_map(|a| match &a.kind {
        PersistedAttachmentKind::Sqlite { path: p } if Path::new(p) == path => {
            Some(a.alias.as_str())
        }
        _ => None,
    })
}

/// Attach the SQLite files in `attachments` to `engine` again. Returns each
/// file that could not be attached, with why: a missing file is not fatal,
/// only its views go unread until it is back.
pub async fn reattach(
    engine: &dyn QueryEngine,
    attachments: &[PersistedAttachment],
) -> Vec<(PathBuf, anyhow::Error)> {
    let mut failed = Vec::new();
    for a in attachments {
        let PersistedAttachmentKind::Sqlite { path } = &a.kind else {
            continue;
        };
        let path = PathBuf::from(path);
        if let Err(e) = attach(engine, &path, &a.alias).await {
            failed.push((path, e));
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sqlite_file_is_known_by_its_header_not_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let (real, fake) = (dir.path().join("a.csv"), dir.path().join("b.sqlite"));
        std::fs::write(&real, b"SQLite format 3\0and the rest").unwrap();
        std::fs::write(&fake, b"id,name\n1,x\n").unwrap();
        assert!(is_sqlite(&real));
        assert!(!is_sqlite(&fake));
        assert!(!is_sqlite(&dir.path().join("missing.db")));
    }

    #[test]
    fn an_alias_is_the_stem_as_an_identifier_and_never_taken_or_reserved() {
        let at = |p: &str, taken: &[&str]| {
            let taken: Vec<String> = taken.iter().map(|s| s.to_string()).collect();
            alias_for(Path::new(p), &taken)
        };
        assert_eq!(at("/x/Chinook.sqlite", &[]), "chinook");
        assert_eq!(at("/x/my data-2024.db", &[]), "my_data_2024");
        assert_eq!(at("/x/2024.db", &[]), "db_2024");
        assert_eq!(at("/x/chinook.sqlite", &["chinook"]), "chinook_2");
        assert_eq!(
            at("/x/chinook.sqlite", &["Chinook", "chinook_2"]),
            "chinook_3"
        );
        assert_eq!(at("/x/main.db", &[]), "main_2");
        assert_eq!(at("/x/workspace.sqlite", &[]), "workspace_2");
    }

    #[test]
    fn a_view_name_says_where_its_rows_live() {
        assert_eq!(view_name("chinook", "Album"), "chinook_Album");
        assert_eq!(
            view_name("chinook", "Invoice Lines/2024"),
            "chinook_Invoice_Lines_2024"
        );
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/small/simple.sqlite")
    }

    async fn engine(dir: &Path) -> dat0_engine::DuckDBEngine {
        dat0_engine::extension_bootstrap::__test_install_sqlite_scanner().expect("sqlite_scanner");
        let e = dat0_engine::DuckDBEngine::new(
            dir.join("s.duckdb"),
            dat0_engine::MemoryBudget {
                bytes: 256 * 1024 * 1024,
            },
        )
        .unwrap();
        e.init().await.unwrap();
        e
    }

    #[tokio::test]
    async fn an_attached_table_opens_as_a_view_of_the_sessions_own() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path()).await;
        assert_eq!(attach(&engine, &fixture(), "sq").await.unwrap(), ["items"]);
        // Attaching the same alias again is not an error.
        assert_eq!(attach(&engine, &fixture(), "sq").await.unwrap(), ["items"]);

        let name = open_table(&engine, "sq", "items").await.unwrap();
        assert_eq!(name, "sq_items");
        assert_eq!(
            open_table(&engine, "sq", "items").await.unwrap(),
            name,
            "the same view"
        );
        let tables = engine.get_tables().await.unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "sq_items");
        engine.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_file_that_is_gone_is_reported_and_the_rest_attach() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path()).await;
        let sqlite = |alias: &str, path: &Path| PersistedAttachment {
            alias: alias.to_string(),
            kind: PersistedAttachmentKind::Sqlite {
                path: path.display().to_string(),
            },
        };
        let gone = dir.path().join("gone.sqlite");
        let failed = reattach(
            &engine,
            &[
                sqlite("gone", &gone),
                PersistedAttachment {
                    alias: super::super::MD_ALIAS.to_string(),
                    kind: PersistedAttachmentKind::Md,
                },
                sqlite("sq", &fixture()),
            ],
        )
        .await;
        let paths: Vec<&PathBuf> = failed.iter().map(|(p, _)| p).collect();
        assert_eq!(paths, [&gone]);
        assert_eq!(engine.attached_tables("sq").await.unwrap(), ["items"]);
        engine.close().await.unwrap();
    }

    #[test]
    fn the_attachment_for_a_file_is_found_by_its_path() {
        let a = [PersistedAttachment {
            alias: "chinook".to_string(),
            kind: PersistedAttachmentKind::Sqlite {
                path: "/x/chinook.sqlite".to_string(),
            },
        }];
        assert_eq!(
            recorded(&a, Path::new("/x/chinook.sqlite")),
            Some("chinook")
        );
        assert_eq!(recorded(&a, Path::new("/y/chinook.sqlite")), None);
    }
}
