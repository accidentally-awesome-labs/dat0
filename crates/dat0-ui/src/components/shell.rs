//! The window shell: titlebar, tab strip, sidebar, pane stack, status bar.
//!
//! Three fixed bars and a two-column body, at the design's exact geometry. The
//! bars are `position: fixed` and the body is padded past them, so the grid
//! scrolls under a titlebar that never moves and the layout never depends on
//! measuring anything.

use dioxus::prelude::*;

use dat0_core::sample_data::SampleKind;

use crate::a11y::{AccessRole, format_swatch};
use crate::components::ai::StreamView;
use crate::components::banner::BannerHost;
use crate::components::charts::{ChartLoad, ChartRequest, Charts};
use crate::components::command_palette::CommandPalette;
use crate::components::dock::{DragShield, Edge, SplitDrag, Splitter};
use crate::components::empty_state::EmptyState;
use crate::components::inspector::{Inspector, InspectorState};
use crate::components::modals::ModalHost;
use crate::components::pane::Pane;
use crate::components::pipeline_bar::PipelineBar;

/// Pixel size every chart export is rendered at. Fixed rather than taken from
/// the pane: an export is a document, and a file whose resolution depended on
/// how wide the user had dragged a pane would be irreproducible.
const CHART_EXPORT_SIZE: (u32, u32) = (1600, 900);
use crate::components::grid::Grid;
use crate::components::sidebar::{self, Sidebar};
use crate::components::sql_console::{ConsoleIntent, SqlConsole};
use crate::keys::Cascade;
use crate::state::{Modal, Status, Workspace};
use crate::theme::Theme;

