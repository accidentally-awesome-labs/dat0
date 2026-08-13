//! The column inspector (5.5).
//!
//! A direct port of `crates/dat0-app/src/inspector/panel.rs`: the same overview
//! line, the same Whole-table ⇄ Current-view toggle, the same lineage chain, the
//! same projection-aware column cards with their Hidden section, and the same
//! inline mini-charts. Only the chrome is re-cut — the panel now wears a
//! [`Pane`], and the mini-charts are inline SVG instead of GPUI quads.
//!
//! # Three things here are load-bearing and easy to "clean up" wrongly
//!
//! 1. **The lineage chain is flat.** `LineageChain` is already ordered
//!    root→parent and child→leaf, and every row draws its own indent from
//!    `step.depth`. There is no tree widget and no recursion, and the indent is
//!    clamped at depth 6 — a 20-deep chain would otherwise indent itself off the
//!    320px panel and become unreadable.
//! 2. **The supersede counter is the only thing standing between a slow profile
//!    and a wrong panel.** [`InspectorState::put_profile`] refuses any write
//!    whose `load_id` is not the newest one handed out by
//!    [`InspectorState::begin_load`]. Drop the guard and switching tables fast
//!    paints table A's columns under table B's name.
//! 3. **Extras are keyed by the *source* column name, never the display label.**
//!    A renamed column still profiles under its base name; keying the chart by
//!    the label loses it the moment someone renames a column.

use dioxus::prelude::*;

use dat0_core::charts::{Bin, bar_fraction};
use dat0_core::inspector::format;
use dat0_core::inspector::lineage::{ChainStep, EdgeKind, LineageChain, NodeKind};
use dat0_core::inspector::model::ColumnExtra;
use dat0_core::inspector::projection::{ProjectionContext, RenderCard, project_cards};
use dat0_core::inspector::{InspectorModel, ProfileTargetMode};
use dat0_engine::{ColumnProfile, TableProfile};

use crate::a11y::AccessRole;
use crate::components::pane::Pane;
use crate::state::Workspace;

/// The deepest indent a lineage row may draw, in steps of
/// [`LINEAGE_INDENT_PX`]. Ported verbatim from `panel.rs`'s `depth.min(6)`.
const LINEAGE_MAX_DEPTH: u32 = 6;
/// Pixels of indent per depth step.
const LINEAGE_INDENT_PX: u32 = 12;

// ── State ────────────────────────────────────────────────────────────────────

/// The Inspector's live state: [`InspectorModel`] behind a signal.
///
/// Held by the shell and passed down, so the async profile load (5.9) and the
/// panel write through the *same* supersede counter the GPUI `WorkspaceShell`
/// used. Every method takes `&self` — the handle is `Copy`, like the `Signal`
/// it wraps, so an event closure needs no `mut` binding and no clone.
#[derive(Clone, Copy, PartialEq)]
pub struct InspectorState {
    model: Signal<InspectorModel>,
}

impl InspectorState {
    /// Create the state. Must be called inside a component — it is a hook.
    pub fn use_new() -> Self {
        Self {
            model: use_signal(InspectorModel::new),
        }
    }

    /// Read the model. The escape hatch for anything this surface does not
    /// expose; the panel itself reads the signal directly.
    pub fn with<R>(&self, f: impl FnOnce(&InspectorModel) -> R) -> R {
        f(&self.model.read())
    }

    /// Point the Inspector at `table`.
    ///
    /// Does **not** start a load: whether one is needed depends on the
    /// (table, epoch) cache, which the caller checks with
    /// [`Self::has_profile`] — the same warm-hit skip as
    /// `WorkspaceShell::set_inspector_target`.
    pub fn set_target(&self, table: String) {
        let mut model = self.model;
        model.write().set_target(table);
    }

    /// Is a profile already cached for the current target and epoch?
    pub fn has_profile(&self) -> bool {
        self.model.read().cached().is_some()
    }

    /// Claim the next load id. Every later write must present it.
    pub fn begin_load(&self) -> u64 {
        let mut model = self.model;
        model.write().begin_load()
    }

