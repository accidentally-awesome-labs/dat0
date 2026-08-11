//! Tier 2 of the visual suite: every scene, measured in a real window.
//!
//! ```text
//! cargo run -p dat0-ui --features visual --example visual_probe
//! ```
//!
//! The SSR tier pins markup. Markup is not a layout: the bug that motivated
//! this suite rendered three children into a two-column CSS grid, so the
//! catalog sat *on top of* the data grid instead of beside it — while fifteen
//! numeric assertions across two suites passed. Only a browser can see that.
//!
//! So this walks `dat0_ui::visual::SCENES` in one wry window, in all three
//! themes, and reads back `getBoundingClientRect` / `getComputedStyle` for
//! every scene. One launch, not 177: the scene is a signal write.
//!
//! # Six generic invariants
//!
//! Generic on purpose. Hand-written expectations for 59 scenes rot, and these
//! catch the class of bug the suite exists for:
//!
//! | | Invariant | Catches |
//! |---|---|---|
//! | V1 | the scene root has width, height and at least one `[data-a11y-id]` | the scene rendered nothing |
//! | V2 | the scene root does not overflow on an axis its catalogue entry did not declare | content escaping a *scrollable* container |
//! | V3 | every shown `[data-a11y-id]` with children has width > 0 and height > 0 | a flex or grid child collapsing to nothing |
//! | V4 | every shown, non-`fixed` `[data-a11y-id]` box lies inside the scene root's, ±1px | content escaping a *clipped* container |
//! | V5 | no element with direct text has `color` equal to its nearest opaque ancestor `background-color` | white-on-white from a token that stopped resolving |
//! | V6 | every family the scene's visible text resolves to is a Geist face | the protocol failed to serve a face and everything silently fell back |
//!
//! **V2 and V4 are a pair, and V4 is the one that bites here.** Reverting the
//! shell's open-sidebar grid template to `{sidebar_px}px minmax(0, 1fr)` — the
//! original bug — is caught on four of the five `shell/*` scenes, by V4 and V3,
//! not by V2: `html, body, #main` are `overflow: hidden`, so a work area that
//! wraps to a second row is *clipped* rather than scrollable and the scene
//! root's `scrollHeight` never moves. What moves is where the content is: the
//! hero lands at `y = 1078..1363`, well outside a 900px frame, and `hero-drop`
//! collapses to `320x0`. V2 covers the surfaces that legitimately scroll; V4
//! covers the ones that clip. Measured, both ways, against that exact revert.
//!
//! V1's element count is also the driver's own self-check: if the scene failed
//! to remount, a `modal/*` scene renders an empty frame and V1 says so, rather
//! than five invariants quietly passing over nothing.
//!
//! V5 is exact colour equality, not a contrast ratio: it has no threshold to
//! argue about and no false positives. Setting `.d0-sidebar`'s background to
//! `var(--d0-fg)` fires it on the sidebar's row names in `high-contrast`, where
//! the ink is the same pure black; in light and dark the row text is a muted
//! shade of the same token, so it survives — a contrast gate would catch those
//! too, and would also fire on legitimate low-contrast chrome.
//!
//! `examples/shell_probe.rs` is deliberately untouched. It carries the shell's
//! *specific* committed geometry (44 / 38 / 30 / 238 / 226 and the side-by-side
//! check); this one carries generic invariants across everything. Duplicating
//! the shell's numbers here would give two tests that fail together.
//!
//! Not a CI job: hosted runners have no display, the same constraint
//! `docs/deferrals.md` records as D-032 for the perf scroll scenarios.
//!
//! Exits 0 when every scene passes, 1 otherwise.

use anyhow::Context as _;
use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

use dat0_core::theme::tokens::BUILTIN_IDS;
use dat0_ui::theme::{Theme, ThemeStyle};
use dat0_ui::visual::{Fixtures, Handle, SCENES, Scene, SceneHost};

