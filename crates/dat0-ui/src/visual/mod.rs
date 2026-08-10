//! The visual scene catalogue: every surface, in every state worth looking at.
//!
//! One declaration per scene, read by three consumers:
//!
//! * `tests/visual_snapshot.rs` — SSRs each scene and `insta`-snapshots the
//!   markup. Hermetic, no display, runs in CI.
//! * `examples/visual_probe.rs` — walks every scene in one real wry window and
//!   asserts computed geometry, containment and typography. Needs a display.
//! * `examples/visual_page.rs` — writes each scene to a self-contained HTML
//!   file for human review.
//!
//! It lives in `src/` rather than `tests/` because a `tests/` module is
//! unreachable from `examples/`, and all three consumers need it. Behind the
//! `visual` feature, which only the self-dev-dependency turns on, so none of it
//! reaches the shipped binary.
//!
//! # Why props, not resources
//!
//! Effects and async do not run under SSR, so every scene renders its *initial*
//! state. Scene data therefore arrives as props or as a signal seeded at mount
//! — never through `use_resource`. Where a surface has no prop path to a
//! populated state (the charts pane's SVG, which the shell fills from a
//! resource), the already-rendered value is passed directly rather than a prop
//! being added to production code.

pub mod fixtures;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_core::grid::selection::{CellCoord, SelectionModel};
use dat0_core::view::filter_popover::ColumnType;
use dat0_engine::transform::{
    FilterOp, FilterValue, Scalar, SortDirection, SortKey, Transformation,
};

use crate::components::ai::{AiController, AiDeps, LiveProbe, StreamKind, StreamPhase, StreamView};
use crate::components::charts::{ChartLoad, ChartRender, Charts, render_chart};
use crate::components::connections::Connections;
use crate::components::grid::{COL_W_DEFAULT, Grid};
use crate::components::import_wizard::{Step, WizardModel};
use crate::components::inspector::{Inspector, InspectorState};
use crate::components::modals::{ModalHost, ModalReply};
use crate::components::sidebar::{self, Sidebar};
use crate::components::sql_console::{SqlConsole, Tab as ConsoleTab};
use crate::components::update_ui::UpdateState;
use crate::components::workspace_in_use::InUse;
use crate::state::{Modal, TabView, Workspace};
use crate::theme::Theme;

pub use fixtures::Fixtures;

// ── The catalogue ────────────────────────────────────────────────────────────

/// A cheap, comparable handle so [`Fixtures`] can be a prop.
///
/// `PartialEq` is `Arc::ptr_eq`: props must compare, and the fixtures are built
/// once and never mutated, so pointer identity is the honest answer.
#[derive(Clone)]
pub struct Handle(Arc<Fixtures>);

impl Handle {
    pub fn new(fx: Fixtures) -> Self {
        Self(Arc::new(fx))
    }

    /// Scrub this run's fixture root out of rendered markup.
    pub fn scrub(&self, html: String) -> String {
        self.0.scrub(html)
    }
}

impl std::ops::Deref for Handle {
    type Target = Fixtures;
    fn deref(&self) -> &Fixtures {
        &self.0
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// What the scene root may legitimately scroll on.
///
/// Anything else overflowing is a layout bug — see the probe's invariant V2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scroll {
    None,
    Vertical,
    Horizontal,
    Both,
}

impl Scroll {
    pub fn allows_x(self) -> bool {
        matches!(self, Scroll::Horizontal | Scroll::Both)
    }

