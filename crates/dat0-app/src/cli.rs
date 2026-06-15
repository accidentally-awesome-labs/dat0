//! P8 T4: headless CLI front-door for `.dat0` package subcommands.
//!
//! [`parse`] sniffs raw process args for a package subcommand
//! (`export|unpack|inspect|replay|diff`). Anything else — a bare launch or a
//! dropped file path — returns `None` so `main` falls through to the GUI.
//!
//! [`run`] executes a [`PackageCmd`] headlessly (no GPUI, no `AppLock`),
//! building its OWN tokio runtime and `block_on`-ing the async core. The
//! per-arm logic lives in `async fn` helpers so tests can drive them directly
//! with `#[tokio::test]` (calling `run` from inside a `#[tokio::test]` would
//! panic on the nested runtime).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Arg, ArgAction, Command};
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use serde::Serialize;

use crate::package;
use crate::session::Session;

/// Default engine memory budget for headless package ops (256 MiB).
const DEFAULT_BUDGET: u64 = 256 * 1024 * 1024;

/// A parsed package subcommand. All arms are implemented and dispatched by
/// [`run`]; the front-door [`parse`] turns raw argv into one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCmd {
    /// `dat0 export <workspace> -o <out.dat0>`
    Export { workspace: PathBuf, out: PathBuf },
    /// `dat0 unpack <package.dat0> <dir>`
    Unpack { package: PathBuf, dir: PathBuf },
    /// `dat0 inspect <package.dat0> [--json]`
    Inspect { package: PathBuf, json: bool },
    /// `dat0 replay <package.dat0> [--source k=v]... [-o <out.dat0>]`
    Replay {
        package: PathBuf,
        source: Vec<String>,
        out: Option<PathBuf>,
    },
    /// `dat0 diff <a.dat0> <b.dat0> [--json]`
    Diff { a: PathBuf, b: PathBuf, json: bool },
}

/// The set of recognized package subcommand verbs.
const VERBS: &[&str] = &["export", "unpack", "inspect", "replay", "diff"];

/// Detect a package subcommand from raw process args (`args[0]` is the binary).
///
/// Returns `None` when `args[1]` is absent or not a package verb — that path is
/// a GUI launch (bare, or a dropped file). When `args[1]` IS a verb, the
/// remaining args are parsed via clap; an argument error still yields
/// `Some(cmd)` is NOT possible (clap reports + the helper returns `None` only
/// for "not a verb"). To keep `main` simple, a malformed verb invocation prints
/// usage to stderr and exits the process directly here (clap's default behavior
/// for a parse error under a recognized subcommand).
pub fn parse(args: &[String]) -> Option<PackageCmd> {
    let verb = args.get(1)?;
    if !VERBS.contains(&verb.as_str()) {
        return None;
    }

    // Build a clap Command rooted at the recognized verb so error/usage text is
    // accurate. clap consumes argv-style input including a program name, so we
    // pass `args` through unchanged.
    let matches = cli_command().get_matches_from(args);
    let (name, sub) = matches.subcommand().expect("verb matched above");
    let cmd = match name {
        "export" => PackageCmd::Export {
            workspace: sub.get_one::<PathBuf>("workspace").cloned().unwrap(),
            out: sub.get_one::<PathBuf>("out").cloned().unwrap(),
        },
        "unpack" => PackageCmd::Unpack {
            package: sub.get_one::<PathBuf>("package").cloned().unwrap(),
            dir: sub.get_one::<PathBuf>("dir").cloned().unwrap(),
        },
        "inspect" => PackageCmd::Inspect {
            package: sub.get_one::<PathBuf>("package").cloned().unwrap(),
            json: sub.get_flag("json"),
        },
        "replay" => PackageCmd::Replay {
            package: sub.get_one::<PathBuf>("package").cloned().unwrap(),
            source: sub
                .get_many::<String>("source")
                .map(|v| v.cloned().collect())
                .unwrap_or_default(),
            out: sub.get_one::<PathBuf>("out").cloned(),
        },
        "diff" => PackageCmd::Diff {
            a: sub.get_one::<PathBuf>("a").cloned().unwrap(),
            b: sub.get_one::<PathBuf>("b").cloned().unwrap(),
            json: sub.get_flag("json"),
        },
        _ => unreachable!("verb set is closed"),
    };
    Some(cmd)
}