/// The window's client area, which MUST equal the scene box.
///
/// `Shell` sizes itself to the viewport — `100vh` and `position: fixed` bars —
/// so a scene box smaller than the window would report the shell overflowing
/// its frame by exactly the difference, and every fixed bar as outside it. That
/// is a harness artefact, not a layout bug. Making the two identical removes
/// the artefact; the probe asserts the match rather than assuming it, because
/// a window manager is free to hand back a smaller window than it was asked
/// for.
const WINDOW: (f64, f64) = (SCENE_W, SCENE_H);

/// The scene box, from `visual::SceneHost`'s inline style.
const SCENE_W: f64 = 1440.0;
const SCENE_H: f64 = 900.0;

/// A macrotask yield between the unmount and the mount, so the vdom flushes
/// both. Returns a value so the Rust side can await the round trip.
const SETTLE: &str = r#"
await new Promise((r) => setTimeout(r, 4));
dioxus.send(1);
"#;

/// Wait, once, for both Geist faces.
///
/// Per-scene it was a liability: `setTimeout` is throttled hard in an occluded
/// window, so 59 × 3 bounded polls turn into 59 × 3 chances to time out on a
/// question whose answer stopped changing after the first scene. Fonts load
/// once per webview, so this asks once, generously, after the first scene has
/// mounted — a face is only requested when something uses it, so asking before
/// the first mount would wait forever.
///
/// NOT `await document.fonts.ready`: in WKWebView that promise does not settle
/// here even though every face loads (verified — the protocol answers
/// `/dat0/fonts/*.ttf` with 200 and the right length).
const READY: &str = r#"
const settle = () => new Promise((r) => setTimeout(r, 8));
let ok = 0;
for (let i = 0; i < 2000; i++) {
  if (
    document.querySelector("[data-scene]") &&
    document.fonts.check("12.5px 'Geist Mono'") &&
    document.fonts.check("15px 'Geist'")
  ) {
    ok = 1;
    break;
  }
  await settle();
}
dioxus.send(ok);
"#;

/// Measure one scene. `__CONFIG__` is replaced with the scene's JSON.
///
/// `setTimeout` rather than `requestAnimationFrame`, deliberately: rAF only
/// fires while the webview is compositing, so an occluded or never-fronted
/// window — the normal state under a runner — gets one callback and then
/// nothing. Timers keep running (throttled). Layout and style resolution are
/// synchronous and need no frame. See `examples/shell_probe.rs`.
const PROBE: &str = r#"
const cfg = __CONFIG__;
const settle = () => new Promise((r) => setTimeout(r, 4));

async function waitFor(pred, label, tries = 400) {
  for (let i = 0; i < tries; i++) {
    if (pred()) return;
    await settle();
  }
  throw new Error("timed out waiting for " + label);
}

const guard = setTimeout(() => {
  dioxus.send({ error: cfg.id + " did not finish within 30s" });
}, 30000);

// An element is "shown" unless the DOM says otherwise. Zero opacity is a
// deliberate hide — a collapsed `Pane` keeps its body mounted at `opacity: 0`
// so it survives the collapse with its scroll position — and `aria-hidden`
// says the same thing to the accessibility tree, which is the tree these ids
// belong to.
function hidden(el) {
  for (let n = el; n && n !== document.documentElement; n = n.parentElement) {
    const cs = getComputedStyle(n);
    if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) === 0) {
      return true;
    }
    if (n.getAttribute && n.getAttribute("aria-hidden") === "true") return true;
  }
  return false;
}

// V4 does not apply to a positioned overlay: `position: fixed` takes an element
// out of its ancestor's box by definition, which is how the shell's bars and
// every modal scrim are built.
function overlaid(el, root) {
  for (let n = el; n && n !== root; n = n.parentElement) {
    if (getComputedStyle(n).position === "fixed") return true;
  }
  return false;
}

const TRANSPARENT = /^rgba\(\s*\d+\s*,\s*\d+\s*,\s*\d+\s*,\s*0\s*\)$/;

