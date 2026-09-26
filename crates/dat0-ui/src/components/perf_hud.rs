//! The performance HUD (MX1): frame rate, frame-time percentiles, resident
//! memory and the grid's pages, pinned bottom-right over the shell. The
//! palette's `perf.hud.toggle` shows and hides it.
//!
//! A frame is a paint the webview makes after the page changed: a mutation
//! observer asks for the next animation frame, and that frame's time is a
//! paint instant, fed to [`FrameClock`]. The HUD's own text is left out, or it
//! would keep the window painting. An idle window paints nothing, so the rate
//! reads an em-dash once [`IDLE_AFTER`](dat0_core::perf::IDLE_AFTER) passes,
//! never `0`. A window that is not compositing stops animation frames, and
//! says so the same way.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dioxus::prelude::*;
use serde::Deserialize;

use dat0_core::perf::FrameClock;

use crate::components::grid::views::Shown;
use crate::components::import_progress::human_bytes;

/// How often the webview sends its paints, and the readout refreshes: each
/// batch refreshes it, empty or not, so the HUD needs no timer of its own.
const REFRESH: Duration = Duration::from_millis(250);

/// What stands for a number there is nothing to measure for.
const DASH: &str = "\u{2014}";

/// A batch of paints from the webview, on its clock.
#[derive(Deserialize)]
struct Paints {
    /// `performance.now()` when the batch was sent.
    now: f64,
    /// `performance.now()` of each paint since the last batch, oldest first.
    paints: Vec<f64>,
}

/// The grid's resident pages out of its cap, when a grid is showing.
fn pages(source: &Shown) -> Option<(usize, usize)> {
    let shown = source.peek();
    let (_, grid) = shown.as_ref()?.as_ref()?;
    grid.as_ref().ok().map(|g| g.cache_occupancy())
}

/// The HUD's four lines.
#[derive(Clone, PartialEq, Debug)]
pub struct Readout {
    pub fps: String,
    pub frame_ms: String,
    pub rss: String,
    pub pages: String,
}

impl Readout {
    /// The readout for `clock`, this process's resident set, and the grid's
    /// resident pages out of its cap, when a grid is showing.
    pub fn of(clock: &FrameClock, rss: Option<u64>, pages: Option<(usize, usize)>) -> Self {
        let fps = match clock.fps() {
            Some(fps) => format!("{fps:.0} fps"),
            None => format!("{DASH} fps"),
        };
        let at = |p: f32| {
            clock
                .percentile_ms(p)
                .map_or_else(|| DASH.to_string(), |ms| format!("{ms:.1}"))
        };
        Self {
            fps,
            frame_ms: format!("p50 {} / p95 {} / p99 {} ms", at(0.5), at(0.95), at(0.99)),
            rss: format!("rss {}", rss.map_or_else(|| DASH.to_string(), human_bytes)),
            pages: match pages {
                Some((resident, cap)) => format!("pages {resident} / {cap}"),
                None => format!("pages {DASH}"),
            },
        }
    }
}

/// Maps the webview's clock onto this process's, anchored at the first batch:
/// paints keep their spacing and their order.
#[derive(Default)]
struct Paced {
    anchor: Option<(f64, Instant)>,
    last: Option<Instant>,
}

impl Paced {
    fn feed(&mut self, clock: &mut FrameClock, batch: &Paints) {
        let (js, at) = *self.anchor.get_or_insert((batch.now, Instant::now()));
        for &t in &batch.paints {
            let offset = Duration::from_secs_f64(((t - js) / 1000.0).abs());
            let instant = if t >= js { at + offset } else { at - offset };
            // Never backwards: a gap below zero is no frame.
            let instant = self.last.map_or(instant, |last| instant.max(last));
            self.last = Some(instant);
            clock.tick_at(instant);
        }
    }
}