/// Build the clap `Command` describing every package subcommand.
fn cli_command() -> Command {
    Command::new("dat0")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("export")
                .about("Export a workspace to a .dat0 package")
                .arg(
                    Arg::new("workspace")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("out")
                        .short('o')
                        .long("out")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("unpack")
                .about("Unpack a .dat0 package into a workspace directory")
                .arg(
                    Arg::new("package")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("dir")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("inspect")
                .about("Print a .dat0 package's recipe summary")
                .arg(
                    Arg::new("package")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg(Arg::new("json").long("json").action(ArgAction::SetTrue)),
        )
        .subcommand(
            Command::new("replay")
                .about("Replay a .dat0 recipe against fresh sources")
                .arg(
                    Arg::new("package")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("source")
                        .long("source")
                        .action(ArgAction::Append)
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    Arg::new("out")
                        .short('o')
                        .long("out")
                        .value_parser(clap::value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("diff")
                .about("Diff two .dat0 packages")
                .arg(
                    Arg::new("a")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("b")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)),
                )
                .arg(Arg::new("json").long("json").action(ArgAction::SetTrue)),
        )
}

/// Run a package subcommand headlessly. Builds its OWN tokio runtime and
/// `block_on`s the async core (which also ENTERS the runtime so the DuckDB
/// engine's `spawn_blocking` works — P4b lesson). Returns a process exit code:
/// `0` success, `1` logical failure (e.g. a non-empty diff, T5), `2` error.
pub fn run(cmd: PackageCmd) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("dat0: failed to start runtime: {e}");
            return 2;
        }
    };
    rt.block_on(run_async(cmd))
}

/// Async dispatch core. Separated from [`run`] so `#[tokio::test]` can call it
/// without a nested runtime.
pub async fn run_async(cmd: PackageCmd) -> i32 {
    match cmd {
        PackageCmd::Export { workspace, out } => match export_async(&workspace, &out).await {
            Ok(()) => {
                println!("exported {} -> {}", workspace.display(), out.display());
                0
            }
            Err(e) => {
                eprintln!("dat0 export: {e:#}");
                2
            }
        },
        PackageCmd::Unpack { package, dir } => match unpack_async(&package, &dir).await {
            Ok(()) => {
                println!("unpacked {} -> {}", package.display(), dir.display());
                0
            }
            Err(e) => {
                eprintln!("dat0 unpack: {e:#}");
                2
            }
        },
        PackageCmd::Diff { a, b, json } => match diff_async(&a, &b, json).await {
            // `diff(1)`-style exit semantics: 0 = no differences, 1 = differences
            // found, 2 = error.
            Ok(empty) => {
                if empty {
                    0
                } else {
                    1
                }
            }
            Err(e) => {
                eprintln!("dat0 diff: {e:#}");
                2
            }
        },
        PackageCmd::Inspect { package, json } => match inspect_async(&package, json).await {
            Ok(output) => {
                println!("{output}");
                0
            }
            Err(e) => {
                eprintln!("dat0 inspect: {e:#}");
                2
            }
        },
        PackageCmd::Replay {
            package,
            source,
            out,
        } => match replay_async(&package, &source, out).await {
            Ok(out_path) => {
                println!("replayed -> {}", out_path.display());
                0
            }
            Err(e) => {
                eprintln!("dat0 replay: {e:#}");
                2
            }
        },
    }
}

/// EXPORT core: open the workspace read-side, map → contents, write the package.
pub async fn export_async(workspace: &std::path::Path, out: &std::path::Path) -> Result<()> {
    let sess = Session::recover_workspace(workspace.to_path_buf(), DEFAULT_BUDGET)
        .await
        .with_context(|| format!("open workspace {}", workspace.display()))?;
    let contents = package::session_to_contents(&sess)
        .await
        .context("map session to package contents")?;
    let write_result = dat0_format::Writer::write(&contents, sess.engine.as_ref(), out)
        .await
        .with_context(|| format!("write package {}", out.display()));
    // Release the engine + workspace flock before returning — on success AND on
    // a write error, so the flock never outlives this call.
    sess.engine.close().await.ok();
    write_result?;
    Ok(())
}