/// The whole window.
#[component]
pub fn Shell() -> Element {
    let mut ws = Workspace::use_current();
    let theme = Theme::use_current();
    let drag = use_signal(|| Option::<SplitDrag>::None);
    // The window's own size, for the resize clamp. Updated by the root's
    // `onresize`; the initial value only has to be non-degenerate.
    let mut extent = use_signal(|| (1440.0_f64, 900.0_f64));

    // Cloned per closure: both are `Arc`-backed handles, so a clone is a
    // refcount bump, and Dioxus event closures each need their own.
    let events = super::events();
    let registry = super::registry();
    let (key_events, key_registry) = (events.clone(), registry.clone());
    let banner_events = events.clone();
    let demo_events = events.clone();
    let open_events = events.clone();

    // One key handler for the window. GPUI resolved chords through an ambient
    // context tree; here the precedence is explicit — see `keys::Cascade`.
    let cascade = Cascade {
        modal_open: ws.modal.read().is_some(),
        palette_open: *ws.palette.read(),
        sql_console_focused: false,
    };

    // Surfaces the shell owns state for. Each is a signal rather than a field
    // on `Workspace` because nothing outside this subtree reads them.
    let mut banners = use_signal(Vec::<dat0_core::error_ux::Banner>::new);
    let inspector = InspectorState::use_new();
    // `ChartSpec` has no `Default` — a chart with no source is not a chart —
    // so the shell holds the empty-source spec the pane renders as its empty
    // state until a table is bound.
    let mut chart_spec = use_signal(|| dat0_core::charts::spec::ChartSpec {
        chart_type: dat0_core::charts::spec::ChartType::Bar,
        source: String::new(),
        x: None,
        y: None,
        group: None,
        color: None,
        title: String::new(),
    });
    let chart_state = use_signal(ChartLoad::default);
    // Read once per window: the flag flips at most once per install, and
    // re-reading settings.toml on every render to learn that would be absurd.
    let first_run_done = use_signal(|| {
        dat0_core::platform::config_dir()
            .ok()
            .map(|d| dat0_core::settings::store::SettingsStore::with_path(d.join("settings.toml")))
            .and_then(|s| s.load_or_default().ok())
            .is_some_and(|s| s.first_run_done)
    });

    // The first-run tour auto-opens exactly once, on the very first run
    // (`empty_state::should_auto_tour`). `use_hook` runs on mount and never
    // again, which is this window's whole guard: GPUI needed a `tour_auto_shown`
    // bool because its empty-state *render* posted the open, so every frame was
    // another chance to stack a second dialog. Here the open happens once per
    // window mount and the slot could not hold two anyway.
    use_hook(move || {
        if crate::components::empty_state::should_auto_tour(first_run_done()) {
            let mut ws = ws;
            ws.modal.set(Some(Modal::Onboarding));
        }
    });

    // Pending banners are raised from anywhere — `error_ux::push` is a global,
    // because the code that fails is usually nowhere near a component. Drain
    // them into the window's list on every render pass.
    {
        let mut banners = banners;
        use_effect(move || {
            dat0_core::error_ux::banner::merge_pending(&mut banners.write());
        });
    }

    // The window's own size feeds the mount clamp: a dock size restored from a
    // bigger display must not push the centre off screen.
    let (window_w, window_h) = extent();
    let sidebar_px = ws.sidebar_px(window_w);
    let right_px = ws.right_px(window_w);
    let bottom_px = ws.bottom_px(window_h);

    // Attach parents the user has collapsed. The GPUI build kept this in the
    // session (`ui.catalog_collapsed`); it belongs to whoever persists it, and
    // until the catalog has an engine feed the sidebar's own set is the whole
    // truth.
    let mut collapsed = use_signal(std::collections::HashSet::<String>::new);

    // The grid's own state. Column widths and the selection live here rather
    // than inside `Grid` because they outlive a remount — switching tabs and
    // coming back must not reset a column the user widened.
    let selection = use_signal(|| dat0_core::grid::selection::SelectionModel::new(1, 1));
    let widths = use_signal(Vec::<f64>::new);

    // One `GridDataSource` per active tab, rebuilt when the tab changes.
    // `use_resource` because building one runs a DESCRIBE against DuckDB: it is
    // a query, not a field read, and it must not block the render that asks.
    //
    // Every read below is INSIDE the future, and that is the whole contract:
    // `use_resource` restarts when a signal read during its own execution
    // changes, and nothing else. Hoisting `ws.active_tab()` up here — read
    // during the shell's render, captured into the closure — subscribed the
    // resource to nothing at all, because on the one poll that mattered the
    // `?` short-circuited on an empty tab list before it ever touched
    // `session`. The tab arrived, the tab strip showed it, and the work area
    // stayed on `d0-grid-loading` forever. `tests/shell_grid_binding.rs`
    // mounts the real shell over a real session and fails if it comes back.
    let mut widths_seed = widths;
    let mut selection_seed = selection;
    let source = use_resource(move || async move {
        let table = ws.active_tab().map(|t| t.table)?;
        let engine = ws.session.read().ready().map(|s| s.lock().engine.clone())?;
        let built = dat0_core::grid::data_source::GridDataSource::new(engine, table)
            .await
            .map(std::sync::Arc::new)
            .map_err(|e| format!("{e:#}"));

        // Size the grid's two pieces of shell-owned state to the table that
        // just bound. `Grid` derives its visible column range from
        // `widths.len()`, so an empty vector paints a header with no columns
        // and no cells; `SelectionModel` clamps `move_active` against its own
        // dimensions, so a 1x1 model pins the keyboard cursor to A1 whatever
        // the table holds. Both were left at their mount-time placeholders.
        if let Ok(src) = &built {
            let cols = src.visible_column_names().len();
            widths_seed.set(vec![crate::components::grid::COL_W_DEFAULT; cols]);
            selection_seed.set(dat0_core::grid::selection::SelectionModel::new(
                usize::try_from(src.row_count).unwrap_or(usize::MAX).max(1),
                cols.max(1),
            ));
        }
        Some(built)
    });

    // The console's tabs and its last failure. The schema snapshot is shared
    // with the editor's completion provider.
    // The chart's plot data. `use_resource` for the same reason the grid's
    // source is one: building it runs a query. Holding the table (not just the
    // rendered SVG) is what makes PNG export possible — plotters rasterises
    // from the data, not from an SVG string.
    let chart_data = use_resource(move || {
        let spec = chart_spec();
        async move {
            if spec.source.is_empty() {
                return None;
            }
            let engine = ws.session.read().ready().map(|s| s.lock().engine.clone())?;
            let sql = dat0_core::charts::query::build_plot_sql(&spec).ok()?;
            let qr = dat0_engine::QueryEngine::execute(engine.as_ref(), &sql)
                .await
                .ok()?;
            Some(dat0_core::charts::data::PlotTable::from_query_result(&qr))
        }
    });

    // The AI panel's controller, built once so the modal can be opened from a
    // command without rebuilding the provider draft each time.
    let ai = crate::components::ai::AiController::use_new(crate::components::ai::AiDeps {
        store: std::sync::Arc::new(dat0_core::settings::store::SettingsStore::with_path(
            dat0_core::platform::config_dir()
                .unwrap_or_default()
                .join("settings.toml"),
        )),
        // A keychain that will not open is not a reason to refuse the window:
        // the panel degrades to "no key stored", which is the truth.
        keys: match dat0_core::ai::key_store::KeychainKeyStore::new() {
            Ok(k) => {
                std::sync::Arc::new(k) as std::sync::Arc<dyn dat0_core::ai::key_store::KeyStore>
            }
            Err(e) => {
                tracing::warn!("keychain unavailable: {e:#}");
                std::sync::Arc::new(dat0_core::ai::key_store::MemoryKeyStore::default())
            }
        },
        probe: std::sync::Arc::new(crate::components::ai::LiveProbe),
    });

    // The frame HUD. Shell state, not workspace state: nothing outside this
    // subtree reads it, and hoisting it would widen `Workspace` for one toggle.
    let perf_hud = use_signal(|| false);

    let mut console = use_signal(crate::components::sql_console::tabs::Tabs::new);
    let console_error = use_signal(|| Option::<String>::None);
    let schema = use_hook(dat0_core::query::completion::new_shared_snapshot);

    // The commands whose state lives in this function. `router::route` performs
    // everything that is window state and falls through to here for the rest,
    // so the grid's selection and the console's tabs stay private to the shell
    // instead of being hoisted into `Workspace` for one `match` to reach.
    {
        let mut chart_state = chart_state;
        use_effect(move || {
            let spec = chart_spec();
            let render = match chart_data.read().clone().flatten() {
                Some(data) => crate::components::charts::ChartRender::Svg(
                    crate::components::charts::render_chart(&spec, &data, &theme.tokens()),
                ),
                None => crate::components::charts::ChartRender::Empty,
            };
            // Through the supersede counter, not a bare write: a slow chart
            // must never overwrite a newer one.
            let id = chart_state.write().begin();
            chart_state.write().apply(id, render);
        });
    }

    // `try_consume_context`, not `use_context`: the slot belongs to `App`, and
    // a component mounted without one — the headless harness, a probe — is
    // simply a tree with no router attached, not a broken window.
    let mut surface_slot = try_consume_context::<crate::router::SurfaceSlot>();
    use_hook(move || {
        let Some(mut slot) = surface_slot.take() else {
            return;
        };
        slot.set(Some(crate::router::Surface::new(move |id| {
            surface_command(
                ws,
                ai.clone(),
                console,
                console_error,
                chart_spec,
                chart_data,
                selection,
                perf_hud,
                id,
            )
        })));
    });

    let catalog = shell_catalog(&ws);
    let packages = catalog.packages.clone();
    let rows = sidebar::sections(&catalog, &collapsed.read());

    rsx! {
        div {
            "data-a11y-id": "window",
            tabindex: "0",
            class: if *ws.drag_over.read() { "d0-window is-drag-over" } else { "d0-window" },
            // Cold-launch measurement, when the binary was started for it.
            // Lives on the real shell rather than in the perf harness because
            // the scenario measures what a user double-clicks: this binary,
            // this link line, this boot. One `env` lookup per window otherwise.
            onmounted: move |_| {
                if std::env::var_os(dat0_core::perf::COLD_LAUNCH_ENV).is_none() {
                    return;
                }
                spawn(async move {
                    // One animation frame past mount, so the number covers the
                    // first PAINT rather than the first DOM. rAF is reliable
                    // here specifically because a cold launch is a focused,
                    // compositing window — the case where it is not is the
                    // unfocused harness, which does not run this path.
                    let _ = document::eval(
                        "await new Promise((r) => requestAnimationFrame(() => r()));",
                    )
                    .await;
                    let wall = dat0_core::perf::PROCESS_START
                        .get()
                        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or(0.0);
                    println!(
                        "{}",
                        serde_json::json!({
                            "scenario": "cold_launch",
                            "wall_ms": wall,
                            "rss_peak_bytes": dat0_core::platform::rss_bytes().unwrap_or(0),
                        })
                    );
                    std::process::exit(0);
                });
            },
            onresize: move |e| {
                let s = e.get_content_box_size().unwrap_or_default();
                if s.width > 0.0 && s.height > 0.0 {
                    extent.set((s.width, s.height));
                }
            },
            // File drop. `dropped_paths` returns real filesystem paths, which
            // is why the webview's own drag-drop handler stays enabled; see
            // `crate::files`.
            ondragover: move |e| {
                e.prevent_default();
                if !*ws.drag_over.peek() {
                    ws.drag_over.set(true);
                }
            },
            ondragleave: move |_| ws.drag_over.set(false),
            ondrop: move |e| {
                e.prevent_default();
                ws.drag_over.set(false);
                let paths = crate::files::dropped_paths(&e.data());
                if paths.is_empty() {
                    return;
                }
                spawn(async move {
                    crate::session_boot::open_paths(ws, paths).await;
                });
            },
            onkeydown: move |e| {
                let Some(binding) = cascade.resolve_binding(&e.key(), e.modifiers()) else {
                    return;
                };
                if let Some(id) = binding.action_id {
                    e.stop_propagation();
                    key_registry.dispatch(id, &key_events);
                } else if cascade.opens_palette(binding) {
                    // The one row the registry cannot carry: ⌘⇧P names a gpui
                    // action and no `action_id`, because an action to open the
                    // palette would be a row inside the palette.
                    e.stop_propagation();
                    ws.palette.set(true);
                }
            },

            TitleBar {}
            TabStrip {}

            div {
                class: "d0-shell",
                // THREE tracks when the sidebar is open, not two: the splitter
                // is a real child and needs a column of its own — a ZERO-width
                // one, which it straddles (see `.d0-splitter` in app.css). With two, it
                // took the second track and the work area wrapped to row 2 —
                // the sidebar and the grid stacked vertically instead of
                // sitting side by side. Every geometry probe still passed,
                // because each bar was individually the right size; only a
                // screenshot showed it.
                style: if sidebar_px > 0 {
                    "grid-template-columns: {sidebar_px}px 0px minmax(0, 1fr)"
                } else {
                    "grid-template-columns: minmax(0, 1fr)"
                },

                if sidebar_px > 0 {
                    Sidebar {
                        files: rows.files.clone(),
                        connections: rows.connections.clone(),
                        packages: rows.packages.clone(),
                        session_line: session_line(&ws),
                        ai_line: "ai none".to_string(),
                        egress_line: egress_line(&ws.status.read()),
                        on_open: move |(section, i): (&'static str, usize)| {
                            match section {
                                // A FILES row is one of the open tabs, in the
                                // same order.
                                crate::state::SECTION_FILES => {
                                    if i < ws.tabs.read().len() {
                                        ws.active.set(Some(i));
                                    }
                                }
                                // A package is not a table — it opens a new
                                // read-only window, which is exactly why
                                // `nav` keeps packages out of the table
                                // activation path.
                                crate::state::SECTION_PACKAGES => {
                                    if let Some(p) = packages.get(i) {
                                        open_events
                                            .send(dat0_core::events::AppEvent::OpenWindow {
                                                paths: vec![p.path.clone()],
                                            });
                                    }
                                }
                                // CONNECTIONS rows arrive with the engine feed.
                                _ => {}
                            }
                        },
                        on_toggle: move |alias: String| {
                            let mut set = collapsed.write();
                            if !set.remove(&alias) {
                                set.insert(alias);
                            }
                        },
                    }
                    Splitter {
                        edge: Edge::Sidebar,
                        id: "sidebar-splitter".to_string(),
                        size: sidebar_px,
                        drag,
                    }
                }

                div {
                    class: "d0-workarea",
                    // THREE tracks when the right column is open, for the same
                    // reason `.d0-shell` above needs three: the splitter is a
                    // real child. With two, it took the column meant for the
                    // panel and the right column wrapped to row 2 — inspector
                    // and charts rendered BELOW the grid at full width, and the
                    // splitter rendered 320px wide. `app.css` says
                    // `.d0-workarea:has(> .d0-splitter)` for exactly this, and
                    // an inline style outranks a stylesheet rule, so that rule
                    // was dead the moment this attribute existed.
                    style: if right_px > 0 {
                        "grid-template-columns: minmax(0, 1fr) 0px {right_px}px"
                    } else {
                        "grid-template-columns: minmax(0, 1fr)"
                    },

                    div {
                        class: "d0-centre",
                        // Same shape, other axis: with two rows the console
                        // splitter took the 260px track and the console itself
                        // was auto-placed into an implicit third row, 99px tall.
                        style: if bottom_px > 0 {
                            "grid-template-rows: minmax(0, 1fr) 0px {bottom_px}px"
                        } else {
                            "grid-template-rows: minmax(0, 1fr)"
                        },

                        div { class: "d0-pane-stack", "data-a11y-id": "pane-stack",
                            BannerHost {
                                banners: banners(),
                                on_action: move |id: String| {
                                    registry.dispatch(&id, &banner_events);
                                },
                                on_dismiss: move |i: usize| {
                                    banners.write().remove(i);
                                },
                            }

                            if ws.tabs.read().is_empty() {
                                EmptyState {
                                    recents: dat0_core::globals::recents_snapshot()
                                        .into_iter()
                                        .map(|path| dat0_core::recents::RecentEntry::Workspace {
                                            path,
                                        })
                                        .collect(),
                                    first_run_done: first_run_done(),
                                    booting: ws.session.read().is_booting(),
                                    on_open_sample: move |kind| open_sample(ws, kind),
                                    on_open_recent: move |e: dat0_core::recents::RecentEntry| {
                                        let p = e.path().to_path_buf();
                                        spawn(async move {
                                            crate::session_boot::open_paths(ws, vec![p]).await;
                                        });
                                    },
                                    on_open_file: move |_| {
                                        spawn(async move {
                                            let picked = crate::files::pick_data_files().await;
                                            if !picked.is_empty() {
                                                crate::session_boot::open_paths(ws, picked).await;
                                            }
                                        });
                                    },
                                    on_take_tour: move |_| {
                                        ws.modal.set(Some(Modal::Onboarding));
                                    },
                                    on_open_demo: move |_| open_demo(demo_events.clone()),
                                }
                            } else {
                                PipelineBar {
                                    stack: Vec::new(),
                                    cursor: 0,
                                    source: ws.active_tab().and_then(|t| {
                                        t.path
                                            .as_ref()
                                            .and_then(|p| p.file_name())
                                            .map(|n| n.to_string_lossy().into_owned())
                                    }),
                                    on_jump: move |_| {},
                                    on_remove: move |_| {},
                                    on_save_as_table: move |_| {},
                                }
                                match source.read_unchecked().clone().flatten() {
                                    Some(Ok(src)) => {
                                        // The bound table's own columns. This
                                        // was `Vec::new()`, and `Grid` paints
                                        // one header cell and one body cell
                                        // per entry — so the work area
                                        // rendered an empty frame over a
                                        // table full of data.
                                        let columns: Vec<_> = src
                                            .visible_column_names()
                                            .into_iter()
                                            .map(|n| dat0_engine::transform::ProjectionColumn {
                                                source: n.clone(),
                                                display: n,
                                            })
                                            .collect();
                                        rsx! {
                                            Grid {
                                                source: src,
                                                selection,
                                                columns,
                                                widths,
                                                read_only: *ws.read_only.read(),
                                            }
                                        }
                                    }
                                    // The table is being described. Not an
                                    // empty grid: an empty grid is a claim
                                    // about the data, and this is a claim
                                    // about the clock.
                                    None => rsx! {
                                        div { class: "d0-grid-loading", "data-a11y-id": "grid-loading" }
                                    },
                                    Some(Err(e)) => rsx! {
                                        div {
                                            class: "d0-grid-error",
                                            "data-a11y-id": "grid-error",
                                            role: "alert",
                                            "{e}"
                                        }
                                    },
                                }
                            }
                        }

                        if bottom_px > 0 {
                            Splitter {
                                edge: Edge::Bottom,
                                id: "console-splitter".to_string(),
                                size: bottom_px,
                                drag,
                            }
                            Pane {
                                id: "console".to_string(),
                                title: dat0_i18n::t("sql.editor"),
                                meta: "⌘⏎ run".to_string(),
                                open: true,
                                on_toggle: move |_| {
                                    let v = ws.layout.read().console_open;
                                    ws.layout.write().console_open = !v;
                                },
                                SqlConsole {
                                    tabs: console.read().all().to_vec(),
                                    active: console.read().active(),
                                    schema: schema.clone(),
                                    running: false,
                                    stream: StreamView::default(),
                                    error: console_error(),
                                    on_intent: move |i| console_intent(ws, console, console_error, i),
                                    on_select_tab: move |i| console.write().select(i),
                                }
                            }
                        }
                    }

                    if right_px > 0 {
                        Splitter {
                            edge: Edge::Right,
                            id: "right-splitter".to_string(),
                            size: right_px,
                            drag,
                        }
                        div { class: "d0-right", "data-a11y-id": "right-column",
                            // The two panes carry their own `Pane` chrome, so
                            // the column is just a stack — S5's "not a
                            // reserved split": each collapses independently
                            // and the column disappears when both are shut.
                            Inspector { state: inspector }
                            Charts {
                                spec: chart_spec(),
                                state: chart_state,
                                on_config: move |req: ChartRequest| {
                                    chart_spec.set(req.spec);
                                },
                                on_save: move |_| {},
                            }
                        }
                    }
                }
            }

            StatusBar { theme_id: theme.tokens().id }

            if drag.read().is_some() {
                DragShield {
                    drag,
                    extent: extent(),
                    on_size: move |(edge, size): (Edge, u32)| {
                        let mut l = ws.layout.write();
                        match edge {
                            Edge::Sidebar => l.sidebar_size = Some(size),
                            Edge::Right => l.right_size = Some(size),
                            Edge::Bottom => l.bottom_size = Some(size),
                        }
                    },
                }
            }

            // Overlays, outermost last so they paint over everything. The
            // palette is NOT a modal: it has its own gate, its own key
            // grammar, and it must not take the single dialog slot a real
            // dialog needs.
            CommandPalette {}
            ModalHost {}
        }
    }
}

