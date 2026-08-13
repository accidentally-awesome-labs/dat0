//! Phase 0.2 — CodeMirror 6 embed spike.
//!
//! Proves the SQL editor is tractable inside `dioxus-desktop`: a locally built
//! CodeMirror bundle is served from Rust over the `dat0` custom asset protocol,
//! mounted into a `div`, and driven over a bidirectional `document::eval`
//! channel.
//!
//! **Acceptance:** typing `SELECT * FROM ` pops a completion list containing a
//! table name that was supplied from Rust, and the Rust side observes the
//! resulting document text.
//!
//! The check is automated rather than eyeballed: the driver uses the bundle's
//! test seams (`dat0cm.type` / `complete` / `completions` / `doc`) to type,
//! open the popup, and report back. The two assertions are made in Rust on the
//! values that came back over the channel.
//!
//! Run (from `crates/dat0-ui`, a detached workspace during the migration):
//!
//! ```text
//! cargo run --release --example editor_spike
//! ```
//!
//! Exits 0 on PASS, 1 on FAIL.

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::wry::http::Response;
use dioxus::desktop::{Config, WindowBuilder, use_asset_handler};
use dioxus::prelude::*;
use serde_json::json;

/// Schema pushed from Rust. The acceptance check looks for this table name in
/// the completion popup, so it must not be something CodeMirror could invent.
const TABLE: &str = "nyc_taxi_trips";
const COLUMNS: [&str; 4] = ["vendor_id", "pickup_at", "fare_amount", "tip_amount"];

/// A slice of `dat0_core::query::completion::duckdb_functions()`, stubbed here
/// so the spike has no dependency on the app crates.
const FUNCTIONS: [&str; 5] = [
    "list_aggregate",
    "regexp_extract",
    "strftime",
    "date_trunc",
    "approx_count_distinct",
];

const INITIAL_DOC: &str = "-- dat0 SQL console\n";

const CSS: &str = r#"
@font-face {
  font-family: 'Geist Mono';
  src: url('/dat0/fonts/GeistMono-Regular.ttf') format('truetype');
  font-weight: 400;
  font-display: block;
}
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; background: #fcfcfb; }
#editor { position: absolute; inset: 0; background: #ffffff; }
.cm-editor { height: 100%; }
"#;

/// Boots the bundle, mounts an editor, then exercises it and reports back.
///
/// Everything after `init` waits for the previous stage's observable effect
/// rather than sleeping and hoping.
///
/// The first act is `dat0cm.bind(dioxus)`: `document::eval` hands the script a
/// *scoped* `dioxus` object that is not published on `window`, so a bundle
/// loaded via `<script src>` has no way back to Rust unless the channel is
/// handed to it explicitly. Everything the bundle pushes — `ready`, `change`,
/// `cursor`, `run` — then travels on this one channel, interleaved with the
/// final `report`, so Rust drains until it sees the report.
const DRIVER_JS: &str = r#"
const load = (src) => new Promise((ok, err) => {
  const s = document.createElement("script");
  s.src = src;
  s.onload = ok;
  s.onerror = () => err(new Error("failed to load " + src));
  document.head.appendChild(s);
});

const settle = () => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));

async function waitFor(pred, label, tries = 200) {
  for (let i = 0; i < tries; i++) {
    if (pred()) return true;
    await settle();
  }
  throw new Error("timed out waiting for " + label);
}

try {
  await load("/dat0/codemirror.js");
  await waitFor(() => typeof window.dat0cm !== "undefined", "dat0cm global");
  window.dat0cm.bind(dioxus);

  // Rust -> JS: everything the editor needs, including the schema map.
  // `dioxus.recv()` resolves to the already-deserialized value, not a JSON string.
  const init = await dioxus.recv();
  if (!window.dat0cm.handle(init)) throw new Error("init rejected");
  await waitFor(() => window.dat0cm.doc(init.id) !== null, "editor mount");

  // Type the trigger. `dat0cm.type` dispatches a real `input.type` transaction,
  // so lang-sql's completion machinery sees exactly what a keystroke produces.
  window.dat0cm.type(init.id, "SELECT * FROM ");
  await settle();
  window.dat0cm.complete(init.id);
  await waitFor(() => window.dat0cm.completions(init.id) !== null, "completion popup");

  const labels = window.dat0cm.completions(init.id);

  // And a second round: partially type a function name and check the
  // Rust-supplied function catalogue is reachable too.
  window.dat0cm.type(init.id, "\nSELECT date_tr");
  await settle();
  window.dat0cm.complete(init.id);
  await waitFor(() => window.dat0cm.completions(init.id) !== null, "function popup");
  const fnLabels = window.dat0cm.completions(init.id);

  // Mod-Enter through CodeMirror's own keymap must surface as a `run` message.
  const handled = window.dat0cm.key(init.id, "Enter", { meta: true });
  await settle();

  dioxus.send({
    t: "report",
    ok: true,
    run_key_handled: handled,
    table_completions: labels,
    fn_completions: fnLabels,
    doc: window.dat0cm.doc(init.id)
  });
} catch (e) {
  dioxus.send({ t: "report", ok: false, error: String(e && e.message ? e.message : e) });
}
"#;

/// Everything the bundle pushes on the JS -> Rust channel.
#[derive(serde::Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Incoming {
    Ready {
        #[allow(dead_code)]
        id: String,
    },
    Change {
        #[allow(dead_code)]
        id: String,
        doc: String,
    },
    Cursor {
        #[allow(dead_code)]
        id: String,
        line: usize,
        col: usize,
    },
    Run {
        #[allow(dead_code)]
        id: String,
        doc: String,
    },
    Report(Report),
}

