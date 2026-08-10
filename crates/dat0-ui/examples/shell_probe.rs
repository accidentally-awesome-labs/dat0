//! Windowed design-conformance probe.
//!
//! The headless harness can assert structure, text and attributes, but not
//! *geometry*: heights and widths come from CSS classes, and there is no layout
//! engine in a `WriteMutations` mirror. A 44px titlebar that renders at 22px
//! because a rule did not load would pass every headless test.
//!
//! So this launches the real shell in a real window, reads back
//! `getBoundingClientRect` and `getComputedStyle` for the load-bearing
//! surfaces, and asserts the Design system's numbers in Rust. It is the
//! automated half of the design gate; the manual half is the side-by-side
//! against `docs/internal/design/redesign-landing-v4.dc.html`.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example shell_probe
//! ```
//!
//! Exits 0 when every measurement matches, 1 otherwise.

use dioxus::prelude::*;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_ui::launch::Boot;

/// Read back what the browser actually laid out.
///
/// Measures rather than asserts: every number crosses back into Rust so a
/// failure names the value it found, not just the rule it broke.
const PROBE: &str = r#"
// A macrotask yield, deliberately NOT `requestAnimationFrame`.
//
// rAF only fires while the webview is compositing. A window that is occluded —
// or simply never brought to the front, which is the normal state under a test
// runner — gets exactly one callback and then nothing, so an rAF-driven poll
// hangs forever. Timers keep running (throttled), so they are what a probe can
// rely on. Layout and style resolution are synchronous and do not need a frame.
const settle = () => new Promise((r) => setTimeout(r, 4));

async function waitFor(pred, label, tries = 500) {
  for (let i = 0; i < tries; i++) {
    if (pred()) return;
    await settle();
  }
  throw new Error("timed out waiting for " + label);
}

// A hard backstop: if anything below wedges, report that rather than hanging
// a CI job for its whole timeout. Timers are throttled in an unfocused
// window, so this fires late rather than exactly on time — which is fine for
// a backstop.
const guard = setTimeout(() => {
  dioxus.send({ error: "probe did not finish within 30s" });
}, 30000);

try {
  const q = (id) => document.querySelector(`[data-a11y-id="${id}"]`);
  await waitFor(() => q("statusbar"), "the shell to mount");
  // Fonts are `font-display: block`; a measurement taken before they land
  // reports the fallback's metrics.
  // NOT `await document.fonts.ready`: in WKWebView that promise does not
  // settle here even though every face loads (verified — the protocol answers
  // /dat0/fonts/*.ttf with 200 and the right length). Poll the one question
  // that matters instead, bounded, so a genuinely missing face fails the probe
  // rather than hanging it.
  await waitFor(
    () => document.fonts.check("12.5px 'Geist Mono'"),
    "Geist Mono to load",
  );
  await settle();

  const box = (id) => {
    const el = q(id);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return {
      x: Math.round(r.x),
      y: Math.round(r.y),
      w: Math.round(r.width),
      h: Math.round(r.height),
    };
  };
  const css = (id, prop) => {
    const el = q(id);
    return el ? getComputedStyle(el).getPropertyValue(prop).trim() : null;
  };
  const rootVar = (name) =>
    getComputedStyle(document.documentElement).getPropertyValue(name).trim();

  clearTimeout(guard);
  dioxus.send({
    titlebar: box("titlebar"),
    tabstrip: box("tabstrip"),
    statusbar: box("statusbar"),
    sidebar: box("sidebar"),
    slot: box("command-slot"),
    pane_stack: box("pane-stack"),
    launcher: box("command-launcher"),
    // Proves app.css was fetched over the protocol: a class-derived value.
    mono_size: css("statusbar", "font-size"),
    mono_family: css("statusbar", "font-family"),
    // Proves the runtime token block is present.
    canvas: rootVar("--d0-canvas"),
    accent: rootVar("--d0-accent"),
    scheme: getComputedStyle(document.documentElement).colorScheme,
    // The three sections are always present, even when empty.
    sections: ["section-files", "section-connections", "section-packages"].filter((s) => q(s)).length,
    fonts_loaded: document.fonts.check("12.5px 'Geist Mono'"),
  });
} catch (e) {
  clearTimeout(guard);
  dioxus.send({ error: String(e && e.message ? e.message : e) });
}
"#;

#[derive(serde::Deserialize, Debug, Default)]
struct Box2 {
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
    w: i64,
    h: i64,
}