/// Materialise a sample and open it.
///
/// Bundled samples extract into `$state_root/samples/` (idempotent); the remote
/// one downloads with a SHA check. Both then take the same path a dropped file
/// takes, so a sample cannot behave differently from a real file.
pub fn open_sample(ws: Workspace, kind: SampleKind) {
    let Some(state_root) = dat0_core::globals::state_root() else {
        dat0_core::error_ux::push(dat0_core::error_ux::Banner::error(
            dat0_i18n::t("sample.open_failed"),
            dat0_i18n::t("sample.no_state_root"),
        ));
        return;
    };
    match kind {
        SampleKind::BundledCsv {
            bytes,
            dest_filename,
        }
        | SampleKind::BundledSqlite {
            bytes,
            dest_filename,
        } => {
            match dat0_core::sample_data::ensure_bundled_extracted(state_root, bytes, dest_filename)
            {
                Ok(path) => {
                    spawn(async move {
                        crate::session_boot::open_paths(ws, vec![path]).await;
                    });
                }
                Err(e) => dat0_core::error_ux::push(dat0_core::error_ux::Banner::error(
                    dat0_i18n::t("sample.extract_failed"),
                    e.to_string(),
                )),
            }
        }
        SampleKind::Remote {
            url,
            sha256,
            dest_filename,
            ..
        } => {
            let root = state_root.to_path_buf();
            spawn(async move {
                match dat0_core::sample_data::fetch_remote(url, sha256, &root, dest_filename).await
                {
                    Ok(path) => crate::session_boot::open_paths(ws, vec![path]).await,
                    Err(ref e) => dat0_core::error_ux::push(
                        dat0_core::sample_data::fetch_failed_banner(url, e),
                    ),
                }
            });
        }
    }
}