#[derive(serde::Deserialize, Debug, Default)]
struct Report {
    ok: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    run_key_handled: bool,
    #[serde(default)]
    table_completions: Vec<String>,
    #[serde(default)]
    fn_completions: Vec<String>,
    #[serde(default)]
    doc: String,
}

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(
                    WindowBuilder::new()
                        .with_title("dat0 — CodeMirror embed spike")
                        .with_inner_size(LogicalSize::new(900.0, 520.0)),
                )
                .with_background_color((0xff, 0xff, 0xff, 0xff)),
        )
        .launch(app);
}

/// Phase 2.3 in miniature: the bundle and the font come out of the process, not
/// off disk. In the shipped app these bytes come from `rust-embed`; the spike
/// reads them from the source tree so a rebuild of the bundle is picked up
/// without recompiling Rust.
fn asset_bytes(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
    match path.rsplit('/').next()? {
        "codemirror.js" => std::fs::read(format!("{root}/codemirror.js"))
            .ok()
            .map(|b| ("text/javascript", b)),
        "GeistMono-Regular.ttf" => std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../dat0-app/assets/fonts/GeistMono-Regular.ttf"
        ))
        .ok()
        .map(|b| ("font/ttf", b)),
        _ => None,
    }
}

fn app() -> Element {
    use_asset_handler("dat0", move |req, resp| {
        match asset_bytes(req.uri().path()) {
            Some((mime, bytes)) => resp.respond(
                Response::builder()
                    .header("Content-Type", mime)
                    .body(bytes)
                    .unwrap(),
            ),
            None => resp.respond(
                Response::builder()
                    .status(404)
                    .body(Vec::<u8>::new())
                    .unwrap(),
            ),
        }
    });

    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(DRIVER_JS);

            let init = json!({
                "t": "init",
                "id": "console-0",
                "mount": "editor",
                "doc": INITIAL_DOC,
                "schema": { TABLE: COLUMNS },
                "functions": FUNCTIONS,
                "vars": { "mode": "light" },
            });
            if let Err(e) = eval.send(init) {
                eprintln!("failed to send init: {e}");
                std::process::exit(2);
            }

            // Drain the channel. The bundle's own push messages (`ready`,
            // `change`, `cursor`, `run`) interleave with the driver's final
            // report, and observing them *is* half the acceptance criterion:
            // it proves the JS -> Rust direction works from inside the editor,
            // not just from the driver script.
            let mut ready = false;
            let mut changes: Vec<String> = Vec::new();
            let mut cursors: Vec<(usize, usize)> = Vec::new();
            let mut runs: Vec<String> = Vec::new();
            let r: Report = loop {
                match eval.recv::<Incoming>().await {
                    Ok(Incoming::Ready { .. }) => ready = true,
                    Ok(Incoming::Change { doc, .. }) => changes.push(doc),
                    Ok(Incoming::Cursor { line, col, .. }) => cursors.push((line, col)),
                    Ok(Incoming::Run { doc, .. }) => runs.push(doc),
                    Ok(Incoming::Report(r)) => break r,
                    Err(e) => {
                        eprintln!("driver channel failed: {e}");
                        std::process::exit(2);
                    }
                }
            };

            println!("--- dat0 CodeMirror embed spike ---");
            if !r.ok {
                println!("driver error: {}", r.error);
                std::process::exit(1);
            }

            let doc_ok = r.doc.starts_with(INITIAL_DOC) && r.doc.contains("SELECT * FROM ");
            let table_ok = r.table_completions.iter().any(|c| c == TABLE);
            let fn_ok = r.fn_completions.iter().any(|c| c == "date_trunc");
            let push_ok =
                ready && changes.last().is_some_and(|d| *d == r.doc) && !cursors.is_empty();
            let run_ok = r.run_key_handled && runs.last().is_some_and(|d| *d == r.doc);

            println!(
                "completions after `SELECT * FROM `: {} offered -> {}",
                r.table_completions.len(),
                preview(&r.table_completions)
            );
            println!(
                "completions after `date_tr`:        {} offered -> {}",
                r.fn_completions.len(),
                preview(&r.fn_completions)
            );
            println!("document observed from Rust:        {:?}", r.doc);
            println!(
                "pushed from the editor:             ready={ready} change x{} cursor x{} run x{} (last cursor {:?})",
                changes.len(),
                cursors.len(),
                runs.len(),
                cursors.last()
            );
            println!("  Rust-supplied table `{TABLE}` in popup: {table_ok}");
            println!("  Rust-supplied function `date_trunc` in popup: {fn_ok}");
            println!("  document round-tripped to Rust: {doc_ok}");
            println!("  editor -> Rust push channel (ready/change/cursor): {push_ok}");
            println!("  Mod-Enter through CodeMirror's keymap -> `run`: {run_ok}");

            let pass = table_ok && fn_ok && doc_ok && push_ok && run_ok;
            println!("ACCEPTANCE: {}", if pass { "PASS" } else { "FAIL" });
            std::process::exit(i32::from(!pass));
        });
    });

    rsx! {
        style { dangerous_inner_html: CSS }
        div { id: "editor" }
    }
}

fn preview(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(8).map(String::as_str).collect();
    if items.len() > shown.len() {
        format!("[{}, … +{}]", shown.join(", "), items.len() - shown.len())
    } else {
        format!("[{}]", shown.join(", "))
    }
}