    pub fn allows_y(self) -> bool {
        matches!(self, Scroll::Vertical | Scroll::Both)
    }
}

pub struct Scene {
    /// Stable id. Also the snapshot name and the page file name.
    pub id: &'static str,
    pub surface: &'static str,
    pub state: &'static str,
    pub scroll: Scroll,
    /// `true` pins the scene root to 900px and clips it — the full-window
    /// surfaces. `false` gives it width only and lets it size to content —
    /// strips, popovers and modals. Declared here so no call site decides it.
    pub fixed_height: bool,
    /// True when the rendered markup itself differs per theme, so the scene is
    /// snapshotted three times rather than once.
    ///
    /// It is not a guess, and it is not the `Theme::use_current()` grep either
    /// — `tests/visual_snapshot.rs` renders **every** scene in all three
    /// builtins and fails when this flag disagrees with what came out.
    ///
    /// The SSR'd subtree excludes the `<style id="d0-theme">` block that carries
    /// `css_vars()` — `ThemeStyle` emits it outside the scene root
    /// (`src/theme.rs`) — so most markup is byte-identical across themes and
    /// three snapshots would be three copies. Exactly two scenes differ:
    ///
    /// * `gallery/tokens`, which mounts its own `ThemeStyle` and marks the
    ///   active theme's button;
    /// * `charts/rendered`, whose SVG is rasterised from the theme's palette by
    ///   `render_chart`.
    ///
    /// `shell/*` and `console/*` read the theme but spend it outside the DOM —
    /// the console hands `editor::theme_vars` to CodeMirror over the eval
    /// channel — so their markup does not move. Where a theme actually lives is
    /// pinned by the `theme-vars__*` snapshots instead.
    pub theme_sensitive: bool,
}

impl Scene {
    /// The scene id as a file-name and snapshot-name stem.
    ///
    /// A scene id carries a `/`, which is legal in neither.
    pub fn stem(&self) -> String {
        self.id.replace('/', "__")
    }
}

/// Shorthand for the common case: fixed height, no scroll, one theme.
const fn s(id: &'static str, surface: &'static str, state: &'static str) -> Scene {
    Scene {
        id,
        surface,
        state,
        scroll: Scroll::None,
        fixed_height: true,
        theme_sensitive: false,
    }
}

/// Content-height variant: strips, popovers and modal panels that size to what
/// is in them.
const fn strip(id: &'static str, surface: &'static str, state: &'static str) -> Scene {
    Scene {
        fixed_height: false,
        ..s(id, surface, state)
    }
}

const fn scrolls(mut sc: Scene, axis: Scroll) -> Scene {
    sc.scroll = axis;
    sc
}

const fn themed(mut sc: Scene) -> Scene {
    sc.theme_sensitive = true;
    sc
}

pub const SCENES: &[Scene] = &[
    s("shell/empty", "shell", "no tabs, session booting"),
    s("shell/populated", "shell", "three tabs, first active"),
    s("shell/sidebar-collapsed", "shell", "sidebar closed"),
    s("shell/right-column-open", "shell", "inspector + charts"),
    s("shell/console-open", "shell", "bottom dock open"),
    scrolls(
        s("grid/populated", "grid", "12 rows, no selection"),
        Scroll::Vertical,
    ),
    // No `grid/empty`: `GridDataSource::new` cannot open a zero-row table, so
    // the state does not exist in the shipped app either. See
    // `visual/fixtures.rs`.
    scrolls(
        s("grid/read-only", "grid", "read-only workspace"),
        Scroll::Vertical,
    ),
    scrolls(
        s("grid/selection", "grid", "rows 1-3 x cols 0-1"),
        Scroll::Vertical,
    ),
    // The three overlay families below are `position: absolute` and are placed
    // in client coordinates against the window their host fills, so they need
    // the full 1440x900 frame rather than a content-height strip: without it
    // the scene root collapses to zero height and the overlay reads as
    // "outside its own frame". Measured, not assumed — see V1 and V4.
    s("cell-editor/text", "cell editor", "string column"),
    s("cell-editor/bool", "cell editor", "boolean select"),
    s("context-menu/default", "context menu", "with selection"),
    s("context-menu/read-only", "context menu", "mutations gated"),
    strip("header/default", "grid header", "four columns"),
    strip(
        "header/dragging",
        "grid header",
        "reorder ghost on column 1",
    ),
    s("sidebar/empty", "sidebar", "three empty sections"),
    s(
        "sidebar/populated",
        "sidebar",
        "files, connections, packages",
    ),
    s(
        "sidebar/section-collapsed",
        "sidebar",
        "connections collapsed",
    ),
    s("console/idle", "sql console", "two tabs, idle"),
    s("console/running", "sql console", "statement in flight"),
    s("console/error", "sql console", "catalog error strip"),
    s("console/preview", "sql console", "nl2sql streaming"),
    s(
        "empty-state/first-run",
        "empty state",
        "no recents, first run",
    ),
    s("empty-state/returning", "empty state", "four recents"),
    s(
        "empty-state/booting",
        "empty state",
        "session still opening",
    ),
    strip("banner/info", "banner", "one info"),
    strip("banner/warning", "banner", "one dismissible warning"),
    strip("banner/error", "banner", "error with a primary action"),
    strip("banner/stacked", "banner", "all three at once"),
    strip(
        "pipeline-bar/chips",
        "pipeline bar",
        "three steps, cursor at end",
    ),
    strip(
        "pipeline-bar/scrubbed",
        "pipeline bar",
        "cursor at 1, steps 2-3 past",
    ),
    scrolls(
        s("inspector/profiled", "inspector", "numeric column focused"),
        Scroll::Vertical,
    ),
    s("inspector/empty", "inspector", "no target"),
    scrolls(
        s("inspector/lineage", "inspector", "deep chain, depth clamp"),
        Scroll::Vertical,
    ),
    themed(s("charts/rendered", "charts", "bar chart svg")),
    s("charts/empty", "charts", "nothing bound"),
    s("charts/error", "charts", "binder error"),
    scrolls(
        s("palette/open", "command palette", "full builtin registry"),
        Scroll::Vertical,
    ),
    s("modal/about", "modal", "about, newer release known"),
    s("modal/export", "modal", "export with a destination"),
    s("modal/name-prompt", "modal", "save prompt, not secret"),
    s("modal/connections", "modal", "md disconnected, one sqlite"),
    s("modal/saved-queries", "modal", "four saved queries"),
    s(
        "modal/query-library",
        "modal",
        "five history rows, one failed",
    ),
    s("modal/onboarding", "modal", "tour panel one"),
    s("modal/crash-report", "modal", "staged payload"),
    s(
        "modal/workspace-in-use",
        "modal",
        "foreign-machine conflict",
    ),
    s("modal/live-refresh", "modal", "3 edits, 1 delete dropped"),
    s("modal/recovery", "modal", "two orphans, one incomplete"),
    s("modal/import-wizard", "modal", "columns step, one invalid"),
    s("modal/update", "modal", "available, manual check"),
    s("modal/ai", "modal", "no provider configured"),
    strip("import/running", "import progress", "4M of 12M rows"),
    strip("import/failed", "import progress", "failed"),
    scrolls(
        s("settings/default", "settings", "profile section"),
        Scroll::Vertical,
    ),
    s(
        "filter-popover/text",
        "filter popover",
        "contains, candidates",
    ),
    s(
        "filter-popover/numeric",
        "filter popover",
        "between, both bounds",
    ),
    strip("pane/open", "pane", "open, with meta"),
    strip("pane/collapsed", "pane", "collapsed"),
    scrolls(
        themed(s("gallery/tokens", "gallery", "the whole token set")),
        Scroll::Vertical,
    ),
];

/// The scene with this id.
pub fn scene(id: &str) -> Option<&'static Scene> {
    SCENES.iter().find(|s| s.id == id)
}