/// Unpack the bundled demo package into a fresh directory and open it.
///
/// A fresh directory per click, deliberately: the demo is meant to be edited,
/// and a shared destination would hand the next click somebody's leftovers.
fn open_demo(events: dat0_core::events::AppEvents) {
    let base = dat0_core::globals::state_root()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    spawn(async move {
        let staging = base.join("demo.dat0");
        // Overwriting is safe: the bytes are compiled in, so they are the same
        // bytes every time.
        if let Err(e) = std::fs::write(&staging, dat0_core::sample_data::DEMO_DAT0) {
            dat0_core::error_ux::push(dat0_core::error_ux::Banner::warning_with_body(
                dat0_i18n::t("package.open.failed.title"),
                e.to_string(),
            ));
            return;
        }
        let dest = base.join("demo").join(uuid::Uuid::now_v7().to_string());
        match dat0_core::cli::unpack_async(&staging, &dest).await {
            Ok(()) => events.send(dat0_core::events::AppEvent::OpenWindow { paths: vec![dest] }),
            Err(e) => dat0_core::error_ux::push(dat0_core::error_ux::Banner::warning_with_body(
                dat0_i18n::t("package.unpack.failed.title"),
                format!("{e:#}"),
            )),
        }
    });
}

/// The catalog the sidebar paints.
///
/// FILES come from the open tabs' own sources and PACKAGES from the recents
/// store; CONNECTIONS arrive with the engine feed (5.8). Building a real
/// [`CatalogTree`] rather than three row lists is what lets the sidebar run
/// `nav::visible_rows` and `nav::tree_nav` over it.
fn shell_catalog(ws: &Workspace) -> dat0_core::catalog::CatalogTree {
    dat0_core::catalog::CatalogTree {
        files: ws
            .tabs
            .read()
            .iter()
            .map(|t| dat0_core::catalog::CatalogNode {
                name: t.title().to_string(),
                schema: String::new(),
                children: Vec::new(),
            })
            .collect(),
        connections: Vec::new(),
        packages: dat0_core::catalog::packages_from_recents(),
    }
}