    /// Store a profile, **if** `load_id` is still the newest load.
    ///
    /// Returns whether the write landed, so a caller that chains extras off a
    /// profile (as `load_column_extras` does) can skip them when it is stale.
    pub fn put_profile(&self, load_id: u64, profile: TableProfile) -> bool {
        let mut model = self.model;
        let mut m = model.write();
        if !m.is_current(load_id) {
            return false;
        }
        m.put(profile);
        true
    }

    /// Store top-N bars for one column, under the same guard.
    pub fn put_topn(&self, load_id: u64, col: &str, data: Vec<(String, u64)>) -> bool {
        let mut model = self.model;
        let mut m = model.write();
        if !m.is_current(load_id) {
            return false;
        }
        m.put_topn(col, data);
        true
    }

    /// Store histogram bins for one column, under the same guard.
    pub fn put_histogram(&self, load_id: u64, col: &str, bins: Vec<Bin>) -> bool {
        let mut model = self.model;
        let mut m = model.write();
        if !m.is_current(load_id) {
            return false;
        }
        m.put_histogram(col, bins);
        true
    }

    /// Invalidate the cached profile for `table` (a write happened).
    pub fn bump_epoch(&self, table: &str) {
        let mut model = self.model;
        model.write().bump_epoch(table);
    }

    /// Replace the lineage chain.
    pub fn set_lineage(&self, chain: LineageChain) {
        let mut model = self.model;
        model.write().set_lineage(chain);
    }

    /// Flip Whole-table ⇄ Current-view and drop the per-column extras.
    ///
    /// Extras query the *base* table by name, so a WholeTable bar is simply
    /// wrong beside a CurrentView (filtered) profile. The caller re-profiles;
    /// the cache is keyed by (table, epoch) and not by mode, so the reload is
    /// unconditional — exactly `WorkspaceShell::toggle_inspector_mode`.
    pub fn toggle_mode(&self) {
        let mut model = self.model;
        let mut m = model.write();
        m.mode = match m.mode {
            ProfileTargetMode::WholeTable => ProfileTargetMode::CurrentView,
            ProfileTargetMode::CurrentView => ProfileTargetMode::WholeTable,
        };
        m.clear_extras();
    }

    /// Expand or collapse the Hidden-columns section.
    pub fn toggle_hidden(&self) {
        let mut model = self.model;
        model.write().toggle_hidden();
    }
}

// ── The panel ────────────────────────────────────────────────────────────────

/// Everything the Inspector needs from the shell.
#[derive(Clone, Props)]
pub struct InspectorProps {
    pub state: InspectorState,
    /// The active grid tab's column projection, supplied **only** when the
    /// Inspector targets that tab's table. `None` means "no projection" and
    /// yields the raw profile order with nothing hidden.
    #[props(default)]
    pub projection: Option<ProjectionContext>,
    /// The column the pane header names in its `{column} · {type}` meta: the
    /// grid's active column. A prop rather than derived state, because the
    /// Inspector cannot see the grid's cursor and inventing one here would give
    /// the app two disagreeing notions of "the current column".
    #[props(default)]
    pub focus_column: Option<String>,
    /// A lineage node was clicked. Charts reopen through `open_saved_chart`,
    /// everything else opens a table tab — the shell routes on the kind.
    #[props(default)]
    pub on_open: EventHandler<(NodeKind, String)>,
    /// The mode toggle flipped; re-profile the target.
    #[props(default)]
    pub on_reload: EventHandler<()>,
}

impl PartialEq for InspectorProps {
    fn eq(&self, other: &Self) -> bool {
        // `ProjectionContext` has no `PartialEq` (it is a plain data carrier in
        // `dat0-core` and nothing there compares one), so compare its fields —
        // both of which are `PartialEq` — rather than adding a derive to a
        // crate the GPUI build still shares.
        let projection = match (&self.projection, &other.projection) {
            (None, None) => true,
            (Some(a), Some(b)) => a.visible == b.visible && a.base_sources == b.base_sources,
            _ => false,
        };
        self.state == other.state && projection && self.focus_column == other.focus_column
    }
}