function nearestOpaqueBackground(el) {
  for (let n = el; n; n = n.parentElement) {
    const bg = getComputedStyle(n).backgroundColor;
    if (bg && bg !== "transparent" && !TRANSPARENT.test(bg)) return bg;
  }
  return getComputedStyle(document.documentElement).backgroundColor;
}

function hasDirectText(el) {
  for (const n of el.childNodes) {
    if (n.nodeType === Node.TEXT_NODE && n.textContent.trim() !== "") return true;
  }
  return false;
}

/// The first family in a computed `font-family` list, unquoted.
function first(list) {
  return list.split(",")[0].replace(/["']/g, "").trim();
}

try {
  await waitFor(() => document.querySelector(`[data-scene="${cfg.id}"]`), "scene " + cfg.id);
  await settle();

  const root = document.querySelector(`[data-scene="${cfg.id}"]`);
  const rr = root.getBoundingClientRect();
  const all = Array.from(root.querySelectorAll("[data-a11y-id]"));

  const collapsed = [];
  const escaped = [];
  let shown = 0;

  // V7. A grid whose track list is computed in Rust must fit its children in
  // the tracks it declared.
  //
  // This is the bug that motivated the whole suite, and it has shipped three
  // times: a dock lays out N children into an N-1 track template, so the last
  // one is auto-placed into an IMPLICIT track and the splitter inherits the
  // track meant for the panel. The result stays inside the frame and every
  // element keeps a sane size, so V1-V6 are all satisfied — it is simply in
  // the wrong cell. Only the track count sees it.
  //
  // Scoped to INLINE templates on purpose: those are the shell's docks, whose
  // track lists are interpolated from `sidebar_px` / `right_px` / `bottom_px`.
  // A stylesheet grid that wraps by design (the gallery's `auto-fill` swatch
  // grid) is not making a claim about its child count and is not checked.
  const misgridded = [];
  const trackCount = (v) => (!v || v === "none" ? 0 : v.trim().split(/\s+/).length);
  // Declared tracks, tokenised at paren depth 0 so `minmax(0, 1fr)` counts once.
  const declaredCount = (v) => {
    let depth = 0, n = 0, inTok = false;
    for (const ch of v) {
      if (ch === "(") depth++;
      else if (ch === ")") depth--;
      if (depth === 0 && /\s/.test(ch)) { inTok = false; continue; }
      if (!inTok) { n++; inTok = true; }
    }
    return n;
  };

  for (const el of root.querySelectorAll("*")) {
    const cols = el.style.gridTemplateColumns;
    const rows = el.style.gridTemplateRows;
    if (!cols && !rows) continue;
    if (getComputedStyle(el).display.indexOf("grid") === -1) continue;
    const cs = getComputedStyle(el);
    const label = (el.getAttribute("data-a11y-id") || el.className || el.tagName).toString();

    for (const [axis, inline, computed] of [
      ["columns", cols, cs.gridTemplateColumns],
      ["rows", rows, cs.gridTemplateRows],
    ]) {
      const got = trackCount(computed);
      if (inline) {
        const want = declaredCount(inline);
        if (got > want) {
          misgridded.push(`${label} ${axis}: declared ${want} (${inline.trim()}), laid out ${got}`);
        }
      } else if (got > 1) {
        // The untemplated axis. More than one track there means children
        // wrapped off the axis this grid actually controls.
        misgridded.push(`${label} ${axis}: untemplated, but laid out ${got} tracks (${computed})`);
      }
    }
  }

  for (const el of all) {
    if (hidden(el)) continue;
    shown += 1;
    // A childless element has no box of its own to lose. The grid header's
    // sort and funnel handles are empty spans that only reserve horizontal
    // space for `zone_from_x`, the shell's `grid-loading` is an empty div, and
    // a CodeMirror mount is empty until the bundle attaches — none of them is
    // a collapse, and V3 exists to catch a collapse.
    if (el.childNodes.length === 0) continue;
    const id = el.getAttribute("data-a11y-id");
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) {
      collapsed.push(`${id} ${Math.round(r.width)}x${Math.round(r.height)}`);
      continue;
    }
    if (overlaid(el, root)) continue;
    if (
      r.left < rr.left - 1 ||
      r.top < rr.top - 1 ||
      r.right > rr.right + 1 ||
      r.bottom > rr.bottom + 1
    ) {
      escaped.push(
        `${id} [${Math.round(r.left)},${Math.round(r.top)},${Math.round(r.right)},${Math.round(r.bottom)}]`,
      );
    }
  }

  // V5. SVG text paints with `fill`, not `color`, so an `<svg>` subtree is a
  // different question and not this one.
  const invisible = [];
  // V6. Not `document.fonts.check`: WebKit releases a face that nothing on the
  // page is currently using, so the two banner scenes with no monospace text
  // report Geist Mono as unloaded while every other scene reports it loaded —
  // a per-window fact answered once by READY, not a per-scene one. What IS per
  // scene is which family each piece of text actually resolved to.
  const families = new Set();
  families.add(first(getComputedStyle(root).fontFamily));

  for (const el of root.querySelectorAll("*")) {
    if (el.closest("svg")) continue;
    if (!hasDirectText(el)) continue;
    if (hidden(el)) continue;
    const cs = getComputedStyle(el);
    families.add(first(cs.fontFamily));
    const bg = nearestOpaqueBackground(el);
    if (cs.color === bg) {
      const label = el.getAttribute("data-a11y-id") || el.className || el.tagName;
      invisible.push(`${label} ${cs.color}`);
    }
  }

  clearTimeout(guard);
  dioxus.send({
    w: Math.round(rr.width),
    h: Math.round(rr.height),
    elements: shown,
    overflow_x: root.scrollWidth - root.clientWidth,
    overflow_y: root.scrollHeight - root.clientHeight,
    collapsed,
    escaped,
    invisible,
    misgridded,
    families: Array.from(families),
    viewport_w: window.innerWidth,
    viewport_h: window.innerHeight,
  });
} catch (e) {
  clearTimeout(guard);
  dioxus.send({ error: String(e && e.message ? e.message : e) });
}
"#;

#[derive(serde::Deserialize, Debug, Default)]
struct Report {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    w: i64,
    #[serde(default)]
    h: i64,
    #[serde(default)]
    elements: usize,
    #[serde(default)]
    overflow_x: i64,
    #[serde(default)]
    overflow_y: i64,
    #[serde(default)]
    collapsed: Vec<String>,
    #[serde(default)]
    escaped: Vec<String>,
    #[serde(default)]
    invisible: Vec<String>,
    #[serde(default)]
    misgridded: Vec<String>,
    /// Every distinct first `font-family` the scene's visible text resolved to.
    #[serde(default)]
    families: Vec<String>,
    #[serde(default)]
    viewport_w: i64,
    #[serde(default)]
    viewport_h: i64,
}

fn main() -> anyhow::Result<()> {
    // The same shape `perf::run_windowed` uses: a runtime entered around the
    // event loop, so every `spawn_blocking` in the tree resolves against it.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("visual probe tokio runtime")?;
    let _guard = rt.enter();

    let fx = Handle::new(rt.block_on(Fixtures::build())?);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("dat0 visual probe")
                    .with_inner_size(LogicalSize::new(WINDOW.0, WINDOW.1)),
            ),
        )
        .with_context(fx)
        .launch(Probe);

    Ok(())
}