/// Insert a newline between adjacent tags.
///
/// `dioxus_ssr::render` emits one line. An unreadable diff is a snapshot nobody
/// reviews, and reviewing the diff *is* the visual check for the snapshot tier.
pub fn normalise(html: String) -> String {
    html.replace("><", ">\n<")
}

// ── The host ─────────────────────────────────────────────────────────────────

/// Renders one scene, contexts and all. The single entry point both tiers use.
///
/// Every hook below runs unconditionally, before the scene is chosen, so the
/// hook list does not depend on `id`. Per-scene mutable state lives in the
/// wrapper components further down, which are distinct types and so remount
/// when the scene changes.
#[component]
pub fn SceneHost(fx: Handle, id: String, theme: String) -> Element {
    // The four contexts every host in the existing suites provides. Copied from
    // `examples/visual_page.rs`, which is the most complete one and is already
    // proven to SSR. Deliberately NOT `router::SurfaceSlot`: `Shell` takes that
    // with `try_consume_context` and tolerates its absence.
    let ws = Workspace::provide();
    let mut th = Theme::provide(None);
    th.set(&theme);
    use_context_provider(|| {
        let reg = ActionRegistry::new();
        dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
        reg
    });
    use_context_provider(|| AppEvents::channel().0);

    // Modal payloads that are signals, and the AI controller, which is a hook.
    // Built here so the hook list is the same for every scene.
    let connections = use_signal(|| {
        let mut c = Connections::default();
        c.add_sqlite("crm", "/data/crm.db");
        c
    });
    let wizard = use_signal(wizard_model);
    let ai = AiController::use_new(AiDeps {
        store: Arc::new(dat0_core::settings::store::SettingsStore::with_path(
            fx.settings_path.clone(),
        )),
        // Never the real keychain: a scene must not depend on what is in the
        // developer's login keyring.
        keys: Arc::new(dat0_core::ai::key_store::MemoryKeyStore::default()),
        probe: Arc::new(LiveProbe),
    });

    let sc = scene(&id).unwrap_or_else(|| panic!("no scene named {id}"));

    {
        let id = id.clone();
        let fx = fx.clone();
        use_hook(move || seed(&id, ws, &fx, connections, wizard, ai));
    }

    let style = if sc.fixed_height {
        "width: 1440px; height: 900px; overflow: hidden"
    } else {
        "width: 1440px"
    };

    rsx! {
        div { class: "d0-scene", "data-scene": "{sc.id}", style: "{style}", {body(sc, &fx)} }
    }
}

