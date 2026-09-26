//! The status bar: the window's engine, the rows in view, the selection, the
//! last query, what dat0 has sent off this machine, and the palette's chord
//! (PD-023, step 5.8c).
//!
//! It said `engine duckdb · native` whatever the engine was doing, and its
//! memory, rows and frame-rate segments were fields nothing wrote, so they
//! never showed. Every segment now reads what it names: the session, the
//! resident set (`chrome::use_memory`), the grid's rows in view
//! (`Grid::on_rows`), the grid's selection and the console's run. The frame
//! rate is gone from the bar: the perf HUD measures it, and a bar that says
//! `60 fps` while nothing paints would be the constant this replaced.

use dioxus::prelude::*;

use dat0_core::session::slot::SessionSlot;

use crate::state::Workspace;

/// What the console's last query is doing, for the bar's query chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryChip {
    /// No query has run in this window.
    #[default]
    Idle,
    Running,
    /// The last query took this long.
    Done {
        ms: u64,
    },
}

#[derive(Clone, PartialEq, Props)]
pub struct StatusBarProps {
    /// How many cells the grid has selected.
    pub selected: ReadSignal<usize>,
    /// The console's last query.
    pub query: ReadSignal<QueryChip>,
}

#[component]
pub fn StatusBar(props: StatusBarProps) -> Element {
    let ws = Workspace::use_current();
    let s = ws.status.read().clone();
    let (dot, state) = engine(&ws.session.read(), *ws.live.read());
    let engine = format!(
        "{} · {}",
        dat0_i18n::t("status.engine"),
        dat0_i18n::t(state)
    );
    // Rows and a selection belong to the grid on screen: with no tab open
    // there is none, whatever the last one left.
    let on_screen = ws.active.read().is_some();
    let rows = s.rows.filter(|_| on_screen);
    let rows_word = dat0_i18n::t("status.rows");
    let selected = if on_screen { (props.selected)() } else { 0 };
    let query = (props.query)();
    let query_word = dat0_i18n::t("status.query");
    let chord = crate::chrome::palette_chord();
    let commands = dat0_i18n::t("status.commands");
    rsx! {
        div { class: "d0-statusbar", "data-a11y-id": "statusbar", role: "status",
            span { class: "{dot}" }
            span { "data-a11y-id": "status-engine", "{engine}" }
            if s.mem_mb > 0 {
                span { "data-a11y-id": "status-mem",
                    "mem " span { class: "d0-num", "{s.mem_mb}" } " MB"
                }
            }
            if let Some((first, last, total)) = rows {
                span { "data-a11y-id": "status-rows",
                    if total == 0 {
                        span { class: "d0-num", "0" }
                        " {rows_word}"
                    } else {
                        "{rows_word} "
                        span { class: "d0-num", "{thousands(first)}–{thousands(last)}" }
                        " / "
                        span { class: "d0-num", "{thousands(total)}" }
                    }
                }
            }
            if selected > 0 {
                span { "data-a11y-id": "status-selection",
                    span { class: "d0-num", "{thousands(selected as u64)}" }
                    " {selection_noun(selected)}"
                }
            }
            match query {
                QueryChip::Idle => rsx! {},
                QueryChip::Running => rsx! {
                    span { "data-a11y-id": "status-query", {dat0_i18n::t("status.query_running")} }
                },
                QueryChip::Done { ms } => rsx! {
                    span { "data-a11y-id": "status-query",
                        "{query_word} "
                        span { class: "d0-num", "{thousands(ms)}" }
                        " ms"
                    }
                },
            }
            span { class: "d0-spacer" }
            span { class: "is-ok", style: "color: var(--d0-ok)", "{crate::chrome::egress_line(&s)}" }
            span { class: "d0-key",
                span { "data-chord": "palette", "{chord}" }
                " {commands}"
            }
        }
    }
}

/// The engine's dot and state: starting while the session opens, native or
/// motherduck once it has, failed when it could not open.
fn engine(slot: &SessionSlot, motherduck: bool) -> (&'static str, &'static str) {
    match slot {
        SessionSlot::Booting => ("d0-dot is-busy", "status.engine.starting"),
        SessionSlot::Ready(_) if motherduck => ("d0-dot is-live", "status.engine.motherduck"),
        SessionSlot::Ready(_) => ("d0-dot is-live", "status.engine.native"),
        SessionSlot::Failed(_) => ("d0-dot is-error", "status.engine.failed"),
    }
}

/// `cell selected` or `cells selected`, after the count.
fn selection_noun(n: usize) -> String {
    dat0_i18n::t(if n == 1 {
        "status.cell_selected"
    } else {
        "status.cells_selected"
    })
}

/// Group a count with commas, the way the design writes row counts.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_counts_are_grouped() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_048_576), "1,048,576");
        assert_eq!(thousands(1_200_000_000), "1,200,000,000");
    }

    #[test]
    fn one_cell_and_many() {
        assert_eq!(selection_noun(1), "cell selected");
        assert_eq!(selection_noun(1_500), "cells selected");
    }

    #[test]
    fn the_engine_says_what_the_session_is_doing() {
        assert_eq!(
            engine(&SessionSlot::Booting, false).1,
            "status.engine.starting"
        );
        assert_eq!(
            engine(&SessionSlot::Failed("disk full".into()), false).1,
            "status.engine.failed"
        );
    }
}