#[component]
fn Probe() -> Element {
    dioxus::desktop::use_asset_handler("dat0", dat0_ui::protocol::serve);
    let fx = use_context::<Handle>();

    // The root's own theme, so `ThemeStyle`'s `:root` block tracks the scene.
    // `SceneHost` provides a second one for its own subtree; both are set from
    // the same id, which is what makes V5 and V6 per-theme questions.
    let mut theme = Theme::provide(None);
    let mut slot = use_signal(|| Option::<(usize, &'static str)>::None);

    use_effect(move || {
        let fx = fx.clone();
        spawn(async move {
            let mut fails: Vec<String> = Vec::new();
            println!("--- dat0 visual probe ---");
            println!(
                "  {:<30} {:<14} {:>5} {:>4}  V1 V2 V3 V4 V5 V6 V7",
                "scene", "theme", "els", "ovf"
            );

            // Mount something, then wait once for the faces. Everything below
            // measures type, and a run against the fallback metrics would be a
            // pass that meant nothing.
            slot.set(Some((0, BUILTIN_IDS[0])));
            if document::eval(READY).recv::<u32>().await.unwrap_or(0) != 1 {
                println!(
                    "FAIL: the Geist faces never loaded. The protocol serves them from \
                     the binary (src/protocol.rs); nothing below would be measuring dat0's \
                     type."
                );
                std::process::exit(1);
            }

            for (ix, scene) in SCENES.iter().enumerate() {
                for id in BUILTIN_IDS {
                    // Unmount, flush, mount. Two writes in one task without a
                    // yield between them would coalesce into one render and the
                    // scene would never remount — which is how a `use_hook`
                    // seed silently stops running from the second scene on.
                    slot.set(None);
                    if !settle().await {
                        eprintln!("probe: the vdom never flushed before {}/{id}", scene.id);
                        std::process::exit(2);
                    }
                    theme.set(id);
                    slot.set(Some((ix, id)));

                    let script = PROBE.replace("__CONFIG__", &config(scene));
                    let Some(report) = measure(&script).await else {
                        eprintln!(
                            "probe: the eval channel never delivered for {}/{id}",
                            scene.id
                        );
                        std::process::exit(2);
                    };
                    check(scene, id, &report, &mut fails);
                }
            }

            let _ = fx; // keeps the fixtures alive for the whole walk
            if fails.is_empty() {
                println!(
                    "PASS ({} scenes x {} themes)",
                    SCENES.len(),
                    BUILTIN_IDS.len()
                );
                std::process::exit(0);
            }
            println!("FAIL ({} violations)", fails.len());
            for f in &fails {
                println!("  - {f}");
            }
            std::process::exit(1);
        });
    });

    let current = slot();

    rsx! {
        ThemeStyle {}
        // A keyed single-element list. The key is the scene AND the theme, so a
        // swap is a remove-and-create rather than a diff, and every scene gets
        // a fresh `Workspace`, a fresh seed and fresh per-scene signals.
        for (ix, id) in current.iter().copied() {
            SceneHost {
                key: "{SCENES[ix].id}/{id}",
                fx: use_context::<Handle>(),
                id: SCENES[ix].id.to_string(),
                theme: id.to_string(),
            }
        }
    }
}

/// How many times a wedged-looking eval channel is re-issued before the probe
/// gives up. Four is empirical: a retry has never been needed twice in a row.
const EVAL_TRIES: usize = 4;

/// Yield to the vdom, so a pending render flushes before the next signal write.
///
/// `false` only when the channel never delivered at all, which means the
/// webview is wedged rather than busy.
async fn settle() -> bool {
    for _ in 0..EVAL_TRIES {
        if document::eval(SETTLE).recv::<u32>().await.is_ok() {
            return true;
        }
    }
    false
}

/// Run the measuring eval, retrying a channel that reports itself already
/// finished.
///
/// Observed a few times per run and never twice in a row: a scene whose own
/// components create evals (`SqlConsole` boots CodeMirror over one, `ModalHost`
/// inerts the background with another) can leave the desktop renderer handing
/// back an already-completed handle for an eval issued in the same tick.
/// Re-issuing after a settle delivers, so the probe retries rather than dying
/// on a renderer hiccup — but it retries a bounded number of times, because a
/// genuinely wedged webview must fail rather than spin.
async fn measure(script: &str) -> Option<Report> {
    for attempt in 0..EVAL_TRIES {
        match document::eval(script).recv::<Report>().await {
            Ok(r) => return Some(r),
            Err(e) => {
                eprintln!("probe: eval attempt {attempt} failed ({e}); retrying");
                settle().await;
            }
        }
    }
    None
}

fn config(scene: &Scene) -> String {
    format!(
        r#"{{"id":{:?},"allowX":{},"allowY":{}}}"#,
        scene.id,
        scene.scroll.allows_x(),
        scene.scroll.allows_y()
    )
}

/// Score one scene against the six invariants, appending every violation.
fn check(scene: &Scene, theme: &str, r: &Report, fails: &mut Vec<String>) {
    let where_ = format!("{}/{theme}", scene.id);

    if let Some(e) = &r.error {
        println!("  {:<30} {:<14} PROBE ERROR", scene.id, theme);
        fails.push(format!("{where_}: {e}"));
        return;
    }

    // A precondition, not an invariant: every geometric check below assumes
    // the viewport IS the scene box, because `Shell` sizes itself to the
    // viewport. A window manager that shrank the window would otherwise be
    // reported as 59 layout bugs.
    if r.viewport_w != SCENE_W as i64 || r.viewport_h != SCENE_H as i64 {
        println!("  {:<30} {:<14} WINDOW TOO SMALL", scene.id, theme);
        fails.push(format!(
            "{where_}: the window is {}x{}, but every measurement assumes the \
             {}x{} scene box. Free up screen space, or lower SCENE_W/SCENE_H \
             and re-run — do not read the numbers below.",
            r.viewport_w, r.viewport_h, SCENE_W as i64, SCENE_H as i64
        ));
        return;
    }

    let v1 = r.w > 0 && r.h > 0 && r.elements > 0;
    let v2 = (scene.scroll.allows_x() || r.overflow_x <= 1)
        && (scene.scroll.allows_y() || r.overflow_y <= 1);
    let v3 = r.collapsed.is_empty();
    let v4 = r.escaped.is_empty();
    let v5 = r.invisible.is_empty();
    let v7 = r.misgridded.is_empty();
    let v6 = !r.families.is_empty() && r.families.iter().all(|f| f == "Geist" || f == "Geist Mono");

    let mark = |ok: bool| if ok { "ok" } else { "XX" };
    println!(
        "  {:<30} {:<14} {:>5} {:>4}  {}  {}  {}  {}  {}  {}  {}",
        scene.id,
        theme,
        r.elements,
        r.overflow_x.max(r.overflow_y),
        mark(v1),
        mark(v2),
        mark(v3),
        mark(v4),
        mark(v5),
        mark(v6),
        mark(v7),
    );

    if !v1 {
        fails.push(format!(
            "{where_} V1: {}x{} with {} shown elements",
            r.w, r.h, r.elements
        ));
    }
    if !v2 {
        fails.push(format!(
            "{where_} V2: overflows by {}px x / {}px y (scroll declared {:?})",
            r.overflow_x, r.overflow_y, scene.scroll
        ));
    }
    if !v3 {
        fails.push(format!("{where_} V3 collapsed: {}", r.collapsed.join(", ")));
    }
    if !v4 {
        fails.push(format!(
            "{where_} V4 outside the frame: {}",
            r.escaped.join(", ")
        ));
    }
    if !v5 {
        fails.push(format!(
            "{where_} V5 text on its own ground: {}",
            r.invisible.join(", ")
        ));
    }
    if !v7 {
        fails.push(format!(
            "{where_} V7 children in implicit grid tracks: {}",
            r.misgridded.join("; ")
        ));
    }
    if !v6 {
        fails.push(format!(
            "{where_} V6 non-Geist type: {}",
            r.families.join(", ")
        ));
    }
}