/// Put the scene's state into the workspace, once, at mount.
///
/// The shell surfaces and the modal slot have no props: they read `Workspace`.
/// This is the only place that writes it, so a scene's state is declared in one
/// spot rather than smeared across the component tree.
fn seed(
    id: &str,
    mut ws: Workspace,
    fx: &Fixtures,
    connections: Signal<Connections>,
    wizard: Signal<WizardModel>,
    ai: AiController,
) {
    match id {
        "shell/populated" => {
            ws.name.set("q3-review".into());
            ws.tabs.set(vec![
                TabView {
                    table: "sales".into(),
                    path: Some(PathBuf::from("/data/sales.csv")),
                },
                TabView {
                    table: "trips".into(),
                    path: Some(PathBuf::from("/data/trips.parquet")),
                },
                TabView {
                    table: "revenue_by_region".into(),
                    path: None,
                },
            ]);
            ws.active.set(Some(0));
            ws.status.set(crate::state::Status {
                engine_ok: true,
                mem_mb: 4096,
                rows: Some((1, 12, 12)),
                fps: 60,
                egress: 0,
            });
        }
        "shell/sidebar-collapsed" => ws.layout.write().sidebar_open = false,
        "shell/right-column-open" => {
            let mut l = ws.layout.write();
            l.inspector_visible = true;
            l.charts_visible = true;
        }
        "shell/console-open" => ws.layout.write().console_open = true,
        // `Inspector` and `Charts` are `Pane`s whose open state is
        // `DockLayout` (S5), not a prop. Without this the standalone scenes
        // render a collapsed header and nothing else.
        id if id.starts_with("inspector/") => ws.layout.write().inspector_visible = true,
        id if id.starts_with("charts/") => ws.layout.write().charts_visible = true,
        "sidebar/section-collapsed" => {
            ws.layout
                .write()
                .sections_collapsed
                .insert(crate::state::SECTION_CONNECTIONS.to_string());
        }
        "palette/open" => ws.palette.set(true),
        _ if id.starts_with("modal/") => {
            ws.modal.set(Some(modal(id, fx, connections, wizard, ai)));
        }
        _ => {}
    }
}

/// The modal slot's payload for a `modal/*` scene.
fn modal(
    id: &str,
    fx: &Fixtures,
    connections: Signal<Connections>,
    wizard: Signal<WizardModel>,
    ai: AiController,
) -> Modal {
    // Every panel that reports a decision takes one of these. Nothing in a
    // scene can be clicked, so they are all inert.
    let reply = || ModalReply::new(|_| {});
    match id {
        "modal/about" => Modal::About {
            newer: Some("0.2.0".into()),
            // The check is a blocking `ureq` call on `spawn_blocking`, and the
            // answer is already known.
            check_latest: false,
        },
        "modal/export" => Modal::Export {
            destination: Some(PathBuf::from("/data/exports")),
            reply: reply(),
        },
        "modal/name-prompt" => Modal::NamePrompt {
            title: dat0_i18n::t("prompt.save"),
            initial: String::new(),
            placeholder: None,
            confirm_label: None,
            secret: false,
            reply: reply(),
        },
        "modal/connections" => Modal::Connections {
            state: connections,
            reply: reply(),
        },
        "modal/saved-queries" => Modal::SavedQueries {
            queries: fx.queries.clone(),
            reply: reply(),
        },
        "modal/query-library" => Modal::QueryLibrary {
            entries: fx.history.clone(),
            reply: reply(),
        },
        "modal/onboarding" => Modal::Onboarding,
        "modal/crash-report" => Modal::CrashReport {
            staged: Some(dat0_core::telemetry::crash::StagedCrash {
                message: "called `Option::unwrap()` on a `None` value".into(),
                backtrace: "   0: dat0_ui::components::grid::Grid\n   1: dioxus_core::scope".into(),
                version: "0.1.0".into(),
            }),
            data_dir: PathBuf::from("/data/state"),
        },
        "modal/workspace-in-use" => Modal::WorkspaceInUse {
            kind: InUse::Conflict {
                holder: dat0_core::workspace::lock_manifest::LockManifest {
                    pid: 4242,
                    hostname: "studio.local".into(),
                    started_at: "1700000000".into(),
                    dat0_version: "0.1.0".into(),
                    tombstoned: false,
                },
                // A fixed "now", so the humanised age is a constant.
                now_secs: 1_700_003_600,
            },
            reply: reply(),
        },
        "modal/live-refresh" => Modal::LiveRefresh {
            dropped_edits: 3,
            dropped_deletes: 1,
            reply: reply(),
        },
        "modal/recovery" => Modal::Recovery {
            scratch_root: fx.recovery_root.clone(),
            recent_roots: vec![fx.incomplete_root.clone()],
            reply: reply(),
        },
        "modal/import-wizard" => Modal::ImportWizard {
            model: wizard,
            reply: reply(),
        },
        "modal/update" => Modal::Update {
            state: UpdateState::Available {
                version: "0.2.0".into(),
                artifact: dat0_core::update::manifest::ArtifactEntry {
                    url: "https://example.invalid/dat0-0.2.0.dmg".into(),
                    sha256: "0".repeat(64),
                    size: 48_234_496,
                },
            },
            is_manual: true,
            reply: reply(),
        },
        "modal/ai" => Modal::Ai { controller: ai },
        other => panic!("no modal scene named {other}"),
    }
}