#[derive(serde::Deserialize, Debug, Default)]
struct Report {
    #[serde(default)]
    error: Option<String>,
    titlebar: Option<Box2>,
    tabstrip: Option<Box2>,
    statusbar: Option<Box2>,
    sidebar: Option<Box2>,
    slot: Option<Box2>,
    pane_stack: Option<Box2>,
    launcher: Option<Box2>,
    #[serde(default)]
    mono_size: String,
    #[serde(default)]
    mono_family: String,
    #[serde(default)]
    canvas: String,
    #[serde(default)]
    accent: String,
    #[serde(default)]
    scheme: String,
    #[serde(default)]
    sections: usize,
    #[serde(default)]
    fonts_loaded: bool,
}

fn main() {
    let (events, rx) = AppEvents::channel();
    let registry = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&registry).expect("built-ins register");

    let boot = Boot {
        events,
        rx: std::sync::Arc::new(parking_lot::Mutex::new(Some(rx))),
        registry,
        cli_paths: std::sync::Arc::new(parking_lot::Mutex::new(Vec::new())),
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(dat0_ui::launch::config())
        .with_context(boot)
        .launch(Probe);
}

#[component]
fn Probe() -> Element {
    // `use_effect`, not `use_future`: an effect runs *after* the component has
    // mounted, which is when the desktop document provider exists. An eval
    // created during the first render pass is queued against nothing and never
    // runs — silently.
    use_effect(move || {
        spawn(async move {
            eprintln!("probe: waiting for the shell…");
            let mut eval = document::eval(PROBE);
            let r: Report = match eval.recv().await {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("probe channel failed: {e}");
                    std::process::exit(2);
                }
            };
            report(r);
        });
    });

    rsx! { dat0_ui::components::App {} }
}

fn report(r: Report) {
    println!("--- dat0 shell probe ---");
    if let Some(e) = r.error {
        println!("probe error: {e}");
        std::process::exit(1);
    }

    let tokens = dat0_core::theme::builtin_or_default(dat0_core::theme::DEFAULT_ID);
    let mut fails: Vec<String> = Vec::new();
    let mut check = |name: &str, got: String, want: String| {
        let ok = got == want;
        println!(
            "  {:<22} {:<34} {}",
            name,
            got,
            if ok { "ok" } else { "MISMATCH" }
        );
        if !ok {
            fails.push(format!("{name}: got {got}, want {want}"));
        }
    };

    let h = |b: &Option<Box2>| b.as_ref().map(|b| b.h).unwrap_or(-1);
    let w = |b: &Option<Box2>| b.as_ref().map(|b| b.w).unwrap_or(-1);

    check("titlebar height", h(&r.titlebar).to_string(), "44".into());
    check("tabstrip height", h(&r.tabstrip).to_string(), "38".into());
    check("statusbar height", h(&r.statusbar).to_string(), "30".into());
    check("sidebar width", w(&r.sidebar).to_string(), "238".into());
    // The gutter must be exactly the sidebar's width — that alignment is the
    // whole point of the launcher sitting there — and the button is inset 6px
    // on each side within it.
    check("launcher slot width", w(&r.slot).to_string(), "238".into());
    check("launcher width", w(&r.launcher).to_string(), "226".into());

    // The sidebar and the grid sit SIDE BY SIDE, and the sidebar runs the full
    // height of the body.
    //
    // Every check above passed while this was false: the shell rendered three
    // children into a two-column grid, so the splitter took column two and the
    // work area wrapped to row two — the catalog on top, the grid underneath,
    // each individually the right size. Sizes are not a layout.
    let (side, pane) = (r.sidebar.as_ref(), r.pane_stack.as_ref());
    let beside = match (side, pane) {
        (Some(s), Some(p)) => p.x >= s.x + s.w,
        _ => false,
    };
    check(
        "grid sits beside the sidebar",
        beside.to_string(),
        "true".into(),
    );
    let tall = match (side, pane) {
        // Within a splitter's width of each other: they are two tracks of one
        // row, not two rows.
        (Some(s), Some(p)) => (s.h - p.h).abs() <= 4,
        _ => false,
    };
    check(
        "sidebar is as tall as the body",
        tall.to_string(),
        "true".into(),
    );
    check("mono size", r.mono_size.clone(), "12.5px".into());
    check("canvas token", r.canvas.clone(), tokens.canvas.clone());
    check("accent token", r.accent.clone(), tokens.accent.clone());
    check("color-scheme", r.scheme.clone(), "light".into());
    check("sidebar sections", r.sections.to_string(), "3".into());
    check(
        "Geist Mono loaded",
        r.fonts_loaded.to_string(),
        "true".into(),
    );
    check(
        "mono family",
        r.mono_family.contains("Geist Mono").to_string(),
        "true".into(),
    );

    if fails.is_empty() {
        println!("PASS");
        std::process::exit(0);
    }
    println!("FAIL ({} mismatches)", fails.len());
    for f in &fails {
        println!("  - {f}");
    }
    std::process::exit(1);
}