#[component]
pub fn PerfHud(source: Shown) -> Element {
    let clock = use_hook(|| Rc::new(RefCell::new(FrameClock::new())));
    let read = move |clock: &FrameClock| {
        Readout::of(clock, dat0_core::platform::rss_bytes(), pages(&source))
    };
    let mut readout = use_signal({
        let clock = clock.clone();
        move || read(&clock.borrow())
    });

    // `use_effect`, not `use_future`: the desktop document provider exists
    // only after mount, and an eval made during the first render never runs.
    use_effect({
        let clock = clock.clone();
        move || {
            let clock = clock.clone();
            spawn(async move {
                let script = PAINTS.replace("REFRESH", &REFRESH.as_millis().to_string());
                let mut eval = document::eval(&script);
                let mut paced = Paced::default();
                while let Ok(batch) = eval.recv::<Paints>().await {
                    paced.feed(&mut clock.borrow_mut(), &batch);
                    let now = read(&clock.borrow());
                    if *readout.peek() != now {
                        readout.set(now);
                    }
                }
            });
        }
    });
    use_drop(|| {
        let _ = document::eval(STOP);
    });

    let r = readout.read();
    rsx! {
        div { class: "d0-perf-hud", "data-a11y-id": "perf-hud", "aria-hidden": "true",
            div { "data-a11y-id": "perf-hud-fps", "{r.fps}" }
            div { "data-a11y-id": "perf-hud-frame-ms", "{r.frame_ms}" }
            div { "data-a11y-id": "perf-hud-rss", "{r.rss}" }
            div { "data-a11y-id": "perf-hud-pages", "{r.pages}" }
        }
    }
}

/// Records paints after the page changes and sends them every `REFRESH` ms,
/// none or not.
///
/// It returns once set up, and the channel keeps working (see
/// `sql_console::editor::BOOT`). [`STOP`] takes it down.
const PAINTS: &str = r#"
if (window.__dat0hud) window.__dat0hud.stop();
const hud = () => document.querySelector(".d0-perf-hud");
const paints = [];
let asked = false;
const observer = new MutationObserver((records) => {
  const h = hud();
  if (asked || records.every((r) => h && h.contains(r.target))) return;
  asked = true;
  requestAnimationFrame((t) => { asked = false; paints.push(t); });
});
observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
const timer = setInterval(() => {
  dioxus.send({ now: performance.now(), paints: paints.splice(0) });
}, REFRESH);
window.__dat0hud = { stop: () => { observer.disconnect(); clearInterval(timer); window.__dat0hud = null; } };
"#;

/// Takes [`PAINTS`] down when the HUD closes.
const STOP: &str = "if (window.__dat0hud) window.__dat0hud.stop();";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_measured_reads_as_dashes_never_zero() {
        let r = Readout::of(&FrameClock::new(), None, None);
        assert_eq!(r.fps, "\u{2014} fps");
        assert_eq!(r.frame_ms, "p50 \u{2014} / p95 \u{2014} / p99 \u{2014} ms");
        assert_eq!(r.rss, "rss \u{2014}");
        assert_eq!(r.pages, "pages \u{2014}");
    }

    #[test]
    fn paints_keep_their_spacing_and_read_as_a_rate() {
        let mut clock = FrameClock::new();
        let mut paced = Paced::default();
        // Sixty paints 16 ms apart, the last just now on the webview's clock.
        let paints: Vec<f64> = (0..60).map(|i| 1000.0 + f64::from(i) * 16.0).collect();
        let now = *paints.last().unwrap();
        paced.feed(&mut clock, &Paints { now, paints });

        let r = Readout::of(&clock, Some(3 * 1024 * 1024), Some((5, 64)));
        let fps: f32 = r.fps.trim_end_matches(" fps").parse().expect(&r.fps);
        assert!((62.0..=63.0).contains(&fps), "{}", r.fps);
        assert_eq!(r.frame_ms, "p50 16.0 / p95 16.0 / p99 16.0 ms");
        assert_eq!(r.rss, "rss 3.0 MB");
        assert_eq!(r.pages, "pages 5 / 64");
    }

    #[test]
    fn batches_keep_the_spacing_between_them() {
        let mut clock = FrameClock::new();
        let mut paced = Paced::default();
        let at = |from: u32| {
            (from..from + 30)
                .map(|i| f64::from(i) * 16.0)
                .collect::<Vec<_>>()
        };
        paced.feed(
            &mut clock,
            &Paints {
                now: 480.0,
                paints: at(0),
            },
        );
        paced.feed(
            &mut clock,
            &Paints {
                now: 960.0,
                paints: at(30),
            },
        );
        let r = Readout::of(&clock, None, None);
        assert_eq!(r.frame_ms, "p50 16.0 / p95 16.0 / p99 16.0 ms");
    }

    #[test]
    fn a_paint_out_of_order_never_ticks_backwards() {
        let mut clock = FrameClock::new();
        let mut paced = Paced::default();
        paced.feed(
            &mut clock,
            &Paints {
                now: 116.0,
                paints: vec![100.0, 116.0],
            },
        );
        let last = paced.last.unwrap();
        paced.feed(
            &mut clock,
            &Paints {
                now: 120.0,
                paints: vec![110.0],
            },
        );
        assert_eq!(paced.last.unwrap(), last);
    }
}