/// The scene's subtree.
///
/// A plain function, not a component: it introduces no scope and no hooks, so
/// switching scenes swaps the child at this position rather than reshaping
/// `SceneHost`'s own hook list.
fn body(sc: &Scene, fx: &Handle) -> Element {
    match sc.id {
        id if id.starts_with("shell/") => rsx! { crate::components::shell::Shell {} },
        id if id.starts_with("modal/") => rsx! { ModalHost {} },

        "grid/populated" => {
            rsx! { GridScene { fx: fx.clone(), read_only: false, selected: false } }
        }
        "grid/read-only" => rsx! { GridScene { fx: fx.clone(), read_only: true, selected: false } },
        "grid/selection" => rsx! { GridScene { fx: fx.clone(), read_only: false, selected: true } },

        "cell-editor/text" => rsx! {
            crate::components::grid::cell_editor::CellEditor {
                cell: CellCoord { row: 1, col: 1 },
                initial: "alpha".to_string(),
                widths: widths(fx),
                column_type: ColumnType::String,
                on_done: |_| {},
            }
        },
        "cell-editor/bool" => rsx! {
            crate::components::grid::cell_editor::CellEditor {
                cell: CellCoord { row: 1, col: 3 },
                initial: "true".to_string(),
                widths: widths(fx),
                column_type: ColumnType::Bool,
                on_done: |_| {},
            }
        },

        "context-menu/default" => rsx! {
            crate::components::grid::context_menu::ContextMenu {
                at: (420.0, 260.0),
                cell: CellCoord { row: 2, col: 1 },
                has_selection: true,
                read_only: false,
                on_pick: |_| {},
                on_dismiss: |_| {},
            }
        },
        "context-menu/read-only" => rsx! {
            crate::components::grid::context_menu::ContextMenu {
                at: (420.0, 260.0),
                cell: CellCoord { row: 2, col: 1 },
                has_selection: true,
                read_only: true,
                on_pick: |_| {},
                on_dismiss: |_| {},
            }
        },

        "header/default" => rsx! {
            crate::components::grid::header::Header {
                columns: fx.columns.clone(),
                widths: widths(fx),
                scroll_left: 0.0,
                dragging_col: None,
                on_resize_start: |_| {},
                on_reorder_start: |_| {},
                on_reorder_drop: |_| {},
            }
        },
        "header/dragging" => rsx! {
            crate::components::grid::header::Header {
                columns: fx.columns.clone(),
                widths: widths(fx),
                scroll_left: 0.0,
                dragging_col: Some(1),
                on_resize_start: |_| {},
                on_reorder_start: |_| {},
                on_reorder_drop: |_| {},
            }
        },

        "sidebar/empty" => rsx! {
            Sidebar {
                files: vec![], connections: vec![], packages: vec![],
                session_line: "session · 1 window · 0 tabs".to_string(),
                ai_line: "ai none".to_string(),
                egress_line: "egress 0 B".to_string(),
                on_open: |_| {}, on_toggle: |_| {},
            }
        },
        "sidebar/populated" | "sidebar/section-collapsed" => {
            let rows = sidebar::sections(&fx.catalog, &HashSet::new());
            rsx! {
                Sidebar {
                    files: rows.files, connections: rows.connections, packages: rows.packages,
                    session_line: "session · 1 window · 3 tabs".to_string(),
                    ai_line: "ai none".to_string(),
                    egress_line: "egress 0 B".to_string(),
                    on_open: |_| {}, on_toggle: |_| {},
                }
            }
        }

        id if id.starts_with("console/") => {
            rsx! { ConsoleScene { state: id.trim_start_matches("console/").to_string() } }
        }

        "empty-state/first-run" => rsx! { EmptyScene { first_run_done: false, booting: false } },
        "empty-state/returning" => rsx! { EmptyScene { first_run_done: true, booting: false } },
        "empty-state/booting" => rsx! { EmptyScene { first_run_done: true, booting: true } },

        id if id.starts_with("banner/") => rsx! {
            crate::components::banner::BannerHost {
                banners: banners(id),
                on_action: |_| {},
                on_dismiss: |_| {},
            }
        },

        "pipeline-bar/chips" => rsx! { PipelineScene { cursor: 3 } },
        // Not `expanded`: the chevron's state is `PipelineBar`'s own
        // `use_signal` and no prop reaches it, so under SSR the timeline is
        // unreachable. The cursor half of that scene IS prop-reachable, and it
        // is the half that changes what the chips say.
        "pipeline-bar/scrubbed" => rsx! { PipelineScene { cursor: 1 } },

        "inspector/profiled" => {
            rsx! { InspectorScene { fx: fx.clone(), mode: "profiled".to_string() } }
        }
        "inspector/empty" => rsx! { InspectorScene { fx: fx.clone(), mode: "empty".to_string() } },
        "inspector/lineage" => {
            rsx! { InspectorScene { fx: fx.clone(), mode: "lineage".to_string() } }
        }

        "charts/rendered" => rsx! { ChartsScene { fx: fx.clone(), mode: "rendered".to_string() } },
        "charts/empty" => rsx! { ChartsScene { fx: fx.clone(), mode: "empty".to_string() } },
        "charts/error" => rsx! { ChartsScene { fx: fx.clone(), mode: "error".to_string() } },

        "palette/open" => rsx! { crate::components::command_palette::CommandPalette {} },

        "import/running" => rsx! {
            crate::components::import_progress::ImportProgress {
                state: crate::components::import_progress::ImportState::Running {
                    file: PathBuf::from("/data/trips.parquet"),
                    done: 4_000_000,
                    total: 12_000_000,
                },
                on_cancel: |_| {}, on_dismiss: |_| {},
            }
        },
        "import/failed" => rsx! {
            crate::components::import_progress::ImportProgress {
                state: crate::components::import_progress::ImportState::Failed {
                    file: PathBuf::from("/data/trips.parquet"),
                    error: "Invalid Input Error: unterminated quote at line 4102".into(),
                },
                on_cancel: |_| {}, on_dismiss: |_| {},
            }
        },

        // `SettingsPanel`, never `SettingsWindow`: the window calls
        // `dioxus::desktop::use_asset_handler` during render, which has no
        // desktop context under SSR.
        "settings/default" => rsx! {
            crate::components::settings_ui::SettingsPanel {
                store: crate::components::settings_ui::Store::open(fx.settings_path.clone()),
                events: crate::components::settings_ui::Bus(None),
            }
        },

        "filter-popover/text" => rsx! {
            crate::components::filter_popover::FilterPopover {
                column: "region".to_string(),
                column_type: ColumnType::String,
                existing: None,
                at: (360.0, 120.0),
                candidates: vec!["north".into(), "south".into(), "east".into(), "west".into()],
                total_distinct: 4,
                on_outcome: |_| {},
            }
        },
        "filter-popover/numeric" => rsx! {
            crate::components::filter_popover::FilterPopover {
                column: "revenue".to_string(),
                column_type: ColumnType::Numeric,
                existing: Some(Transformation::Filter {
                    column: "revenue".into(),
                    op: FilterOp::Between,
                    value: FilterValue::Range {
                        lo: Scalar::Float(500.0),
                        hi: Scalar::Float(1500.0),
                        inclusive: true,
                    },
                }),
                at: (620.0, 120.0),
                candidates: vec![],
                total_distinct: 12,
                on_outcome: |_| {},
            }
        },

        "pane/open" => rsx! {
            crate::components::pane::Pane {
                id: "inspector".to_string(),
                title: dat0_i18n::t("inspector.title"),
                meta: "revenue · DOUBLE".to_string(),
                open: true,
                on_toggle: |_| {},
                p { class: "d0-body", "twelve rows, four columns" }
            }
        },
        "pane/collapsed" => rsx! {
            crate::components::pane::Pane {
                id: "inspector".to_string(),
                title: dat0_i18n::t("inspector.title"),
                meta: "revenue · DOUBLE".to_string(),
                open: false,
                on_toggle: |_| {},
                p { class: "d0-body", "twelve rows, four columns" }
            }
        },

        "gallery/tokens" => rsx! { crate::gallery::Gallery {} },

        other => panic!("no scene body for {other}"),
    }
}

