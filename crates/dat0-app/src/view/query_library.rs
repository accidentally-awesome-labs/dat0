//! Query history list + saved-query picker overlays (P5b). Lightweight list
//! views over the session's persisted stores; each row click emits a load.
//!
//! These are FREE render functions (no GPUI entity of their own). The history
//! list is mounted as an overlay INSIDE [`SqlConsole::render`](crate::view::sql_console::SqlConsole),
//! which owns a `&mut Window` — exactly what a row's `load-into-new-tab` needs.
//! So `on_pick` is handed `(sql, &mut Window, &mut App)`: the raw-`div`
//! `on_click` forwards its `window`/`cx` straight through, and the caller in
//! `SqlConsole::render` reaches the console entity via `Entity::update` to run
//! [`load_into_new_tab`](crate::view::sql_console::SqlConsole::load_into_new_tab)
//! — the path that needs a live `Window`.

use gpui::prelude::*;
use gpui::{App, ParentElement, SharedString, Styled, Window, div};

use crate::session::queries::HistoryEntry;

/// Render a history list (newest first). `on_pick` is called with the chosen
/// SQL plus the live `Window`/`App` from the click, so the caller can load it
/// into a new tab (which needs a `&mut Window`).
pub fn render_history_list(
    entries: &[HistoryEntry],
    on_pick: impl Fn(String, &mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .children(entries.iter().rev().enumerate().map(|(i, e)| {
            let sql = e.sql.clone();
            let on_pick = on_pick.clone();
            let preview: SharedString = first_line(&e.sql, 80).into();
            let meta: SharedString =
                format!("{} · {} ms", if e.ok { "ok" } else { "err" }, e.elapsed_ms).into();
            div()
                .id(("hist-row", i))
                .flex()
                .flex_row()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .cursor_pointer()
                .child(preview)
                .child(meta)
                .on_click(move |_ev, window, cx| on_pick(sql.clone(), window, cx))
        }))
}

/// First line of `sql`, truncated to `max` chars with an ellipsis.
pub fn first_line(sql: &str, max: usize) -> String {
    let line = sql.lines().next().unwrap_or("").trim();
    if line.chars().count() > max {
        let s: String = line.chars().take(max).collect();
        format!("{s}…")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_line_truncates_and_takes_first_line() {
        assert_eq!(first_line("select 1\nfrom t", 80), "select 1");
        assert_eq!(
            first_line(&"x".repeat(100), 10),
            format!("{}…", "x".repeat(10))
        );
    }
}
