//! Charts (P9a): the axis-binding helpers, the plot query, the chart
//! toolbar, and save / open of a named chart.
//!
//! The five free axis helpers carry their own unit tests at the bottom of
//! this file — they are pure functions over a `ChartSpec` and are the only
//! part of the chart surface testable without a window.

use super::*;

/// Read the spec field bound to `role`.
fn axis_field(
    spec: &crate::charts::spec::ChartSpec,
    role: crate::charts::spec::AxisRole,
) -> Option<&str> {
    use crate::charts::spec::{AxisRole, ChartType};
    match role {
        AxisRole::X => spec.x.as_deref(),
        AxisRole::Y => spec.y.as_deref(),
        AxisRole::Group => spec.group.as_deref(),
        AxisRole::Color => spec.color.as_deref(),
        // BoxPlot value → y; Heatmap value → color (per query.rs contract).
        AxisRole::Value => match spec.chart_type {
            ChartType::Heatmap => spec.color.as_deref(),
            _ => spec.y.as_deref(),
        },
    }
}

/// Write `val` into the spec field bound to `role`.
fn set_axis_field(
    spec: &mut crate::charts::spec::ChartSpec,
    role: crate::charts::spec::AxisRole,
    val: Option<String>,
) {
    use crate::charts::spec::{AxisRole, ChartType};
    match role {
        AxisRole::X => spec.x = val,
        AxisRole::Y => spec.y = val,
        AxisRole::Group => spec.group = val,
        AxisRole::Color => spec.color = val,
        AxisRole::Value => match spec.chart_type {
            ChartType::Heatmap => spec.color = val,
            _ => spec.y = val,
        },
    }
}

/// i18n key for an axis role's short label.
fn axis_role_key(role: crate::charts::spec::AxisRole) -> &'static str {
    use crate::charts::spec::AxisRole;
    match role {
        AxisRole::X => "chart.axis.x",
        AxisRole::Y => "chart.axis.y",
        AxisRole::Group => "chart.axis.group",
        AxisRole::Color => "chart.axis.color",
        AxisRole::Value => "chart.axis.value",
    }
}

/// Whether a role must always carry a column (X + the value axes) vs may be
/// cleared (Group / Color are optional dims that default to COUNT/none).
fn axis_required(role: crate::charts::spec::AxisRole) -> bool {
    use crate::charts::spec::AxisRole;
    matches!(role, AxisRole::X | AxisRole::Y | AxisRole::Value)
}

/// Advance an axis pick through `opts`. `required` axes cycle only over the
/// options (wrapping); optional axes additionally pass through `None` so the
/// user can clear a Group/Color dim. Picks not in `opts` (stale) reset to the
/// first option (or `None` for optional).
fn cycle_axis(current: Option<&str>, opts: &[String], required: bool) -> Option<String> {
    if opts.is_empty() {
        return None;
    }
    // Build the cycle order: [opt0, opt1, …] for required; [None, opt0, …] for
    // optional (None is index "before" the first option).
    let pos = current.and_then(|c| opts.iter().position(|o| o == c));
    match (required, pos) {
        // Required: just wrap over the options.
        (true, Some(i)) => Some(opts[(i + 1) % opts.len()].clone()),
        (true, None) => Some(opts[0].clone()),
        // Optional: order is None → opt0 → … → optN → None → …
        (false, None) => Some(opts[0].clone()),
        (false, Some(i)) if i + 1 < opts.len() => Some(opts[i + 1].clone()),
        (false, Some(_)) => None,
    }
}