// ── Scene wrappers ───────────────────────────────────────────────────────────
//
// One per surface that needs its own signals. Each is a distinct component
// type, so switching to a scene in another group unmounts it and the next one
// mounts fresh. Switching *within* a group keeps the scope alive, which is why
// the probe drops the scene entirely for one settle between measurements.

#[component]
fn GridScene(fx: Handle, read_only: bool, selected: bool) -> Element {
    let source = fx.source.clone();
    let rows = source.row_count.max(1) as usize;
    let cols = fx.columns.len().max(1);
    let selection = use_signal(move || {
        let mut m = SelectionModel::new(rows, cols);
        if selected {
            m.click(CellCoord { row: 1, col: 0 });
            m.extend_to(CellCoord { row: 3, col: 1 });
        }
        m
    });
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);

    rsx! {
        Grid {
            source,
            selection,
            columns: fx.columns.clone(),
            widths,
            read_only,
            on_edit: |_| {},
            on_action: |_| {},
        }
    }
}

#[component]
fn ConsoleScene(state: String) -> Element {
    let schema = use_hook(dat0_core::query::completion::new_shared_snapshot);
    let tabs = vec![
        ConsoleTab {
            id: "tab-1".into(),
            title: "query 1".into(),
            doc: "SELECT region, SUM(revenue) AS total\nFROM sales\nGROUP BY region".into(),
        },
        ConsoleTab {
            id: "tab-2".into(),
            title: "query 2".into(),
            doc: "SUMMARIZE sales".into(),
        },
    ];
    let stream = if state == "preview" {
        StreamView {
            kind: Some(StreamKind::NlToSql),
            prompt: "total revenue by region".into(),
            text: "SELECT region, SUM(revenue)\nFROM sal".into(),
            phase: StreamPhase::Streaming,
            error: None,
        }
    } else {
        StreamView::default()
    };

    rsx! {
        SqlConsole {
            tabs,
            active: 0,
            schema,
            running: state == "running",
            stream,
            error: (state == "error")
                .then(|| "Catalog Error: Table with name saless does not exist!".to_string()),
            on_intent: |_| {},
            on_select_tab: |_| {},
        }
    }
}