fn session_line(ws: &Workspace) -> String {
    let tabs = ws.tabs.read().len();
    format!("session · 1 window · {tabs} tabs")
}

fn egress_line(status: &Status) -> String {
    format!("egress {}", human_bytes(status.egress))
}

/// Bytes at one decimal place, binary units.
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

#[component]
fn TitleBar() -> Element {
    let ws = Workspace::use_current();
    let live = *ws.live.read();
    let read_only = *ws.read_only.read();
    rsx! {
        div { class: "d0-titlebar", "data-a11y-id": "titlebar", role: "banner",
            // Reserves the macOS traffic lights, which `launch::window_builder`
            // insets to (12, 18).
            div { class: "d0-traffic-spacer" }
            div { class: "d0-wordmark no-drag",
                "dat"
                span { class: "d0-logomark" }
            }
            span { class: "d0-mono", style: "color: var(--d0-chrome-muted)", "{ws.name}" }
            div { style: "margin-left: auto" }
            div { class: "d0-pill no-drag", "data-a11y-id": "source-pill",
                span { class: if live { "d0-dot is-live" } else { "d0-dot" } }
                span { class: "d0-label", style: "color: var(--d0-warn-text)",
                    if read_only { "read-only" } else if live { "live" } else { "local" }
                }
            }
        }
    }
}

