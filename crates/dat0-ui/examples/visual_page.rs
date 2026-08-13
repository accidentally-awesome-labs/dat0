//! Render every scene to a self-contained HTML page, one per theme.
//!
//! ```text
//! cargo run -p dat0-ui --features visual --example visual_page
//! open target/visual/index.html
//! ```
//!
//! This is the human half of the visual gate. Dioxus has no official
//! visual-testing story — the framework's own desktop end-to-end suite drives
//! WebView2 over CDP with `--remote-debugging-port`, which exists only on
//! Windows; WKWebView and WebKitGTK expose no such endpoint, and dat0 does not
//! target Windows. What Dioxus *does* give us is `dioxus-ssr`, which renders
//! the real component tree with no window at all.
//!
//! So: SSR every scene in `dat0_ui::visual::SCENES`, inline the real
//! stylesheet, inline the real fonts as data URIs, and emit files that open
//! anywhere and need nothing from disk. A browser can screenshot them, a human
//! can eyeball them, and CI can do either without a display server. `index.html`
//! lists all of them by surface, so a review is one file open rather than 177.
//!
//! **What this does and does not prove.** It renders the same markup and the
//! same CSS the app serves, so it shows every visual regression that lives in
//! dat0's own code — a changed class, a dropped rule, a token that stopped
//! resolving. It does not exercise wry: WKWebView's font rasterisation and this
//! page's will differ in the last pixel, which is exactly why measurement lives
//! in `examples/visual_probe.rs`, in a real window, and why these pages are for
//! *appearance*.
//!
//! The machine-checked half of the same renders is `tests/visual_snapshot.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use dioxus::prelude::*;

use dat0_core::theme::tokens::BUILTIN_IDS;
use dat0_ui::visual::{Fixtures, Handle, SCENES, SceneHost, SceneHostProps};

fn main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("visual page tokio runtime")?;
    let _guard = rt.enter();
    let fx = Handle::new(rt.block_on(Fixtures::build())?);

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = crate_dir.join("../../target/visual");
    std::fs::create_dir_all(&out)?;

    let css = inline_fonts(
        &std::fs::read_to_string(crate_dir.join("assets/app.css"))?,
        &crate_dir.join("assets"),
    )?;

    // Surface -> the files written for it, in catalogue order.
    let mut index: BTreeMap<&'static str, Vec<(String, String)>> = BTreeMap::new();

    for scene in SCENES {
        for theme in BUILTIN_IDS {
            let mut dom = VirtualDom::new_with_props(
                SceneHost,
                SceneHostProps {
                    fx: fx.clone(),
                    id: scene.id.to_string(),
                    theme: theme.to_string(),
                },
            );
            dom.rebuild_in_place();
            let body = dioxus_ssr::render(&dom);

            let vars = dat0_core::theme::builtin(theme)
                .expect("a builtin theme")
                .css_vars();

            let title = format!("{} — {} — {theme}", scene.surface, scene.state);
            let page = format!(
                "<!doctype html>\n<html lang=\"en\" data-theme=\"{theme}\">\n<head>\n\
                 <meta charset=\"utf-8\">\n\
                 <title>dat0 — {}</title>\n\
                 <style>{vars}</style>\n\
                 <style>{css}</style>\n\
                 <style>html,body{{margin:0;padding:0}}</style>\n\
                 </head>\n<body>{body}</body>\n</html>\n",
                scene.id
            );

            let name = format!("{}__{theme}.html", scene.stem());
            std::fs::write(out.join(&name), page)?;
            index.entry(scene.surface).or_default().push((name, title));
        }
    }

    let path = out.join("index.html");
    std::fs::write(&path, index_page(&index))?;
    println!("{}", path.display());
    Ok(())
}

/// A plain contents page: every generated file, grouped by surface.
///
/// No styling beyond the stylesheet the pages themselves carry — this is a
/// table of contents, and a designed one would be one more thing to keep in
/// step with the design it is meant to be reviewing.
fn index_page(index: &BTreeMap<&'static str, Vec<(String, String)>>) -> String {
    let mut body = String::new();
    for (surface, pages) in index {
        body.push_str(&format!("<h2>{surface}</h2>\n<ul>\n"));
        for (name, title) in pages {
            body.push_str(&format!("<li><a href=\"{name}\">{title}</a></li>\n"));
        }
        body.push_str("</ul>\n");
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n<title>dat0 — visual scenes</title>\n\
         </head>\n<body>\n<h1>dat0 visual scenes</h1>\n\
         <p>{} scenes x {} themes. Compare against \
         <code>docs/internal/design/redesign-landing-v4.dc.html</code>.</p>\n{body}</body>\n</html>\n",
        SCENES.len(),
        BUILTIN_IDS.len(),
    )
}

/// Rewrite `url("/dat0/fonts/X.ttf")` to a base64 data URI.
///
/// The page has to be openable from anywhere with no server and no `assets/`
/// directory — the same guarantee the shipped binary makes through the `dat0://`
/// protocol, reproduced here by the only mechanism a plain file has. A page that
/// silently fell back to the system font would be a visual check that could not
/// see the one thing most likely to break.
fn inline_fonts(css: &str, assets: &Path) -> anyhow::Result<String> {
    use base64::Engine as _;

    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("url(\"/dat0/") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 5..]; // past `url("`
        let end = tail.find('"').expect("a closing quote on the url");
        let rel = &tail[..end];
        let file: PathBuf = assets.join(rel.trim_start_matches("/dat0/"));
        let bytes =
            std::fs::read(&file).map_err(|e| anyhow::anyhow!("read {}: {e}", file.display()))?;
        let mime = if rel.ends_with(".ttf") {
            "font/ttf"
        } else {
            "image/svg+xml"
        };
        out.push_str("url(\"data:");
        out.push_str(mime);
        out.push_str(";base64,");
        out.push_str(&base64::engine::general_purpose::STANDARD.encode(&bytes));
        out.push('"');
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}