#[component]
fn EmptyScene(first_run_done: bool, booting: bool) -> Element {
    // Fixed literal paths. `components/empty_state.rs` only displays them, so
    // they need not exist, and a real temp path would change every run.
    let recents = if first_run_done {
        vec![
            recent("/data/sales.csv"),
            recent("/data/trips.parquet"),
            recent("/data/crm.db"),
            recent("/data/q3-review.dat0"),
        ]
    } else {
        vec![]
    };
    rsx! {
        crate::components::empty_state::EmptyState {
            recents,
            first_run_done,
            booting,
            on_open_sample: |_| {},
            on_open_recent: |_| {},
            on_open_file: |_| {},
            on_take_tour: |_| {},
            on_open_demo: |_| {},
        }
    }
}

#[component]
fn PipelineScene(cursor: usize) -> Element {
    rsx! {
        crate::components::pipeline_bar::PipelineBar {
            stack: pipeline_stack(),
            cursor,
            source: Some("/data/sales.csv".to_string()),
            on_jump: |_| {},
            on_remove: |_| {},
            on_save_as_table: |_| {},
        }
    }
}

#[component]
fn InspectorScene(fx: Handle, mode: String) -> Element {
    let state = InspectorState::use_new();
    {
        let fx = fx.clone();
        let mode = mode.clone();
        use_hook(move || {
            if mode == "empty" {
                return;
            }
            state.set_target("sales".into());
            let id = state.begin_load();
            state.put_profile(id, fx.profile.clone());
            if mode == "lineage" {
                state.set_lineage(lineage());
            }
        });
    }

    // The right column, at the width the shell mounts it at. Both panes are
    // column surfaces; measuring one across the full 1440px frame is not a
    // shape the app has, and for `Charts` it is actively wrong — the SVG keeps
    // its 520x360 aspect, so a full-width pane is 1068px tall and overflows a
    // 900px window.
    rsx! {
        div { class: "d0-right", style: "width: {crate::state::RIGHT_WIDTH}px",
            Inspector {
                state,
                projection: None,
                focus_column: (mode != "empty").then(|| "revenue".to_string()),
                on_open: |_| {},
                on_reload: |_| {},
            }
        }
    }
}

#[component]
fn ChartsScene(fx: Handle, mode: String) -> Element {
    let theme = Theme::use_current();
    // The SVG is what the shell's `chart_data` resource would have produced.
    // Passed already-rendered rather than adding a prop to production code:
    // a resource does not run under SSR.
    let render = match mode.as_str() {
        "rendered" => ChartRender::Svg(render_chart(&fx.spec, &fx.plot, &theme.tokens())),
        "error" => ChartRender::Error(
            "Binder Error: Referenced column \"revenu\" not found in FROM clause".into(),
        ),
        _ => ChartRender::Empty,
    };
    let bound = mode != "empty";
    let state = use_signal(move || ChartLoad::ready(render.clone()));

    rsx! {
        div { class: "d0-right", style: "width: {crate::state::RIGHT_WIDTH}px",
            Charts {
                spec: if bound { fx.spec.clone() } else { empty_spec() },
                columns: if bound { fx.chart_columns.clone() } else { vec![] },
                source: bound.then(|| "\"sales\"".to_string()),
                state,
                on_config: |_| {},
                on_save: |_| {},
            }
        }
    }
}

// ── Scene data ───────────────────────────────────────────────────────────────

fn widths(fx: &Handle) -> Vec<f64> {
    vec![COL_W_DEFAULT; fx.columns.len()]
}

fn recent(path: &str) -> dat0_core::recents::RecentEntry {
    let p = PathBuf::from(path);
    if path.ends_with(".dat0") {
        dat0_core::recents::RecentEntry::Package { path: p }
    } else {
        dat0_core::recents::RecentEntry::Workspace { path: p }
    }
}

