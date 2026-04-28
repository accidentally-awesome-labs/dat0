//! dat0-fixtures: generate deterministic large fixtures for engine tests.
//!
//! CSV via direct write (fastest). Parquet via DuckDB COPY ... TO (no Arrow
//! workspace dep). SQLite via rusqlite.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Parser, Debug)]
#[command(name = "dat0-fixtures")]
struct Cli {
    /// Output directory. Files written: generated.csv, generated.parquet, generated.sqlite.
    #[arg(long)]
    out: PathBuf,
    /// Deterministic seed (default 42).
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// CSV target bytes (default 1 GiB).
    #[arg(long, default_value_t = 1_073_741_824)]
    csv_bytes: u64,
    /// Parquet target bytes (default 500 MiB). Approx — DuckDB compresses.
    #[arg(long, default_value_t = 524_288_000)]
    parquet_target_rows: u64,
    /// SQLite target bytes (default 100 MiB).
    #[arg(long, default_value_t = 104_857_600)]
    sqlite_target_bytes: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.out).context("create out dir")?;

    println!("dat0-fixtures: generating into {}", cli.out.display());
    let started = std::time::Instant::now();

    let csv_path = cli.out.join("generated.csv");
    write_csv(&csv_path, cli.seed, cli.csv_bytes)?;

    let parquet_path = cli.out.join("generated.parquet");
    write_parquet_via_duckdb(&csv_path, &parquet_path)?;

    let sqlite_path = cli.out.join("generated.sqlite");
    write_sqlite(&sqlite_path, cli.seed, cli.sqlite_target_bytes)?;

    println!(
        "dat0-fixtures: done in {:?}; csv {} MB, parquet {} MB, sqlite {} MB",
        started.elapsed(),
        std::fs::metadata(&csv_path)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0),
        std::fs::metadata(&parquet_path)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0),
        std::fs::metadata(&sqlite_path)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0),
    );
    Ok(())
}

fn write_csv(path: &Path, seed: u64, target_bytes: u64) -> Result<()> {
    let f = std::fs::File::create(path).context("create csv")?;
    let mut w = BufWriter::with_capacity(8 * 1024 * 1024, f);
    writeln!(
        w,
        "id,name,score,flag,date,city,department,quantity,unit_price,note"
    )?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let cities = ["new_york", "london", "tokyo", "berlin", "paris", "sydney"];
    let depts = ["sales", "engineering", "marketing", "ops", "finance"];
    let mut written: u64 = 0;
    let mut id: u64 = 0;
    while written < target_bytes {
        id += 1;
        // Note: `r#gen` escapes `gen` because Rust edition 2024 reserves `gen`
        // as a keyword; rand 0.8 still names the trait method `gen` (see PD-006).
        let name = format!("item_{:08x}", rng.r#gen::<u32>());
        let score: f64 = rng.r#gen::<f64>() * 1000.0;
        let flag = rng.gen_bool(0.6);
        let day = 1 + (rng.r#gen::<u32>() % 28);
        let month = 1 + (rng.r#gen::<u32>() % 12);
        let year = 2020 + (rng.r#gen::<u32>() % 6);
        let city = cities[rng.gen_range(0..cities.len())];
        let dept = depts[rng.gen_range(0..depts.len())];
        let qty: u32 = rng.gen_range(0..1000);
        let unit: f64 = rng.r#gen::<f64>() * 100.0;
        let note = if rng.gen_bool(0.05) { "" } else { "ok" };
        let line = format!(
            "{},{},{:.4},{},{:04}-{:02}-{:02},{},{},{},{:.4},{}\n",
            id, name, score, flag, year, month, day, city, dept, qty, unit, note
        );
        w.write_all(line.as_bytes())?;
        written += line.len() as u64;
    }
    w.flush()?;
    Ok(())
}

fn write_parquet_via_duckdb(csv_in: &Path, parquet_out: &Path) -> Result<()> {
    let conn = duckdb::Connection::open_in_memory()?;
    let copy_sql = format!(
        "COPY (SELECT * FROM read_csv('{}')) TO '{}' (FORMAT PARQUET);",
        csv_in.display().to_string().replace('\'', "''"),
        parquet_out.display().to_string().replace('\'', "''"),
    );
    conn.execute_batch(&copy_sql)?;
    Ok(())
}

fn write_sqlite(path: &Path, seed: u64, target_bytes: u64) -> Result<()> {
    let _ = std::fs::remove_file(path); // start fresh
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE items (
             id INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             score REAL,
             flag INTEGER,
             city TEXT,
             department TEXT,
             quantity INTEGER,
             unit_price REAL
         );",
    )?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let cities = ["new_york", "london", "tokyo", "berlin", "paris", "sydney"];
    let depts = ["sales", "engineering", "marketing", "ops", "finance"];
    let tx = conn.unchecked_transaction()?;
    let mut id: i64 = 0;
    let mut last_size = 0_u64;
    let mut stmt = tx.prepare(
        "INSERT INTO items (id, name, score, flag, city, department, quantity, unit_price)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    loop {
        for _ in 0..10_000 {
            id += 1;
            // `r#gen` raw-identifier escape: see PD-006 note above.
            stmt.execute(rusqlite::params![
                id,
                format!("item_{:08x}", rng.r#gen::<u32>()),
                rng.r#gen::<f64>() * 1000.0,
                if rng.gen_bool(0.6) { 1 } else { 0 },
                cities[rng.gen_range(0..cities.len())],
                depts[rng.gen_range(0..depts.len())],
                rng.gen_range(0..1000_i64),
                rng.r#gen::<f64>() * 100.0,
            ])?;
        }
        // Re-prepare to flush. Inspect file size every 10k inserts.
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size >= target_bytes {
            break;
        }
        if size == last_size {
            // Forward progress safeguard
            break;
        }
        last_size = size;
    }
    drop(stmt);
    tx.commit()?;
    Ok(())
}