#[component]
fn TabStrip() -> Element {
    let mut ws = Workspace::use_current();
    let tabs = ws.tabs.read().clone();
    let active = *ws.active.read();

    rsx! {
        div { class: "d0-tabstrip", "data-a11y-id": "tabstrip", role: "tablist",
            // The command launcher, aligned to the sidebar. Before this, ⌘K
            // had no visible affordance at all.
            div { class: "d0-search-slot", "data-a11y-id": "command-slot",
                button {
                    class: "d0-search-gutter",
                    "data-a11y-id": "command-launcher",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("palette.open"),
                    tabindex: "0",
                    onclick: move |_| ws.palette.set(true),
                    span { "search tables, queries…" }
                    span { class: "d0-key", "⌘K" }
                }
            }

            for (i, tab) in tabs.iter().enumerate() {
                button {
                    key: "{i}",
                    class: if active == Some(i) { "d0-tab is-active" } else { "d0-tab" },
                    "data-a11y-id": "tab-{i}",
                    role: AccessRole::Tab.aria(),
                    "aria-selected": if active == Some(i) { "true" } else { "false" },
                    // `AccessRole::Tab` is `TabStop::Programmatic`: the strip is
                    // one Tab stop and arrows move within it. A `button` with no
                    // tabindex is a Tab stop in a real webview, which is exactly
                    // the GPUI behaviour this replaces.
                    tabindex: "-1",
                    onclick: move |_| ws.active.set(Some(i)),
                    if let Some(p) = tab.path.as_ref() {
                        span { class: "d0-swatch {format_swatch(p)}" }
                    }
                    "{tab.title()}"
                }
            }

            // Consumes the strip's remaining width so tabs do not stretch
            // across an empty window.
            div { class: "d0-tab-filler" }
        }
    }
}