impl WorkspaceShell {
    /// Show/hide the right-dock Charts panel. On open, bind the panel to the
    /// active grid's base table (off-thread `describe_table`) and kick off the
    /// first plot query. No-op (toggle still flips) when no file is registered.
    ///
    /// Uses the proven off-thread pattern from `load_inspector_profile`:
    /// `tokio::spawn` the engine call, hop the UI write back via the registry
    /// dispatcher. `base_table()` is QUOTED + may be schema-qualified
    /// (`"main"."orders"`); `describe_table` wants the BARE name, while the
    /// chart `source` must be a single quoted identifier — so we reduce to the
    /// bare name then re-quote it with `quote_ident`.
    pub(crate) fn toggle_chart_panel(&mut self, cx: &mut gpui::Context<Self>) {
        self.chart_panel_visible = !self.chart_panel_visible;
        // v11 is the first schema to persist chart-panel visibility at all —
        // v10's `ui` carried only the catalog and inspector bools.
        self.persist_dock_layout(cx);
        if self.chart_panel_visible {
            if let Some(base) = self.base_table() {
                let bare = bare_table_name(&base);
                let engine = self.engine();
                let ws_weak = cx.entity().downgrade();
                tokio::spawn(async move {
                    use dat0_engine::QueryEngine as _;
                    let cols = engine
                        .describe_table(&bare, None)
                        .await
                        .map(|cs| {
                            cs.into_iter()
                                .map(|c| (c.name, c.data_type))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    // Single quoted identifier; an `a.b` qualified name would be
                    // quoted whole (`"a.b"`) — accepted v1 limitation.
                    let quoted = dat0_engine::quote_ident(&bare);
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                            let Some(ws) = ws_weak.upgrade() else {
                                return;
                            };
                            ws.update(app_cx, |ws, cx| {
                                ws.chart_panel.bind(quoted, cols);
                                ws.run_plot_query(cx);
                            });
                        });
                    } else {
                        tracing::warn!(
                            "toggle_chart_panel: no MainThreadDispatcher installed; chart bind dropped"
                        );
                    }
                });
            }
        }
        cx.notify();
    }

    /// Build the plot SQL for the current spec, run it off-thread, render the
    /// result to a BGRA `RenderImage`, and stash it on the shell. Bumps
    /// `chart_load_id` so only the latest query's image survives (supersede
    /// guard for fast type/axis changes). On a missing-axis spec error the
    /// panel shows the error text in place of a chart and clears the image.
    pub(crate) fn run_plot_query(&mut self, cx: &mut gpui::Context<Self>) {
        let spec = self.chart_panel.spec.clone();
        let engine = self.engine();
        let sql = match crate::charts::query::build_plot_sql(&spec) {
            Ok(s) => s,
            Err(e) => {
                self.chart_panel.error = Some(e);
                self.chart_image = None;
                cx.notify();
                return;
            }
        };
        self.chart_load_id = self.chart_load_id.wrapping_add(1);
        let load_id = self.chart_load_id;
        // Logical chart size (px) × the bitmap supersample factor.
        let (lw, lh) = (520u32, 360u32);
        let scale = 2.0_f32;
        let (pw, ph) = ((lw as f32 * scale) as u32, (lh as f32 * scale) as u32);
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let qr = engine.execute(&sql).await;
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        // Supersede: a newer query already kicked off → drop this.
                        if ws.chart_load_id != load_id {
                            return;
                        }
                        match qr {
                            Ok(qr) => {
                                let pt = crate::charts::data::PlotTable::from_query_result(&qr);
                                let (bgra, w, h) = crate::charts::render::render_bgra(
                                    &ws.chart_panel.spec,
                                    &pt,
                                    (pw, ph),
                                );
                                match image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, bgra)
                                {
                                    Some(buf) => {
                                        let ri =
                                            gpui::RenderImage::new(smallvec::SmallVec::from_elem(
                                                image::Frame::new(buf),
                                                1,
                                            ));
                                        ws.chart_panel.error = None;
                                        ws.chart_panel.data = Some(pt);
                                        ws.chart_image = Some(std::sync::Arc::new(ri));
                                    }
                                    None => {
                                        ws.chart_panel.error =
                                            Some("chart image buffer build failed".into());
                                        ws.chart_image = None;
                                    }
                                }
                            }
                            Err(e) => {
                                ws.chart_panel.error = Some(e.to_string());
                                ws.chart_image = None;
                            }
                        }
                        cx.notify();
                    });
                });
            } else {
                tracing::warn!("run_plot_query: no MainThreadDispatcher installed; chart dropped");
            }
        });
        cx.notify();
    }

    /// Render the Charts dock toolbar (P9a T7): a chart-TYPE cycle button, one
    /// cycle button per *visible* axis (per `visible_axes(type)`), and PNG / SVG
    /// export buttons.
    ///
    /// Toolbar approach: **Button-cycle** (not gpui-component `Select`). Each
    /// click advances the value and immediately re-runs the plot query, so the
    /// data flow is identical to a Select-backed picker — type/axis change →
    /// mutate `spec` → `run_plot_query` → re-render. A button cycle is
    /// borrow-checker-trivial (no `Entity<SelectState>` to thread through the
    /// shell) and re-renders reliably, which the escalation note prefers over a
    /// half-working Select.
    pub(super) fn render_chart_toolbar(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        use crate::charts::panel::{column_options, visible_axes};
        use crate::charts::spec::ChartType;
        use gpui_component::button::Button;
        // `.disabled(..)` on `Button` comes from the `Disableable` trait.
        use gpui_component::Disableable;

        let cur_type = self.chart_panel.spec.chart_type;

        // ── Chart-type cycle button ────────────────────────────────────────
        let type_btn = Button::new("chart-type")
            .label(format!(
                "{}: {}",
                dat0_i18n::t("chart.panel.title"),
                dat0_i18n::t(cur_type.label_key())
            ))
            .on_click(cx.listener(|ws, _ev, _window, cx| {
                let cur = ws.chart_panel.spec.chart_type;
                let i = ChartType::ALL.iter().position(|t| *t == cur).unwrap_or(0);
                let next = ChartType::ALL[(i + 1) % ChartType::ALL.len()];
                ws.chart_panel.spec.chart_type = next;
                // A new type may expose axes the old picks don't satisfy; leave
                // the picks as-is (build_plot_sql errors → panel shows a "needs a
                // <role> column" hint until the user picks one).
                ws.run_plot_query(cx);
            }));

        // ── Per-visible-axis cycle buttons ─────────────────────────────────
        let mut row = h_flex()
            .gap_sp(Sp::S8)
            .flex_wrap()
            .p_sp(Sp::S8)
            .child(type_btn);
        for role in visible_axes(cur_type) {
            let current = axis_field(&self.chart_panel.spec, role).map(str::to_string);
            let label_role = dat0_i18n::t(axis_role_key(role));
            let label_val = current.clone().unwrap_or_else(|| "—".to_string());
            let id = format!("chart-axis-{}", axis_role_key(role));
            let role_copy = role;
            let btn = Button::new(gpui::SharedString::from(id))
                .label(format!("{label_role}: {label_val}"))
                .on_click(cx.listener(move |ws, _ev, _window, cx| {
                    let opts = column_options(role_copy, &ws.chart_panel.columns);
                    let next = cycle_axis(
                        axis_field(&ws.chart_panel.spec, role_copy),
                        &opts,
                        // Required axes (X always, plus Y/Value) never cycle to
                        // None; optional axes (Group/Color) include a None step.
                        axis_required(role_copy),
                    );
                    set_axis_field(&mut ws.chart_panel.spec, role_copy, next);
                    ws.run_plot_query(cx);
                }));
            row = row.child(btn);
        }

        // ── Save button (P9a-2) ────────────────────────────────────────────
        // Disabled until a chart is renderable (a source is bound AND at least
        // one axis is picked), so an empty chart can never be saved. Mirrors the
        // export guard's spirit but is enforced at the button (disabled) rather
        // than as a silent no-op, so the affordance reads correctly.
        let can_save = self.chart_panel.source.is_some()
            && (self.chart_panel.spec.x.is_some() || self.chart_panel.spec.y.is_some());
        let save_btn = Button::new("chart-save")
            .label(dat0_i18n::t("chart.save"))
            .disabled(!can_save)
            .on_click(cx.listener(|ws, _ev, window, cx| {
                ws.open_chart_save_prompt(window, cx);
            }));

        // B6: PNG/SVG export moved to `ChartsPanel::toolbar_buttons` — the
        // dock's 30px title bar. Save stays here: its `disabled` state reads
        // correctly at body size, and it opens a name prompt rather than a file
        // dialog, so it is not an "export" affordance.
        row.child(save_btn)
    }

    /// Open the shared name-prompt overlay to save the currently-bound chart
    /// under a user name (P9a-2). Seeds the prompt with the generated default
    /// ([`default_chart_name`](crate::session::charts::default_chart_name)), then
    /// routes a confirm to [`save_named_chart`](Self::save_named_chart) via the
    /// [`SaveChart`](NamePromptIntent::SaveChart) intent. No-op when no source is
    /// bound (the toolbar Save button is also disabled in that state).
    pub(crate) fn open_chart_save_prompt(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.chart_panel.source.is_none() {
            return;
        }
        let prefill = crate::session::charts::default_chart_name(&self.chart_panel.spec);
        self.open_name_prompt_with(
            dat0_i18n::t("chart.save.prompt"),
            prefill,
            NamePromptIntent::SaveChart,
            window,
            cx,
        );
    }

    /// Open the native save panel and export the current chart to PNG (`png =
    /// true`) or SVG. No-op when there's no rendered data yet — the live
    /// `chart_panel.spec` + `data` carry everything `export_*` needs (P9a T7).
    pub(crate) fn export_chart(&mut self, png: bool, cx: &mut gpui::Context<Self>) {
        let Some(data) = self.chart_panel.data.clone() else {
            return;
        };
        let spec = self.chart_panel.spec.clone();
        let ext = if png { "png" } else { "svg" };
        let suggested = format!("chart.{ext}");
        let path_rx = cx.prompt_for_new_path(std::path::Path::new(""), Some(&suggested));
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, _async_cx| {
            let dest = match path_rx.await {
                Ok(Ok(Some(dest))) => dest,
                _ => return,
            };
            // Export the SAME logical size the dock renders at (the bitmap
            // backend supersamples internally; here we write at logical px).
            let size = (1040u32, 720u32);
            let result: Result<(), String> = if png {
                crate::charts::export::export_png(&spec, &data, size, &dest)
                    .map_err(|e| e.to_string())
            } else {
                crate::charts::export::export_svg(&spec, &data, size, &dest)
                    .map_err(|e| e.to_string())
            };
            match result {
                Ok(()) => crate::error_ux::push(crate::error_ux::Banner::info(format!(
                    "{} → {}",
                    dat0_i18n::t("chart.save"),
                    dest.display()
                ))),
                Err(e) => crate::error_ux::push(crate::error_ux::Banner::warning(e)),
            }
        })
        .detach();
    }

    /// Persist the currently-bound chart spec as a named saved chart (P9a-2).
    /// Upserts by name (case-insensitive). No-op on empty name / no chart bound.
    /// Mirrors [`save_named_query`](Self::save_named_query) — reaches the session
    /// via `self.session.lock()`, upserts into the persisted list, then pushes an
    /// info banner and refreshes the catalog so the new chart appears in lineage.
    /// Called from [`on_name_prompt_event`](Self::on_name_prompt_event) on a
    /// [`SaveChart`](NamePromptIntent::SaveChart) confirm.
    pub(crate) fn save_named_chart(&mut self, name: String, cx: &mut Context<Self>) {
        if name.trim().is_empty() {
            return;
        }
        // `chart_panel` is a plain field on the shell (not an `Entity`), so the
        // live spec is read directly — there is no chart to save unless a source
        // is bound.
        if self.chart_panel.source.is_none() {
            return;
        }
        let spec = self.chart_panel.spec.clone();
        let c = crate::session::charts::SavedChart {
            id: uuid::Uuid::now_v7(),
            name: name.trim().to_string(),
            spec,
            saved_at: now_unix_millis(),
        };
        let mut sess = self.session.lock();
        let mut list = sess.charts().to_vec();
        crate::session::charts::upsert_chart(&mut list, c);
        let _ = sess.set_charts(list);
        drop(sess);
        crate::error_ux::push(crate::error_ux::Banner::info(dat0_i18n::t(
            "chart.save.done.title",
        )));
        self.refresh_catalog(cx); // so the new chart appears in lineage
        self.maybe_prompt_save_workspace();
    }

    /// Reopen a saved chart by name (P9a-2): look it up in the session, bind the
    /// chart panel to its stored spec, and render. Mirrors the "Visualize" open
    /// path (`toggle_chart_panel`) but seeds the panel from a persisted
    /// [`ChartSpec`] instead of building a fresh one from the active grid.
    /// Invoked from the Inspector lineage chain when a `NodeKind::Chart` row is
    /// clicked. No-op (silently) when the named chart is gone from the session.
    pub(crate) fn open_saved_chart(
        &mut self,
        name: String,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let spec = {
            let sess = self.session.lock();
            sess.charts()
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.spec.clone())
        };
        let Some(spec) = spec else { return };
        self.show_chart_with_spec(spec, window, cx);
    }

    /// Show the Charts dock seeded from a persisted [`ChartSpec`] (P9a-2). Unlike
    /// [`toggle_chart_panel`](Self::toggle_chart_panel) — which binds a *fresh*
    /// chart from the active grid and so resets all axis picks — this preserves
    /// the saved spec verbatim (chart type + axis picks + title) and only fetches
    /// the source's columns off-thread to repopulate the toolbar's axis-cycle
    /// options. The render is then driven by the SAME `run_plot_query` path the
    /// Visualize flow uses, so the data flow is identical.
    ///
    /// `spec.source` is a single quoted identifier (saved from a live spec);
    /// `describe_table` needs the bare name, so we reduce it via
    /// [`bare_table_name`].
    pub(crate) fn show_chart_with_spec(
        &mut self,
        spec: crate::charts::spec::ChartSpec,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // Seed the panel from the saved spec, preserving axis picks. Columns are
        // filled in once `describe_table` returns (below); the plot renders then.
        self.chart_panel_visible = true;
        self.chart_panel.source = Some(spec.source.clone());
        self.chart_panel.spec = spec.clone();
        self.chart_panel.columns = Vec::new();
        self.chart_panel.data = None;
        self.chart_panel.error = None;

        let bare = bare_table_name(&spec.source);
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let cols = engine
                .describe_table(&bare, None)
                .await
                .map(|cs| {
                    cs.into_iter()
                        .map(|c| (c.name, c.data_type))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        ws.chart_panel.columns = cols;
                        ws.run_plot_query(cx);
                    });
                });
            } else {
                tracing::warn!(
                    "show_chart_with_spec: no MainThreadDispatcher installed; chart bind dropped"
                );
            }
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::spec::{AxisRole, ChartSpec, ChartType};

    fn spec(t: ChartType) -> ChartSpec {
        ChartSpec {
            chart_type: t,
            source: "\"t\"".into(),
            x: None,
            y: None,
            group: None,
            color: None,
            title: String::new(),
        }
    }

    #[test]
    fn required_axis_cycles_over_options_only() {
        let opts = vec!["a".to_string(), "b".to_string()];
        // None → first; a → b; b → wrap to a. Required never returns None.
        assert_eq!(cycle_axis(None, &opts, true), Some("a".into()));
        assert_eq!(cycle_axis(Some("a"), &opts, true), Some("b".into()));
        assert_eq!(cycle_axis(Some("b"), &opts, true), Some("a".into()));
        // Stale pick (not in opts) resets to the first option.
        assert_eq!(cycle_axis(Some("zzz"), &opts, true), Some("a".into()));
        // No options → None even when required (nothing to pick).
        assert_eq!(cycle_axis(None, &[], true), None);
    }

    #[test]
    fn optional_axis_passes_through_none() {
        let opts = vec!["a".to_string(), "b".to_string()];
        // None → a → b → None → a (None is a real step for optional dims).
        assert_eq!(cycle_axis(None, &opts, false), Some("a".into()));
        assert_eq!(cycle_axis(Some("a"), &opts, false), Some("b".into()));
        assert_eq!(cycle_axis(Some("b"), &opts, false), None);
    }

    #[test]
    fn value_axis_maps_to_the_field_each_type_reads() {
        // BoxPlot reads its value from spec.y; Heatmap from spec.color
        // (matches charts/query.rs build_plot_sql).
        let mut bx = spec(ChartType::BoxPlot);
        set_axis_field(&mut bx, AxisRole::Value, Some("amt".into()));
        assert_eq!(bx.y.as_deref(), Some("amt"));
        assert_eq!(bx.color, None);
        assert_eq!(axis_field(&bx, AxisRole::Value), Some("amt"));

        let mut hm = spec(ChartType::Heatmap);
        set_axis_field(&mut hm, AxisRole::Value, Some("cnt".into()));
        assert_eq!(hm.color.as_deref(), Some("cnt"));
        assert_eq!(hm.y, None);
        assert_eq!(axis_field(&hm, AxisRole::Value), Some("cnt"));
    }

    #[test]
    fn required_axes_classification() {
        assert!(axis_required(AxisRole::X));
        assert!(axis_required(AxisRole::Y));
        assert!(axis_required(AxisRole::Value));
        assert!(!axis_required(AxisRole::Group));
        assert!(!axis_required(AxisRole::Color));
    }
}