fn banners(id: &str) -> Vec<dat0_core::error_ux::Banner> {
    use dat0_core::error_ux::Banner;
    let info = Banner::info("sales.csv imported · 12 rows");
    let warning = {
        let mut b = Banner::warning("two sessions left behind by a previous run");
        b.dismissible = true;
        b
    };
    let error = {
        let mut b = Banner::error(
            "the query failed",
            "Catalog Error: Table with name saless does not exist!",
        );
        b.primary = Some(dat0_core::error_ux::banner::BannerAction {
            label: "open the console".into(),
            action_id: "sql.console.toggle".into(),
        });
        b
    };
    match id {
        "banner/info" => vec![info],
        "banner/warning" => vec![warning],
        "banner/error" => vec![error],
        _ => vec![info, warning, error],
    }
}

/// A three-step stack: filter, sort, rename. One of each family the chip
/// labeller has a branch for.
fn pipeline_stack() -> Vec<Transformation> {
    vec![
        Transformation::Filter {
            column: "revenue".into(),
            op: FilterOp::Gt,
            value: FilterValue::Scalar {
                value: Scalar::Float(800.0),
            },
        },
        Transformation::Sort {
            keys: vec![SortKey {
                column: "region".into(),
                direction: SortDirection::Asc,
            }],
        },
        Transformation::Rename {
            column: "revenue".into(),
            to: "gross".into(),
        },
    ]
}

/// A four-deep chain with one node past the depth-6 indent clamp.
fn lineage() -> dat0_core::inspector::lineage::LineageChain {
    use dat0_core::inspector::lineage::{ChainStep, EdgeKind, LineageChain, NodeKind};
    let step = |label: &str, depth: u32, kind: NodeKind, edge: EdgeKind, open: bool| ChainStep {
        label: label.into(),
        kind,
        edge,
        depth,
        open_name: open.then(|| label.to_string()),
    };
    LineageChain {
        ancestors: vec![
            step("sales.csv", 1, NodeKind::File, EdgeKind::FileImport, false),
            step(
                "sales_clean",
                2,
                NodeKind::Table,
                EdgeKind::Transform(3),
                true,
            ),
        ],
        descendants: vec![
            step(
                "revenue_by_region",
                1,
                NodeKind::Table,
                EdgeKind::SqlRef,
                true,
            ),
            step("revenue_chart", 2, NodeKind::Chart, EdgeKind::Chart, true),
            // Past the depth-6 clamp, so the indent stops growing.
            step("deep_rollup", 8, NodeKind::Table, EdgeKind::SqlRef, true),
        ],
    }
}

/// A chart with nothing bound. `ChartSpec` has no `Default` — a chart with no
/// source is not a chart — so the empty scene builds the same shape the shell
/// holds until a table is bound.
fn empty_spec() -> dat0_core::charts::spec::ChartSpec {
    dat0_core::charts::spec::ChartSpec {
        chart_type: dat0_core::charts::spec::ChartType::Bar,
        source: String::new(),
        x: None,
        y: None,
        group: None,
        color: None,
        title: String::new(),
    }
}

/// The wizard sitting on the Columns step with one column that fails
/// validation — an empty target name, which `WizardModel::issues` rejects.
fn wizard_model() -> WizardModel {
    use crate::components::import_wizard::ColumnDraft;
    let mut cols = vec![
        ColumnDraft::new("id", "BIGINT"),
        ColumnDraft::new("region", "VARCHAR"),
        ColumnDraft::new("revenue", "DOUBLE"),
        ColumnDraft::new("active", "BOOLEAN"),
    ];
    cols[2].name = String::new();
    WizardModel {
        path: PathBuf::from("/data/sales.csv"),
        delimiter: ",".into(),
        quote: "\"".into(),
        has_header: true,
        encoding_supported: true,
        columns: cols,
        step: Step::Columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A duplicate id would silently overwrite a snapshot and an HTML page, so
    /// two scenes would share one baseline and one of them would be untested
    /// with nothing to show for it.
    #[test]
    fn scene_ids_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for scene in SCENES {
            assert!(seen.insert(scene.id), "duplicate scene id {}", scene.id);
        }
    }

    /// `stem` is what becomes a file name and a snapshot name. Two ids that
    /// differ only where `/` becomes `__` would collide there even though
    /// `scene_ids_are_unique` passed.
    #[test]
    fn scene_stems_are_unique() {
        let mut seen = std::collections::BTreeMap::new();
        for scene in SCENES {
            if let Some(other) = seen.insert(scene.stem(), scene.id) {
                panic!("{} and {} share the stem {}", other, scene.id, scene.stem());
            }
        }
    }

    /// Every scene is addressable by the id it declares — the lookup both tiers
    /// go through.
    #[test]
    fn every_scene_resolves() {
        for s in SCENES {
            assert_eq!(scene(s.id).map(|f| f.id), Some(s.id));
        }
    }
}