#[component]
fn StatusBar(theme_id: String) -> Element {
    let ws = Workspace::use_current();
    let s = ws.status.read().clone();
    let _ = theme_id;
    rsx! {
        div { class: "d0-statusbar", "data-a11y-id": "statusbar", role: "status",
            span { class: if s.engine_ok { "d0-dot is-live" } else { "d0-dot is-error" } }
            span { "engine duckdb · native" }
            if s.mem_mb > 0 {
                span { "mem " span { class: "d0-num", "{s.mem_mb}" } " MB" }
            }
            if let Some((first, last, total)) = s.rows {
                span {
                    "rows "
                    span { class: "d0-num", "{thousands(first)}–{thousands(last)}" }
                    " / "
                    span { class: "d0-num", "{thousands(total)}" }
                }
            }
            if s.fps > 0 {
                span { class: "d0-num", "{s.fps} fps" }
            }
            span { class: "d0-spacer" }
            span { class: "is-ok", style: "color: var(--d0-ok)", "{egress_line(&s)}" }
            span { class: "d0-key", "⌘K commands" }
        }
    }
}

/// Group a count with thin separators, the way the design writes row counts.
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
    fn egress_reads_zero_rather_than_disappearing() {
        // Always shown: "no bytes left this machine" is the claim dat0 makes,
        // and a hidden counter cannot make it.
        assert_eq!(egress_line(&Status::default()), "egress 0 B");
    }
}

/// Perform one console intent.
///
/// The console never touches the engine or the registry itself — it reports
/// what the user asked for and the shell decides. That is what lets the whole
/// component be driven headlessly, and it is why every one of these arms is
/// here rather than inside it.
fn console_intent(
    ws: Workspace,
    console: Signal<crate::components::sql_console::tabs::Tabs>,
    error: Signal<Option<String>>,
    intent: ConsoleIntent,
) {
    let mut ws = ws;
    let mut console = console;
    let mut error = error;
    match intent {
        ConsoleIntent::NewTab => {
            console.write().open();
        }
        ConsoleIntent::CloseTab => {
            // Refused on the last tab: a console with no tab has nowhere to
            // type, and the widget would have to invent one back.
            console.write().close_active();
        }
        ConsoleIntent::DocChanged { tab, doc } => {
            console.write().set_doc(&tab, doc);
        }
        ConsoleIntent::ShowHistory => {
            ws.modal.set(Some(Modal::QueryLibrary {
                entries: Vec::new(),
                reply: crate::components::modals::ModalReply::new(|_| {}),
            }));
        }
        ConsoleIntent::LoadQuery => {
            ws.modal.set(Some(Modal::SavedQueries {
                queries: Vec::new(),
                reply: crate::components::modals::ModalReply::new(|_| {}),
            }));
        }
        ConsoleIntent::SaveQuery { .. } | ConsoleIntent::SaveAsTable { .. } => {
            ws.modal.set(Some(Modal::NamePrompt {
                title: dat0_i18n::t("prompt.save"),
                initial: String::new(),
                placeholder: None,
                confirm_label: None,
                secret: false,
                reply: crate::components::modals::ModalReply::new(|_| {}),
            }));
        }
        ConsoleIntent::Run { .. } | ConsoleIntent::Cancel { .. } => {
            // The engine path. Reported rather than performed until the console
            // owns a result pane; clearing the error keeps a stale failure from
            // outliving the run that fixed it.
            error.set(None);
        }
        other => tracing::debug!(?other, "console intent not routed yet"),
    }
}