/// UNPACK core: parse the package, materialize a fresh `.dat0/` workspace.
pub async fn unpack_async(pkg: &std::path::Path, dir: &std::path::Path) -> Result<()> {
    let parsed = dat0_format::Reader::open(pkg)
        .with_context(|| format!("open package {}", pkg.display()))?;
    package::contents_to_workspace(&parsed, dir, DEFAULT_BUDGET)
        .await
        .with_context(|| format!("unpack into {}", dir.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// REPLAY
// ---------------------------------------------------------------------------

/// REPLAY core: rebind sources to new files and re-run derived tables.
///
/// Each `source` string must have the form `logical_name=path` (split on the
/// FIRST `=` so paths containing `=` are handled correctly). A malformed entry
/// returns a clear error. The replayed package is written to `out` if provided,
/// or a default `<package_stem>-replayed.dat0` next to the original package.
/// Returns the output path.
pub async fn replay_async(
    package: &std::path::Path,
    source: &[String],
    out: Option<PathBuf>,
) -> Result<PathBuf> {
    // Parse "logical=path" specs.
    let mut new_sources: HashMap<String, PathBuf> = HashMap::new();
    for spec in source {
        let (logical, path_str) = spec.split_once('=').ok_or_else(|| {
            anyhow!(
                "malformed --source spec {:?}: expected 'logical_name=path' (no '=' found)",
                spec
            )
        })?;
        if logical.is_empty() {
            bail!("malformed --source spec {:?}: logical name is empty", spec);
        }
        if path_str.is_empty() {
            bail!("malformed --source spec {:?}: path is empty", spec);
        }
        new_sources.insert(logical.to_string(), PathBuf::from(path_str));
    }

    let parsed = dat0_format::Reader::open(package)
        .with_context(|| format!("open package {}", package.display()))?;

    // Scratch engine — throwaway, closed + dropped before we return (P7a T6 lesson).
    let scratch_dir = tempfile::tempdir().context("create replay scratch dir")?;
    let engine = DuckDBEngine::new(
        scratch_dir.path().join("replay.duckdb"),
        MemoryBudget {
            bytes: DEFAULT_BUDGET,
        },
    )
    .context("create replay engine")?;
    engine.init().await.context("init replay engine")?;

    let new_contents = dat0_format::replay::ReplayEngine::replay(&parsed, &new_sources, &engine)
        .await
        .context("replay recipe")?;

    // Determine output path.
    let out_path = match out {
        Some(p) => p,
        None => {
            // Default: "<package_stem>-replayed.dat0" next to the original.
            let stem = package
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("package");
            let parent = package.parent().unwrap_or(std::path::Path::new("."));
            parent.join(format!("{stem}-replayed.dat0"))
        }
    };

    dat0_format::Writer::write(&new_contents, &engine, &out_path)
        .await
        .with_context(|| format!("write replayed package {}", out_path.display()))?;

    // Hygiene: close before the scratch_dir drops (mirrors export_async).
    engine.close().await.ok();

    Ok(out_path)
}

// ---------------------------------------------------------------------------
// INSPECT
// ---------------------------------------------------------------------------

/// A single row in the inspect tables list.
#[derive(Debug, Serialize)]
pub struct InspectTable {
    pub name: String,
    pub kind: String, // "base" or "derived"
    pub rows: u64,
    pub cols: usize,
}

/// A directed lineage edge derived from a [`Derivation`].
#[derive(Debug, Serialize)]
pub struct LineageEdge {
    pub table: String,  // the derived table
    pub parent: String, // one of its parent tables
}

/// A saved query entry in the inspect report.
#[derive(Debug, Serialize)]
pub struct InspectQuery {
    pub name: String,
    pub sql: String,
}

/// A saved chart entry in the inspect report.
#[derive(Debug, Serialize)]
pub struct InspectChart {
    pub name: String,
    pub chart_type: String,
    pub source: String,
}

/// The full inspect report (returned as JSON or rendered as text).
#[derive(Debug, Serialize)]
pub struct InspectReport {
    pub tables: Vec<InspectTable>,
    pub lineage: Vec<LineageEdge>,
    pub queries: Vec<InspectQuery>,
    pub charts: Vec<InspectChart>,
}

/// INSPECT core: open the package and produce a summary report string.
///
/// With `json = true` returns pretty-printed JSON; otherwise a human-readable
/// text tree. No engine required — reads recipe metadata only.
pub async fn inspect_async(package: &std::path::Path, json: bool) -> Result<String> {
    let parsed = dat0_format::Reader::open(package)
        .with_context(|| format!("open package {}", package.display()))?;

    // Build the tables list.
    let tables: Vec<InspectTable> = parsed
        .recipe
        .tables
        .iter()
        .map(|t| InspectTable {
            name: t.name.clone(),
            kind: match t.kind {
                dat0_format::TableKind::Base => "base".to_string(),
                dat0_format::TableKind::Derived => "derived".to_string(),
            },
            rows: t.row_count,
            cols: t.schema.len(),
        })
        .collect();

    // Build lineage edges from each derived table's Derivation.
    let mut lineage: Vec<LineageEdge> = Vec::new();
    for t in &parsed.recipe.tables {
        match &t.derivation {
            Some(dat0_format::Derivation::Sql { parents, .. }) => {
                for parent in parents {
                    lineage.push(LineageEdge {
                        table: t.name.clone(),
                        parent: parent.clone(),
                    });
                }
            }
            Some(dat0_format::Derivation::Transform { parent, .. }) => {
                lineage.push(LineageEdge {
                    table: t.name.clone(),
                    parent: parent.clone(),
                });
            }
            None => {}
        }
    }

    // Saved queries.
    let queries: Vec<InspectQuery> = parsed
        .queries
        .queries
        .iter()
        .map(|q| InspectQuery {
            name: q.name.clone(),
            sql: q.sql.clone(),
        })
        .collect();

    // Saved charts.
    let charts: Vec<InspectChart> = parsed
        .charts
        .charts
        .iter()
        .map(|c| InspectChart {
            name: c.name.clone(),
            chart_type: format!("{:?}", c.spec.chart_type).to_lowercase(),
            source: c.spec.source.clone(),
        })
        .collect();

    let report = InspectReport {
        tables,
        lineage,
        queries,
        charts,
    };

    if json {
        serde_json::to_string_pretty(&report).context("serialize inspect report")
    } else {
        Ok(render_inspect_text(&report))
    }
}

/// Render an [`InspectReport`] as a human-readable text tree.
fn render_inspect_text(report: &InspectReport) -> String {
    let mut out = String::new();

    out.push_str("Tables:\n");
    for t in &report.tables {
        out.push_str(&format!(
            "  {} ({}, {} rows, {} cols)\n",
            t.name, t.kind, t.rows, t.cols
        ));
    }

    if !report.lineage.is_empty() {
        out.push_str("\nLineage:\n");
        for edge in &report.lineage {
            out.push_str(&format!("  {} <- {}\n", edge.table, edge.parent));
        }
    }

    if !report.queries.is_empty() {
        out.push_str("\nSaved queries:\n");
        for q in &report.queries {
            // Truncate very long SQL for readability.
            let sql_preview: String = q.sql.chars().take(80).collect();
            let ellipsis = if q.sql.len() > 80 { "…" } else { "" };
            out.push_str(&format!("  {}: {}{}\n", q.name, sql_preview, ellipsis));
        }
    }

    if !report.charts.is_empty() {
        out.push_str("\nCharts:\n");
        for c in &report.charts {
            out.push_str(&format!("  {} ({}, {})\n", c.name, c.chart_type, c.source));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// DIFF
// ---------------------------------------------------------------------------

/// DIFF core: open both packages, compute the pure-JSON recipe diff, print it
/// (text or `--json`), and return whether the diff is EMPTY (so the caller maps
/// `true`→exit 0, `false`→exit 1). No engine, no parquet read — the diff is
/// metadata-only (`dat0_format::diff`).
pub async fn diff_async(a: &std::path::Path, b: &std::path::Path, json: bool) -> Result<bool> {
    let pa =
        dat0_format::Reader::open(a).with_context(|| format!("open package {}", a.display()))?;
    let pb =
        dat0_format::Reader::open(b).with_context(|| format!("open package {}", b.display()))?;
    let d = dat0_format::diff::diff(&pa, &pb);
    if json {
        let rendered =
            serde_json::to_string_pretty(&d.render_json()).context("serialize diff JSON")?;
        println!("{rendered}");
    } else {
        print!("{}", d.render_text());
    }
    Ok(d.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bare_launch_is_none() {
        assert_eq!(parse(&argv(&["dat0"])), None);
    }

    #[test]
    fn file_path_arg_is_none() {
        // A dropped file path (not a verb) → GUI launch.
        assert_eq!(parse(&argv(&["dat0", "/tmp/data.csv"])), None);
    }

    #[test]
    fn export_parses() {
        let cmd = parse(&argv(&["dat0", "export", "/ws", "-o", "/out.dat0"])).unwrap();
        assert_eq!(
            cmd,
            PackageCmd::Export {
                workspace: PathBuf::from("/ws"),
                out: PathBuf::from("/out.dat0"),
            }
        );
    }

    #[test]
    fn unpack_parses() {
        let cmd = parse(&argv(&["dat0", "unpack", "/p.dat0", "/dir"])).unwrap();
        assert_eq!(
            cmd,
            PackageCmd::Unpack {
                package: PathBuf::from("/p.dat0"),
                dir: PathBuf::from("/dir"),
            }
        );
    }

    #[test]
    fn replay_collects_repeatable_sources() {
        let cmd = parse(&argv(&[
            "dat0", "replay", "/p.dat0", "--source", "a=1", "--source", "b=2",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            PackageCmd::Replay {
                package: PathBuf::from("/p.dat0"),
                source: vec!["a=1".into(), "b=2".into()],
                out: None,
            }
        );
    }

    #[test]
    fn diff_parses_with_json_flag() {
        let cmd = parse(&argv(&["dat0", "diff", "/a.dat0", "/b.dat0", "--json"])).unwrap();
        assert_eq!(
            cmd,
            PackageCmd::Diff {
                a: PathBuf::from("/a.dat0"),
                b: PathBuf::from("/b.dat0"),
                json: true,
            }
        );
    }
}
