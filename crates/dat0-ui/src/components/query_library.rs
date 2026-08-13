//! The query-history list.
//!
//! Ported from `view/query_library.rs` (the row markup and [`first_line`]) plus
//! the listbox half of the console's history overlay
//! (`view/sql_console.rs:1065-1180`), which owned the active index because a
//! free render function could not.
//!
//! Bringing the index in here is the whole difference: the list is now one
//! component with one keyboard contract, mountable both as the console's
//! history overlay and as the standalone library modal, instead of a render
//! function whose behaviour lived in its caller.
//!
//! Rows are **newest first**. History is stored oldest-first
//! (`queries::push_history` appends), so display order is the reverse, and the
//! active index counts in DISPLAY order — the GPUI overlay built a reversed
//! `picks` vector for exactly this reason.

use dioxus::prelude::*;

use dat0_core::session::queries::HistoryEntry;

/// First line of `sql`, truncated to `max` characters with an ellipsis.
///
/// Character-counted, not byte-counted: a truncation that splits a multi-byte
/// character panics on the slice.
pub fn first_line(sql: &str, max: usize) -> String {
    let line = sql.lines().next().unwrap_or("").trim();
    if line.chars().count() > max {
        let s: String = line.chars().take(max).collect();
        format!("{s}…")
    } else {
        line.to_string()
    }
}

/// Preview width, in characters. The GPUI list passed 80 at its single call
/// site; it is a constant here so the two mount points cannot disagree.
pub const PREVIEW_CHARS: usize = 80;

/// The `ok · 12 ms` trailer for one run.
pub fn outcome_meta(entry: &HistoryEntry) -> String {
    let outcome = if entry.ok {
        dat0_i18n::t("sql.history.ok")
    } else {
        dat0_i18n::t("sql.history.err")
    };
    format!("{outcome} · {} ms", entry.elapsed_ms)
}

#[derive(Clone, Props, PartialEq)]
pub struct QueryLibraryProps {
    /// History as stored: oldest first. Displayed reversed.
    pub entries: Vec<HistoryEntry>,
    /// Load this SQL into a new console tab.
    pub on_pick: EventHandler<String>,
}

#[component]
pub fn QueryLibrary(props: QueryLibraryProps) -> Element {
    let mut active = use_signal(|| 0usize);
    // Display order, newest first.
    let rows: Vec<HistoryEntry> = props.entries.iter().rev().cloned().collect();
    let len = rows.len();
    let active_ix = active().min(len.saturating_sub(1));

    let on_pick = props.on_pick;
    let picks: Vec<String> = rows.iter().map(|e| e.sql.clone()).collect();

    rsx! {
        div {
            class: "d0-history",
            "data-a11y-id": "sql-history-list",
            role: "listbox",
            "aria-label": dat0_i18n::t("sql.history"),
            "aria-activedescendant": if len > 0 { format!("hist-row-{active_ix}") } else { String::new() },
            tabindex: "0",
            onkeydown: move |e| {
                match e.key() {
                    // Arrows clamp; this is a list, not a radio group.
                    Key::ArrowDown => {
                        e.prevent_default();
                        active.set((active_ix + 1).min(len.saturating_sub(1)));
                    }
                    Key::ArrowUp => {
                        e.prevent_default();
                        active.set(active_ix.saturating_sub(1));
                    }
                    k if k == Key::Enter || k == Key::Character(" ".into()) => {
                        if let Some(sql) = picks.get(active_ix) {
                            e.prevent_default();
                            on_pick.call(sql.clone());
                        }
                    }
                    _ => {}
                }
            },

            if rows.is_empty() {
                div { class: "d0-row is-empty", "data-a11y-id": "hist-empty",
                    "{dat0_i18n::t(\"sql.history.empty\")}"
                }
            } else {
                for (i , e) in rows.iter().enumerate() {
                    div {
                        key: "{i}",
                        id: "hist-row-{i}",
                        class: if i == active_ix { "d0-row is-active" } else { "d0-row" },
                        "data-a11y-id": "hist-row-{i}",
                        role: "option",
                        "aria-selected": if i == active_ix { "true" } else { "false" },
                        "aria-label": first_line(&e.sql, PREVIEW_CHARS),
                        onclick: {
                            let sql = e.sql.clone();
                            move |_| {
                                active.set(i);
                                on_pick.call(sql.clone());
                            }
                        },
                        span { class: "d0-row-name d0-mono", "{first_line(&e.sql, PREVIEW_CHARS)}" }
                        span {
                            class: if e.ok { "d0-row-meta d0-mono" } else { "d0-row-meta d0-mono is-err" },
                            "{outcome_meta(e)}"
                        }
                    }
                }
            }
        }
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

    #[test]
    fn a_multibyte_preview_does_not_split_a_character() {
        // Byte-slicing "…" at 4 would panic; the GPUI original counted chars
        // for this reason and so does this one.
        assert_eq!(first_line("émoji ☕ here", 4), "émoj…");
    }
}
