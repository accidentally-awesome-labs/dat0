//! Writer — turns a [`PackageContents`] + live engine into a `.dat0` zip package.
//!
//! Each base/derived table's data is exported to Parquet via the engine's
//! `COPY (...) TO ... (FORMAT PARQUET)` path (`export_query_to_path`), then
//! stored uncompressed (Parquet is already compressed) alongside the JSON
//! sidecars. Every entry's sha256 is recorded in `manifest.json` so the reader
//! can verify integrity (PD-014 self-describing manifest).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use dat0_engine::{ExportFormat, QueryEngine};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use crate::error::{FormatError, Result};
use crate::model::{PackageContents, PackageManifest};
use crate::{FORMAT_VERSION, PACKAGE_KIND};

/// Quote a DuckDB identifier: wrap in double quotes, doubling any internal
/// double-quote. Intentionally inlined (rather than `use dat0_engine::quote_ident`)
/// to keep this pure format crate decoupled from the engine's identifier helper.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Writes a `.dat0` package to disk from portable contents + a live engine.
pub struct Writer;

impl Writer {
    /// Serialize `contents` into a `.dat0` zip at `dest`.
    ///
    /// Data bytes are pulled from `engine` per `RecipeTable.name` (a plain
    /// `SELECT *` Parquet export — T0 spike proved Parquet round-trips DuckDB
    /// types faithfully, so no CAST-pinning is needed).
    ///
    /// # Errors
    /// - [`FormatError::Io`] — `dest` unwritable, or a temp parquet unreadable.
    /// - [`FormatError::Engine`] — a table export failed.
    /// - [`FormatError::Zip`] / [`FormatError::Json`] — zip/serialize failures.
    pub async fn write(
        contents: &PackageContents,
        engine: &dyn QueryEngine,
        dest: &Path,
    ) -> Result<()> {
        let tmp = tempfile::tempdir().map_err(|e| FormatError::Io {
            path: dest.into(),
            source: e,
        })?;
        let file = std::fs::File::create(dest).map_err(|e| FormatError::Io {
            path: dest.into(),
            source: e,
        })?;
        let mut zip = zip::ZipWriter::new(file);
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut checksums: BTreeMap<String, String> = BTreeMap::new();

        // 1. Export each table's data to Parquet (Stored — already compressed).
        for t in &contents.recipe.tables {
            let pq = tmp.path().join(format!("{}.parquet", t.name));
            let select = format!("SELECT * FROM {}", quote_ident(&t.name));
            engine
                .export_query_to_path(&select, ExportFormat::Parquet, &pq)
                .await?;
            let bytes = std::fs::read(&pq).map_err(|e| FormatError::Io {
                path: pq.clone(),
                source: e,
            })?;
            let entry = t.data.clone(); // "data/<name>.parquet"
            zip.start_file(&entry, stored)?;
            zip.write_all(&bytes).map_err(|e| FormatError::Io {
                path: dest.into(), // write target is the zip, not the temp source
                source: e,
            })?;
            checksums.insert(entry, format!("sha256:{:x}", Sha256::digest(&bytes)));
        }

        // 2. JSON sidecars (Deflated). recipe.json is the only one checksummed
        //    here (it is the load-bearing portable recipe).
        let recipe_bytes = serde_json::to_vec_pretty(&contents.recipe)?;
        checksums.insert(
            "recipe.json".into(),
            format!("sha256:{:x}", Sha256::digest(&recipe_bytes)),
        );
        write_json(&mut zip, "recipe.json", &recipe_bytes, deflated)?;
        write_json(
            &mut zip,
            "sources.json",
            &serde_json::to_vec_pretty(&contents.sources)?,
            deflated,
        )?;
        write_json(
            &mut zip,
            "views.json",
            &serde_json::to_vec_pretty(&contents.views)?,
            deflated,
        )?;
        write_json(
            &mut zip,
            "queries.json",
            &serde_json::to_vec_pretty(&contents.queries)?,
            deflated,
        )?;

        // 3. Manifest LAST, so checksums for all prior entries are populated.
        let manifest = PackageManifest {
            format_version: FORMAT_VERSION,
            kind: PACKAGE_KIND.into(),
            dat0_version: env!("CARGO_PKG_VERSION").into(),
            package_id: uuid::Uuid::now_v7(),
            workspace_id: contents.workspace_id,
            created_at: contents.created_at.clone(),
            table_count: contents.recipe.tables.len() as u32,
            checksums,
        };
        write_json(
            &mut zip,
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest)?,
            deflated,
        )?;

        zip.finish()?;
        Ok(())
    }
}

/// Add a JSON sidecar entry to the zip.
fn write_json(
    zip: &mut zip::ZipWriter<std::fs::File>,
    name: &str,
    bytes: &[u8],
    opt: SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name, opt)?;
    zip.write_all(bytes).map_err(|e| FormatError::Io {
        path: name.into(),
        source: e,
    })?;
    Ok(())
}