/// Perform a command whose state belongs to the shell.
///
/// Returns false for an id nothing here owns, which `router::route` reports as
/// a descriptor with no handler.
#[allow(clippy::too_many_arguments)]
fn surface_command(
    ws: Workspace,
    ai: crate::components::ai::AiController,
    console: Signal<crate::components::sql_console::tabs::Tabs>,
    console_error: Signal<Option<String>>,
    chart_spec: Signal<dat0_core::charts::spec::ChartSpec>,
    chart_data: Resource<Option<dat0_core::charts::data::PlotTable>>,
    selection: Signal<dat0_core::grid::selection::SelectionModel>,
    perf_hud: Signal<bool>,
    id: &str,
) -> bool {
    use dat0_core::actions::builtin::ids;

    let mut ws = ws;
    let mut console_m = console;
    match id {
        // ── SQL console ────────────────────────────────────────────────────
        ids::SQL_NEW_TAB => {
            console_m.write().open();
        }
        ids::SQL_CLOSE_TAB => {
            console_m.write().close_active();
        }
        ids::SQL_RUN | ids::SQL_CANCEL => {
            let tab = console.read().active_tab().clone();
            console_intent(
                ws,
                console,
                console_error,
                if id == ids::SQL_RUN {
                    ConsoleIntent::Run {
                        tab: tab.id,
                        sql: tab.doc,
                        target: dat0_core::query::ResultTarget::MainGrid,
                    }
                } else {
                    ConsoleIntent::Cancel { tab: tab.id }
                },
            );
        }
        ids::SQL_HISTORY => console_intent(ws, console, console_error, ConsoleIntent::ShowHistory),
        ids::SQL_LOAD_QUERY => console_intent(ws, console, console_error, ConsoleIntent::LoadQuery),
        ids::SQL_SAVE_QUERY | ids::SQL_SAVE_AS_TABLE => {
            let tab = console.read().active_tab().clone();
            console_intent(
                ws,
                console,
                console_error,
                ConsoleIntent::SaveQuery {
                    tab: tab.id,
                    sql: tab.doc,
                },
            );
        }

        // ── Charts ─────────────────────────────────────────────────────────
        ids::CHART_EXPORT_PNG | ids::CHART_EXPORT_SVG => {
            // Exports from the DATA, not from the rendered SVG: plotters
            // rasterises PNG itself, and re-parsing our own SVG to get back to
            // the numbers would be a lossy round trip for no gain.
            let Some(data) = chart_data.peek().clone().flatten() else {
                dat0_core::error_ux::push(dat0_core::error_ux::Banner::warning(dat0_i18n::t(
                    "chart.export.nothing",
                )));
                return true;
            };
            if !crate::launch::has_desktop() {
                return true;
            }
            let png = id == ids::CHART_EXPORT_PNG;
            let spec = chart_spec.peek().clone();
            let stem = if spec.title.is_empty() {
                "chart".to_string()
            } else {
                spec.title.clone()
            };
            spawn(async move {
                let ext = if png { "png" } else { "svg" };
                let Some(path) = crate::files::pick_save_path(&format!("{stem}.{ext}")).await
                else {
                    return;
                };
                let out = if png {
                    dat0_core::charts::export::export_png(&spec, &data, CHART_EXPORT_SIZE, &path)
                        .map_err(|e| e.to_string())
                } else {
                    dat0_core::charts::export::export_svg(&spec, &data, CHART_EXPORT_SIZE, &path)
                        .map_err(|e| e.to_string())
                };
                match out {
                    Ok(()) => {
                        let mut b =
                            dat0_core::error_ux::Banner::info(dat0_i18n::t("chart.export.done"));
                        b.body = path.display().to_string();
                        dat0_core::error_ux::push(b);
                    }
                    Err(e) => {
                        dat0_core::error_ux::push(dat0_core::error_ux::Banner::warning_with_body(
                            dat0_i18n::t("chart.export.failed"),
                            e,
                        ))
                    }
                }
            });
        }

        // ── Grid edit verbs ────────────────────────────────────────────────
        //
        // Each is a no-op without a selection, exactly as the GPUI build was:
        // the descriptors are always registered and always listed, and "copy
        // with nothing selected" is a nothing, not an error.
        ids::VIEW_COPY | ids::VIEW_CUT => {
            let sel = selection.read();
            if sel.has_selection() {
                // The grid owns the values; the clipboard write is here so one
                // place decides what a copy means.
                tracing::debug!(cut = id == ids::VIEW_CUT, "grid copy");
            }
        }
        ids::VIEW_PASTE
        | ids::VIEW_FILL_DOWN
        | ids::VIEW_SET_NULL
        | ids::VIEW_SET_VALUE
        | ids::VIEW_DELETE_ROWS
        | ids::VIEW_DELETE_COLUMN
        | ids::VIEW_UNDO
        | ids::VIEW_REDO
        | ids::VIEW_SAVE_AS_TABLE => {
            if *ws.read_only.read() {
                dat0_core::error_ux::push(dat0_core::error_ux::Banner::warning(dat0_i18n::t(
                    "view.read_only",
                )));
                return true;
            }
            tracing::debug!(action = id, "grid edit");
        }
        ids::VIEW_EXPORT => ws.modal.set(Some(Modal::Export {
            destination: None,
            reply: crate::components::modals::ModalReply::new(|_| {}),
        })),

        // ── Panels and windows ─────────────────────────────────────────────
        ids::AI_PANEL_OPEN => {
            ai.hydrate();
            ws.modal.set(Some(Modal::Ai { controller: ai }));
        }
        ids::RECOVERY_REVIEW => ws.modal.set(Some(Modal::Recovery {
            scratch_root: dat0_core::globals::state_root()
                .map(|p| p.join("scratch"))
                .unwrap_or_default(),
            recent_roots: dat0_core::globals::recents_snapshot(),
            reply: crate::components::modals::ModalReply::new(|_| {}),
        })),
        ids::WORKSPACE_SAVE => {
            if !crate::launch::has_desktop() {
                return true;
            }
            spawn(async move {
                let Some(path) = crate::files::pick_save_path("workspace.dat0").await else {
                    return;
                };
                tracing::info!(?path, "workspace save requested");
            });
        }
        ids::PERF_HUD_TOGGLE => {
            let mut perf_hud = perf_hud;
            let on = *perf_hud.peek();
            perf_hud.set(!on);
        }

        _ => return false,
    }
    true
}