#[component]
pub fn Inspector(props: InspectorProps) -> Element {
    let mut ws = Workspace::use_current();
    let open = ws.layout.read().inspector_visible;

    let state = props.state;
    // One read guard for the whole render. Safe because nothing below mutates
    // the model: every mutator sits inside an event closure, which cannot run
    // while this function is on the stack.
    let model = state.model.read();

    let profile = model.cached();
    let target = model.target_table.clone();

    // Overview line: `name — N rows · M cols` warm, `name — Profiling…` while a
    // load is in flight, the empty-state string with no target at all.
    let overview = match (&target, profile) {
        (Some(name), Some(p)) => format!("{} — {} rows · {} cols", name, p.rows, p.columns.len()),
        (Some(name), None) => format!("{} — {}", name, dat0_i18n::t("inspector.loading")),
        (None, _) => dat0_i18n::t("inspector.empty"),
    };

    let mode_label = match model.mode {
        ProfileTargetMode::WholeTable => dat0_i18n::t("inspector.mode.whole"),
        ProfileTargetMode::CurrentView => dat0_i18n::t("inspector.mode.view"),
    };

    let meta = header_meta(
        profile,
        props.projection.as_ref(),
        props.focus_column.as_deref(),
    );

    // Paired, not two independent `Option`s: cards exist exactly when a
    // profile does, and pairing them keeps the render from having to assert it.
    let cards = profile.map(|p| (p, project_cards(&p.columns, props.projection.as_ref())));
    let hidden_count = cards.as_ref().map(|(_, c)| c.hidden.len()).unwrap_or(0);
    let hidden_header = format!("{} ({})", dat0_i18n::t("inspector.hidden"), hidden_count);
    let hidden_expanded = model.hidden_expanded;

    let on_open = props.on_open;
    let on_reload = props.on_reload;

    rsx! {
        Pane {
            id: "inspector".to_string(),
            title: dat0_i18n::t("inspector.title"),
            meta,
            open,
            on_toggle: move |_| {
                let now = ws.layout.read().inspector_visible;
                ws.layout.write().inspector_visible = !now;
            },

            div { class: "d0-insp", "data-a11y-id": "inspector",

                div {
                    class: "d0-insp-overview d0-mono",
                    "data-a11y-id": "inspector-overview",
                    role: AccessRole::Label.aria(),
                    "aria-label": "{overview}",
                    "{overview}"
                }

                button {
                    class: "d0-btn d0-mono",
                    "data-a11y-id": "inspector-mode-toggle",
                    role: AccessRole::Button.aria(),
                    "aria-label": "{mode_label}",
                    onclick: move |_| {
                        state.toggle_mode();
                        on_reload.call(());
                    },
                    "{mode_label}"
                }

                if let Some(target) = target {
                    div { class: "d0-insp-lineage", "data-a11y-id": "inspector-lineage",

                        Note { id: "inspector-lineage-title", text: dat0_i18n::t("inspector.lineage"), class: "d0-label" }

                        if !model.lineage.ancestors.is_empty() {
                            Note {
                                id: "inspector-lineage-sources",
                                text: dat0_i18n::t("inspector.lineage.sources"),
                                class: "d0-label",
                            }
                            for step in model.lineage.ancestors.iter() {
                                ChainRow { key: "anc-{step.depth}-{step.label}", step: step.clone(), on_open }
                            }
                        }

                        // The inspected table itself: highlighted, never
                        // clickable — clicking it would re-root the Inspector
                        // onto the table it is already showing.
                        div {
                            class: "d0-insp-target d0-mono",
                            "data-a11y-id": "inspector-target",
                            role: AccessRole::Label.aria(),
                            "aria-label": "{target}",
                            span { class: "d0-chevron is-collapsed", "aria-hidden": "true", "▾" }
                            "{target}"
                        }

                        Note {
                            id: "inspector-lineage-usedby",
                            text: if model.lineage.descendants.is_empty() {
                                dat0_i18n::t("inspector.lineage.none")
                            } else {
                                dat0_i18n::t("inspector.lineage.usedby")
                            },
                            class: "d0-label",
                        }
                        for step in model.lineage.descendants.iter() {
                            ChainRow { key: "desc-{step.depth}-{step.label}", step: step.clone(), on_open }
                        }
                    }
                }

                if let Some((p, cards)) = cards.as_ref() {
                    div { class: "d0-insp-cards", "data-a11y-id": "inspector-cards",
                        for card in cards.visible.iter() {
                            if let Some(col) = p.columns.iter().find(|c| c.name == card.source) {
                                ColumnCard {
                                    key: "{card.source}",
                                    card: card.clone(),
                                    col: col.clone(),
                                    dimmed: false,
                                    chart: chart_svg(model.extra(&col.name)),
                                }
                            }
                        }
                    }

                    if !cards.hidden.is_empty() {
                        div { class: "d0-insp-hidden", "data-a11y-id": "inspector-hidden",
                            button {
                                class: "d0-insp-hidden-head d0-mono",
                                "data-a11y-id": "inspector-hidden-toggle",
                                role: AccessRole::Button.aria(),
                                // The GPUI original left this control unnamed
                                // and called naming it "real a11y work for its
                                // own slice". In the DOM the name is one
                                // attribute, so it ships.
                                "aria-label": "{hidden_header}",
                                "aria-expanded": if hidden_expanded { "true" } else { "false" },
                                onclick: move |_| state.toggle_hidden(),
                                span {
                                    class: if hidden_expanded { "d0-chevron" } else { "d0-chevron is-collapsed" },
                                    "aria-hidden": "true",
                                    "▾"
                                }
                                "{hidden_header}"
                            }
                            if hidden_expanded {
                                for card in cards.hidden.iter() {
                                    if let Some(col) = p.columns.iter().find(|c| c.name == card.source) {
                                        ColumnCard {
                                            key: "hidden-{card.source}",
                                            card: card.clone(),
                                            col: col.clone(),
                                            dimmed: true,
                                            chart: chart_svg(model.extra(&col.name)),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A content-only locator: text a screen reader announces and the harness
/// queries, with no interaction of its own.
#[component]
fn Note(id: String, text: String, class: String) -> Element {
    rsx! {
        div {
            class: "{class}",
            "data-a11y-id": "{id}",
            role: AccessRole::Label.aria(),
            "aria-label": "{text}",
            "{text}"
        }
    }
}

/// One lineage row: a kind glyph, the node label, the edge label — indented by
/// depth, clickable when the node maps to something openable.
#[component]
fn ChainRow(step: ChainStep, on_open: EventHandler<(NodeKind, String)>) -> Element {
    let glyph = match step.kind {
        NodeKind::File => "📄",
        NodeKind::External => "☁",
        NodeKind::Table => "▦",
        NodeKind::Chart => "📊",
    };
    // Clamped: a deep chain must indent to show structure, not walk itself off
    // the right edge of a 320px dock.
    let indent = step.depth.min(LINEAGE_MAX_DEPTH) * LINEAGE_INDENT_PX;
    let id = format!("lineage-{}-{}", step.depth, step.label);
    let edge = edge_label(&step.edge);
    let open = step.open_name.clone();
    let kind = step.kind;

    rsx! {
        div {
            class: if open.is_some() { "d0-insp-chain is-open" } else { "d0-insp-chain" },
            "data-a11y-id": "{id}",
            style: "padding-left: {indent}px",
            // The accessible name is the bare node label, not the composed
            // glyph/edge text: the glyph is decoration and the edge is already
            // announced as its own text run.
            role: AccessRole::Label.aria(),
            "aria-label": "{step.label}",
            onclick: move |_| {
                if let Some(name) = open.clone() {
                    on_open.call((kind, name));
                }
            },
            span { class: "d0-insp-glyph", "aria-hidden": "true", "{glyph}" }
            span { class: "d0-insp-chain-name d0-mono", "{step.label}" }
            span { class: "d0-insp-chain-edge d0-label", "{edge}" }
        }
    }
}

/// One column card: the projected header, the stat lines the original renders,
/// and the inline mini-chart once its lazy data has landed.
#[component]
fn ColumnCard(
    card: RenderCard,
    col: ColumnProfile,
    dimmed: bool,
    chart: Option<String>,
) -> Element {
    let header = match &card.original {
        Some(orig) => format!(
            "{} · {}  ·  {} {}",
            card.label,
            col.ty,
            dat0_i18n::t("inspector.col.was"),
            orig
        ),
        None => format!("{} · {}", card.label, col.ty),
    };
    let stats = format::format_stats_line(&col);
    let distinct = format::format_distinct(&col);
    let null = format::format_null(&col);

    rsx! {
        div {
            class: if dimmed { "d0-insp-card is-dimmed" } else { "d0-insp-card" },
            "data-a11y-id": "inspector-card-{card.source}",

            Note { id: "inspector-card-head-{card.source}", text: header, class: "d0-insp-card-head d0-mono" }

            // `format_stats_line` is empty for a column with neither numeric
            // nor length stats (booleans, all-null); the original skips the
            // line rather than rendering a blank one.
            if !stats.is_empty() {
                Note { id: "inspector-stats-{card.source}", text: stats, class: "d0-insp-stat d0-mono" }
            }
            Note { id: "inspector-distinct-{card.source}", text: distinct, class: "d0-insp-stat d0-mono" }
            Note { id: "inspector-null-{card.source}", text: null, class: "d0-insp-stat d0-mono" }

            if let Some(svg) = chart {
                div {
                    class: "d0-mini-wrap",
                    "data-a11y-id": "inspector-chart-{card.source}",
                    role: "img",
                    "aria-label": "{card.label}",
                    dangerous_inner_html: "{svg}",
                }
            }
        }
    }
}

// ── Pure helpers ─────────────────────────────────────────────────────────────

/// A short, human-readable label for a lineage edge.
pub fn edge_label(edge: &EdgeKind) -> String {
    match edge {
        EdgeKind::FileImport => dat0_i18n::t("inspector.edge.file"),
        EdgeKind::SqlRef => dat0_i18n::t("inspector.edge.sql"),
        EdgeKind::Chart => dat0_i18n::t("inspector.edge.chart"),
        EdgeKind::Transform(n) => format!("{} ({n} ops)", dat0_i18n::t("inspector.edge.transform")),
    }
}

/// The pane header's right-aligned meta: `{column} · {type}` (S4).
///
/// `focus` names the grid's active column by either its base name or its
/// displayed label, because the grid knows the label and the profile knows the
/// base name and the Inspector is where the two meet. Empty when there is no
/// profile, no focus column, or the focus column is not in this table — an
/// empty meta is the honest answer, and a stale one is worse than none.
pub fn header_meta(
    profile: Option<&TableProfile>,
    projection: Option<&ProjectionContext>,
    focus: Option<&str>,
) -> String {
    let (Some(profile), Some(focus)) = (profile, focus) else {
        return String::new();
    };
    let cards = project_cards(&profile.columns, projection);
    let card = cards
        .visible
        .iter()
        .chain(cards.hidden.iter())
        .find(|c| c.source == focus || c.label == focus);
    let (source, label) = match card {
        Some(c) => (c.source.as_str(), c.label.as_str()),
        // Not in the projection (the surrogate, or a column the grid dropped):
        // fall back to the raw profile so the header still names a real type.
        None => (focus, focus),
    };
    match profile.columns.iter().find(|c| c.name == source) {
        Some(col) => format!("{} · {}", label, col.ty),
        None => String::new(),
    }
}

/// The inline chart for a column, if its lazy data has landed.
///
/// Top-N wins over the histogram, matching the original's `if/else if`: a
/// low-cardinality column gets bars, and only a high-cardinality numeric one
/// gets a histogram, so the two are never both meaningful.
fn chart_svg(extra: Option<&ColumnExtra>) -> Option<String> {
    let extra = extra?;
    if let Some(topn) = &extra.topn {
        return Some(render_topn(topn));
    }
    extra.histogram.as_ref().map(|bins| render_histogram(bins))
}

/// Histogram as a row of vertical bars, each scaled to the tallest bin.
///
/// Inline SVG, not a raster: the GPUI panel painted quads and the charts pane
/// painted a BGRA buffer, both of which needed a re-render on every theme
/// change. An SVG whose bars take their fill from a CSS class re-colours with
/// the theme for free, and stays crisp on a HiDPI display.
pub fn render_histogram(bins: &[Bin]) -> String {
    const BAR_W: u32 = 9;
    const GAP: u32 = 1;
    const H: u32 = 28;
    const FLOOR: f64 = 4.0;

    let max = bins.iter().map(|b| b.count).max().unwrap_or(0);
    let w = (bins.len() as u32 * (BAR_W + GAP))
        .saturating_sub(GAP)
        .max(1);
    let mut s = String::with_capacity(64 + bins.len() * 80);
    s.push_str(&format!(
        r#"<svg class="d0-mini" width="{w}" height="{H}" viewBox="0 0 {w} {H}">"#
    ));
    for (i, b) in bins.iter().enumerate() {
        // A zero bin still draws the 4px floor, so an empty bucket reads as
        // "measured and empty" rather than "not measured".
        let h = FLOOR + bar_fraction(b.count, max) * (H as f64 - FLOOR);
        let x = i as u32 * (BAR_W + GAP);
        let y = H as f64 - h;
        s.push_str(&format!(
            r#"<rect class="d0-mini-bar" x="{x}" y="{y:.1}" width="{BAR_W}" height="{h:.1}" rx="1"/>"#
        ));
    }
    s.push_str("</svg>");
    s
}

/// Horizontal top-N bars: `(label, count)` scaled to the largest count.
pub fn render_topn(items: &[(String, u64)]) -> String {
    const ROW_H: u32 = 14;
    const LABEL_W: u32 = 70;
    const BAR_X: u32 = 78;
    const BAR_H: u32 = 8;
    const BAR_MIN: f64 = 6.0;
    const BAR_MAX: f64 = 80.0;

    let max = items.iter().map(|(_, c)| *c).max().unwrap_or(0);
    let w = BAR_X + BAR_MAX as u32 + 6;
    let h = (items.len() as u32 * ROW_H).max(1);
    let mut s = String::with_capacity(64 + items.len() * 140);
    s.push_str(&format!(
        r#"<svg class="d0-mini" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
    ));
    for (i, (label, count)) in items.iter().enumerate() {
        let y = i as u32 * ROW_H;
        let bw = BAR_MIN + bar_fraction(*count, max) * BAR_MAX;
        s.push_str(&format!(
            r#"<text class="d0-mini-label" x="0" y="{ty}" textLength="{LABEL_W}" lengthAdjust="spacingAndGlyphs">{label}</text>"#,
            ty = y + ROW_H - 4,
            // Labels are *data* — column values straight out of DuckDB. This
            // string is handed to `dangerous_inner_html`, so an unescaped `<`
            // in a value would be markup.
            label = escape_xml(label),
        ));
        s.push_str(&format!(
            r#"<rect class="d0-mini-bar-topn" x="{BAR_X}" y="{by}" width="{bw:.1}" height="{BAR_H}" rx="2"/>"#,
            by = y + (ROW_H - BAR_H) / 2,
        ));
    }
    s.push_str("</svg>");
    s
}

/// Escape the five XML metacharacters.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Indent for a lineage row at `depth`, in pixels. Exposed for the test that
/// pins the clamp.
pub fn lineage_indent(depth: u32) -> u32 {
    depth.min(LINEAGE_MAX_DEPTH) * LINEAGE_INDENT_PX
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{NumericStats, transform::ProjectionColumn};

    fn col(name: &str, ty: &str) -> ColumnProfile {
        ColumnProfile {
            name: name.into(),
            ty: ty.into(),
            null_pct: 0.0,
            approx_distinct: 3,
            count: 3,
            numeric: None,
            length: None,
        }
    }

    #[test]
    fn edge_labels_are_human_readable() {
        assert_eq!(
            edge_label(&EdgeKind::FileImport),
            dat0_i18n::t("inspector.edge.file")
        );
        assert_eq!(
            edge_label(&EdgeKind::Transform(2)),
            format!("{} (2 ops)", dat0_i18n::t("inspector.edge.transform"))
        );
    }

    #[test]
    fn the_indent_stops_growing_at_depth_six() {
        assert_eq!(lineage_indent(0), 0);
        assert_eq!(lineage_indent(3), 36);
        assert_eq!(lineage_indent(6), 72);
        // A 20-deep chain must not walk off the panel.
        assert_eq!(lineage_indent(7), 72);
        assert_eq!(lineage_indent(200), 72);
    }

    #[test]
    fn header_meta_names_the_focus_column_and_its_type() {
        let p = TableProfile {
            rows: 3,
            columns: vec![col("id", "BIGINT"), col("price", "DOUBLE")],
        };
        assert_eq!(header_meta(Some(&p), None, Some("price")), "price · DOUBLE");
        // No focus, no profile, or an unknown column: empty, never stale.
        assert_eq!(header_meta(Some(&p), None, None), "");
        assert_eq!(header_meta(None, None, Some("price")), "");
        assert_eq!(header_meta(Some(&p), None, Some("nope")), "");
    }

    #[test]
    fn header_meta_uses_the_renamed_label_but_the_base_column_s_type() {
        let p = TableProfile {
            rows: 3,
            columns: vec![col("price", "DOUBLE")],
        };
        let ctx = ProjectionContext {
            visible: vec![ProjectionColumn {
                source: "price".into(),
                display: "Unit price".into(),
            }],
            base_sources: vec!["price".into()],
        };
        // Addressed by either name, announced by the one the user sees.
        assert_eq!(
            header_meta(Some(&p), Some(&ctx), Some("price")),
            "Unit price · DOUBLE"
        );
        assert_eq!(
            header_meta(Some(&p), Some(&ctx), Some("Unit price")),
            "Unit price · DOUBLE"
        );
    }

    #[test]
    fn a_histogram_draws_one_bar_per_bin_with_a_floor_for_empty_ones() {
        let bins = vec![
            Bin {
                lo: 0.0,
                hi: 1.0,
                count: 0,
            },
            Bin {
                lo: 1.0,
                hi: 2.0,
                count: 4,
            },
        ];
        let svg = render_histogram(&bins);
        assert_eq!(svg.matches("<rect").count(), 2);
        // Empty bucket keeps the 4px floor; the tallest fills the 28px box.
        assert!(svg.contains(r#"height="4.0""#), "{svg}");
        assert!(svg.contains(r#"height="28.0""#), "{svg}");
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn topn_labels_are_escaped_because_they_are_data() {
        let svg = render_topn(&[("<script>".into(), 2), ("a&b".into(), 1)]);
        assert!(svg.contains("&lt;script&gt;"), "{svg}");
        assert!(svg.contains("a&amp;b"), "{svg}");
        assert!(
            !svg.contains("<script>"),
            "raw markup reached the DOM: {svg}"
        );
    }

    #[test]
    fn topn_beats_the_histogram_when_both_landed() {
        let extra = ColumnExtra {
            topn: Some(vec![("a".into(), 1)]),
            histogram: Some(vec![Bin {
                lo: 0.0,
                hi: 1.0,
                count: 1,
            }]),
        };
        let svg = chart_svg(Some(&extra)).unwrap();
        assert!(svg.contains("d0-mini-bar-topn"), "{svg}");
        assert!(chart_svg(None).is_none());
        assert!(chart_svg(Some(&ColumnExtra::default())).is_none());
    }

    #[test]
    fn a_numeric_column_still_renders_its_stat_line() {
        // Guards the `if !stats.is_empty()` gate from being inverted: a numeric
        // column must have a line, a boolean must not.
        let mut numeric = col("amount", "DOUBLE");
        numeric.numeric = Some(NumericStats {
            min: 0.0,
            max: 10.0,
            avg: 5.0,
            std: 1.0,
            q25: 2.0,
            median: 5.0,
            q75: 8.0,
        });
        assert!(!format::format_stats_line(&numeric).is_empty());
        assert!(format::format_stats_line(&col("flag", "BOOLEAN")).is_empty());
    }
}
