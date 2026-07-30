//! Status bar: permanent, read-only chrome along the bottom of the workspace
//! shell (UI redesign slice B3). Shows `rows × cols`, the selection size, the
//! last SQL run's timing, and the connection state.
//!
//! Shape mirrors [`crate::view::pipeline_bar`]: a snapshot struct plus pure
//! string builders that need no window, and one free render fn. The bar owns no
//! state, mints no focus handles, and registers no click handlers — it is
//! chrome, not a control, so every keyboard-nav cycle count in the suite is
//! unchanged by construction.

use crate::connections::routing::Routing;
use crate::connections::{ConnectionManager, ConnectionStatus};

/// What the last SQL run is doing, as far as the status bar is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryStatus {
    /// No console, or no run has completed yet.
    #[default]
    Idle,
    Running,
    Done {
        ms: u64,
        routing: Option<Routing>,
    },
}

/// Everything the bar renders, snapshotted from the shell once per frame.
///
/// `rows`/`cols` are `None` until a data source is mounted; `selected_cells` is
/// `None` until the grid has a selection. `connection` arrives pre-rendered from
/// [`describe_connection`] because the shell owns the [`ConnectionManager`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusBarModel {
    pub rows: Option<u64>,
    pub cols: Option<usize>,
    pub selected_cells: Option<usize>,
    pub query: QueryStatus,
    pub connection: String,
}

/// Group a count into thousands with `,` separators. dat0 ships English only
/// and `dat0_i18n::t` has no interpolation, so this is deliberately not
/// locale-aware.
pub fn format_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.char_indices() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// One-line connection summary: the MotherDuck state, plus the number of
/// attached SQLite databases when there are any.
///
/// The `Error` variant's payload is deliberately dropped — it can carry a server
/// message, and this surface has no room for one. The Connections panel shows it.
pub fn describe_connection(conns: &ConnectionManager) -> String {
    let state = match conns.md_status() {
        ConnectionStatus::Connected => dat0_i18n::t("status.conn_md"),
        ConnectionStatus::Connecting => dat0_i18n::t("status.conn_connecting"),
        ConnectionStatus::Error(_) => dat0_i18n::t("status.conn_error"),
        ConnectionStatus::Disconnected => dat0_i18n::t("status.conn_local"),
    };
    let attached = conns.sqlite().len();
    if attached == 0 {
        state
    } else {
        format!("{state} · {attached} {}", dat0_i18n::t("status.attached"))
    }
}

impl StatusBarModel {
    /// The bar's segment strings, in render order. Pure, so the entire rendered
    /// text of the bar is assertable with no window.
    pub fn segments(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(4);
        if let (Some(rows), Some(cols)) = (self.rows, self.cols) {
            out.push(format!(
                "{} {} × {} {}",
                format_count(rows),
                dat0_i18n::t("status.rows"),
                cols,
                dat0_i18n::t("status.cols"),
            ));
        }
        if let Some(n) = self.selected_cells.filter(|n| *n > 0) {
            out.push(format!(
                "{} {}",
                format_count(n as u64),
                dat0_i18n::t("status.cells_selected"),
            ));
        }
        match self.query {
            QueryStatus::Idle => {}
            QueryStatus::Running => out.push(dat0_i18n::t("status.query_running")),
            QueryStatus::Done { ms, routing } => {
                // Same routing tail as the SQL console's own timing chip
                // (`sql.local` / `sql.md` / `sql.mixed`), so the two surfaces
                // can never word it differently.
                let key = routing.map(|r| r.i18n_key()).unwrap_or("sql.local");
                out.push(format!(
                    "{} {ms} ms · {}",
                    dat0_i18n::t("status.query"),
                    dat0_i18n::t(key),
                ));
            }
        }
        out.push(self.connection.clone());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::routing::Routing;
    use crate::connections::{ConnectionManager, ConnectionStatus};

    #[test]
    fn format_count_groups_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(1), "1");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn describe_connection_maps_every_status() {
        let mut m = ConnectionManager::default();
        assert_eq!(describe_connection(&m), "Local");
        m.set_md_status(ConnectionStatus::Connecting);
        assert_eq!(describe_connection(&m), "Connecting…");
        m.set_md_status(ConnectionStatus::Connected);
        assert_eq!(describe_connection(&m), "MotherDuck");
        m.set_md_status(ConnectionStatus::Error("nope".into()));
        assert_eq!(describe_connection(&m), "Connection error");
    }

    /// The error text itself must never reach the bar — it can carry a server
    /// message, and this surface is 12px muted chrome with no room for one.
    #[test]
    fn describe_connection_never_leaks_the_error_body() {
        let mut m = ConnectionManager::default();
        m.set_md_status(ConnectionStatus::Error("token rejected by md".into()));
        assert!(!describe_connection(&m).contains("token"));
    }

    #[test]
    fn describe_connection_appends_attachment_count() {
        let mut m = ConnectionManager::default();
        m.add_sqlite("shop".into(), "/tmp/shop.db".into());
        m.add_sqlite("logs".into(), "/tmp/logs.db".into());
        assert_eq!(describe_connection(&m), "Local · 2 attached");
    }

    fn model() -> StatusBarModel {
        StatusBarModel {
            rows: None,
            cols: None,
            selected_cells: None,
            query: QueryStatus::Idle,
            connection: "Local".to_string(),
        }
    }

    #[test]
    fn segments_of_an_empty_model_are_connection_only() {
        assert_eq!(model().segments(), vec!["Local".to_string()]);
    }

    #[test]
    fn segments_render_shape_only_when_both_dimensions_are_known() {
        let mut m = model();
        m.rows = Some(1_234);
        assert_eq!(m.segments(), vec!["Local".to_string()]);
        m.cols = Some(12);
        assert_eq!(
            m.segments(),
            vec!["1,234 rows × 12 cols".to_string(), "Local".to_string()]
        );
    }

    #[test]
    fn segments_hide_an_empty_selection() {
        let mut m = model();
        m.selected_cells = Some(0);
        assert_eq!(m.segments(), vec!["Local".to_string()]);
        m.selected_cells = Some(84);
        assert_eq!(
            m.segments(),
            vec!["84 cells selected".to_string(), "Local".to_string()]
        );
    }

    #[test]
    fn segments_render_query_state() {
        let mut m = model();
        m.query = QueryStatus::Running;
        assert_eq!(
            m.segments(),
            vec!["Query running…".to_string(), "Local".to_string()]
        );
        m.query = QueryStatus::Done {
            ms: 12,
            routing: Some(Routing::Md),
        };
        assert_eq!(
            m.segments(),
            vec!["Query 12 ms · md".to_string(), "Local".to_string()]
        );
        m.query = QueryStatus::Done {
            ms: 12,
            routing: None,
        };
        assert_eq!(
            m.segments(),
            vec!["Query 12 ms · local".to_string(), "Local".to_string()]
        );
    }

    #[test]
    fn segments_render_in_a_fixed_order() {
        let m = StatusBarModel {
            rows: Some(2),
            cols: Some(2),
            selected_cells: Some(4),
            query: QueryStatus::Done {
                ms: 7,
                routing: None,
            },
            connection: "Local".to_string(),
        };
        assert_eq!(
            m.segments(),
            vec![
                "2 rows × 2 cols".to_string(),
                "4 cells selected".to_string(),
                "Query 7 ms · local".to_string(),
                "Local".to_string(),
            ]
        );
    }
}
