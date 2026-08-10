//! Pipeline bar, empty-state hero, banners and the filter popover.
//!
//! Each surface is driven through the headless harness the way a user drives it
//! — a click, a keystroke, a typed value — and asserted on what that changed.
//! The rules under test are the ones the GPUI originals encoded and that a
//! restyle is most likely to quietly drop: the pipeline cursor, the hero's
//! call-to-action wiring, banner severity and dismissal, and the filter
//! popover's type-driven operator list.

mod support;

use dioxus::prelude::*;
use support::{Harness, Key, Modifiers};

use dat0_core::error_ux::banner::Banner;
use dat0_core::recents::RecentEntry;
use dat0_core::sample_data::SampleKind;
use dat0_core::view::filter_popover::{ColumnType, Outcome};
use dat0_engine::transform::{SortDirection, SortKey, Transformation};
use dat0_engine::{FilterOp, FilterValue, Scalar};

use dat0_ui::components::banner::BannerHost;
use dat0_ui::components::empty_state::EmptyState;
use dat0_ui::components::filter_popover::FilterPopover;
use dat0_ui::components::pipeline_bar::PipelineBar;

/// A three-op stack: filter, sort, rename.
fn stack() -> Vec<Transformation> {
    vec![
        Transformation::Filter {
            column: "price".into(),
            op: FilterOp::Gt,
            value: FilterValue::Scalar {
                value: Scalar::Int(10),
            },
        },
        Transformation::Sort {
            keys: vec![SortKey {
                column: "qty".into(),
                direction: SortDirection::Asc,
            }],
        },
        Transformation::Rename {
            column: "qty".into(),
            to: "quantity".into(),
        },
    ]
}

// ── pipeline bar ────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct PipelineHostProps {
    stack: Vec<Transformation>,
    cursor: usize,
    source: Option<String>,
}

/// Hosts the bar and records what it emitted, because the harness reads the DOM,
/// not Rust state.
#[component]
fn PipelineHost(props: PipelineHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        PipelineBar {
            stack: props.stack.clone(),
            cursor: props.cursor,
            source: props.source.clone(),
            on_jump: move |k: usize| log.write().push(format!("jump {k}")),
            on_remove: move |i: usize| log.write().push(format!("remove {i}")),
            on_save_as_table: move |_| log.write().push("save".to_string()),
        }
        div { "data-a11y-id": "log", "{log.read().join(\",\")}" }
    }
}

fn pipeline(cursor: usize) -> Harness {
    Harness::new(
        PipelineHost,
        PipelineHostProps {
            stack: stack(),
            cursor,
            source: None,
        },
    )
}

fn log(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("log").expect("the host renders its log"))
}

#[test]
fn no_bar_until_a_transform_exists() {
    // A strip reading `base` on every freshly-opened table is chrome that
    // explains nothing.
    let h = Harness::new(
        PipelineHost,
        PipelineHostProps {
            stack: Vec::new(),
            cursor: 0,
            source: None,
        },
    );
    assert!(h.by_a11y_id("pipeline-bar").is_none());
}

#[test]
fn every_op_gets_a_chip_that_reads_as_the_transform() {
    let h = pipeline(3);
    assert_eq!(
        h.text_of(h.by_a11y_id("pipeline-chip-0").unwrap()),
        "Filter price"
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("pipeline-chip-1").unwrap()),
        "Sort qty↑"
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("pipeline-chip-2").unwrap()),
        "Rename qty→quantity"
    );
}

#[test]
fn clicking_a_chip_keeps_the_ops_up_to_and_including_it() {
    // The scrubber contract: chip `i` means "keep the first i+1 ops".
    let mut h = pipeline(3);
    h.click("pipeline-chip-1");
    assert_eq!(log(&h), "jump 2");
}

#[test]
fn the_base_chip_returns_to_the_unfiltered_table() {
    let mut h = pipeline(3);
    h.click("pipeline-base");
    assert_eq!(log(&h), "jump 0");
}

#[test]
fn chips_past_the_cursor_are_dimmed_and_still_clickable() {
    // Scrubbed-past ops stay on screen: pointing at where you came from is the
    // way back, and a bar that deleted them would leave only ⌘Z.
    let mut h = pipeline(1);
    let past = h.by_a11y_id("pipeline-chip-1").unwrap();
    assert!(
        h.attr(past, "class").unwrap().contains("is-past"),
        "an op past the cursor must read as inactive"
    );
    let active = h.by_a11y_id("pipeline-chip-0").unwrap();
    assert!(!h.attr(active, "class").unwrap().contains("is-past"));

    h.click("pipeline-chip-2");
    assert_eq!(log(&h), "jump 3", "a dimmed chip still scrubs to its op");
}

#[test]
fn the_cursor_decides_dimming_not_the_stack_length() {
    let h = pipeline(3);
    for i in 0..3 {
        let chip = h.by_a11y_id(&format!("pipeline-chip-{i}")).unwrap();
        assert!(!h.attr(chip, "class").unwrap().contains("is-past"));
    }
}

#[test]
fn expanding_the_bar_adds_a_remove_button_per_step() {
    let mut h = pipeline(3);
    assert!(
        h.by_a11y_id("pipeline-remove-0").is_none(),
        "the collapsed strip has no per-step remove"
    );

    h.click("pipeline-toggle");
    h.click("pipeline-remove-1");
    assert_eq!(log(&h), "remove 1");
}

#[test]
fn collapsing_returns_to_the_strip() {
    let mut h = pipeline(3);
    h.click("pipeline-toggle");
    assert!(h.by_a11y_id("pipeline-remove-0").is_some());
    h.click("pipeline-toggle");
    assert!(h.by_a11y_id("pipeline-remove-0").is_none());
}

#[test]
fn save_as_table_is_reachable_from_both_shapes() {
    let mut h = pipeline(3);
    h.click("pipeline-save-table");
    h.click("pipeline-toggle");
    h.click("pipeline-save-table");
    assert_eq!(log(&h), "save,save");
}

#[test]
fn a_file_backed_base_chip_carries_its_format_swatch() {
    // S8: the same 7×7 square marks a `.parquet` in the sidebar, in a tab title
    // and here, so the strip says which file it is filtering.
    let h = Harness::new(
        PipelineHost,
        PipelineHostProps {
            stack: stack(),
            cursor: 3,
            source: Some("events.parquet".to_string()),
        },
    );
    let base = h.by_a11y_id("pipeline-base").unwrap();
    assert!(h.text_of(base).contains("events.parquet"));
    assert!(
        h.html().contains("sw-parquet"),
        "the base chip must carry the parquet swatch"
    );
}

// ── empty-state hero ────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct HeroHostProps {
    recents: Vec<RecentEntry>,
    first_run_done: bool,
    booting: bool,
}

#[component]
fn HeroHost(props: HeroHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        EmptyState {
            recents: props.recents.clone(),
            first_run_done: props.first_run_done,
            booting: props.booting,
            on_open_sample: move |k: SampleKind| {
                let name = match k {
                    SampleKind::BundledCsv { dest_filename, .. } => dest_filename,
                    SampleKind::BundledSqlite { dest_filename, .. } => dest_filename,
                    SampleKind::Remote { dest_filename, .. } => dest_filename,
                };
                log.write().push(format!("sample {name}"));
            },
            on_open_recent: move |e: RecentEntry| {
                log.write().push(format!("recent {}", e.path().display()));
            },
            on_open_file: move |_| log.write().push("picker".to_string()),
            on_take_tour: move |_| log.write().push("tour".to_string()),
            on_open_demo: move |_| log.write().push("demo".to_string()),
        }
        div { "data-a11y-id": "log", "{log.read().join(\",\")}" }
    }
}

fn hero(recents: Vec<RecentEntry>, first_run_done: bool) -> Harness {
    Harness::new(
        HeroHost,
        HeroHostProps {
            recents,
            first_run_done,
            booting: false,
        },
    )
}

fn recents() -> Vec<RecentEntry> {
    vec![
        RecentEntry::Workspace {
            path: "/tmp/one.dat0".into(),
        },
        RecentEntry::Package {
            path: "/tmp/two.dat0".into(),
        },
    ]
}

#[test]
fn the_sample_picker_shows_only_when_there_are_no_recents() {
    let fresh = hero(Vec::new(), true);
    assert!(fresh.by_a11y_id("hero-samples").is_some());
    assert!(fresh.by_a11y_id("hero-recents").is_none());

    let returning = hero(recents(), true);
    assert!(returning.by_a11y_id("hero-recents").is_some());
    assert!(returning.by_a11y_id("hero-samples").is_none());
}

#[test]
fn each_sample_card_opens_its_own_dataset() {
    // The call to action of a first-ever launch: three cards, three datasets. A
    // shared handler here would make two of them dead.
    let mut h = hero(Vec::new(), true);
    h.click("hero-sample-iris");
    h.click("hero-sample-chinook");
    h.click("hero-sample-nyc-taxi");
    assert_eq!(
        log(&h),
        "sample iris.csv,sample chinook.sqlite,sample nyc_taxi.parquet"
    );
}

#[test]
fn open_file_is_reachable_from_either_column() {
    let mut fresh = hero(Vec::new(), true);
    fresh.click("hero-open-file-samples");
    assert_eq!(log(&fresh), "picker");

    let mut returning = hero(recents(), true);
    returning.click("hero-open-file-recents");
    assert_eq!(log(&returning), "picker");
}

#[test]
fn clicking_a_recent_opens_that_entry() {
    let mut h = hero(recents(), true);
    h.click("hero-recent-1");
    assert_eq!(log(&h), "recent /tmp/two.dat0");
}

#[test]
fn the_recents_list_is_one_tab_stop_and_the_arrows_move_within_it() {
    // Six recents as six tab stops is how a keyboard user ends up pressing Tab a
    // dozen times to reach the button after them.
    let mut h = hero(recents(), true);
    let list = h.by_a11y_id("recents-list").unwrap();
    assert_eq!(h.attr(list, "tabindex").as_deref(), Some("0"));
    for i in 0..recents().len() {
        let row = h.by_a11y_id(&format!("hero-recent-{i}")).unwrap();
        assert_eq!(h.attr(row, "tabindex").as_deref(), Some("-1"));
    }

    let active = |h: &Harness| {
        (0..2)
            .find(|i| {
                h.attr(h.by_a11y_id(&format!("hero-recent-{i}")).unwrap(), "class")
                    .unwrap()
                    .contains("is-active")
            })
            .expect("exactly one row is active")
    };
    assert_eq!(active(&h), 0);

    h.key(list, Key::ArrowDown, Modifiers::empty());
    assert_eq!(active(&h), 1);
    h.key(list, Key::ArrowDown, Modifiers::empty());
    assert_eq!(active(&h), 1, "the last row is the end of the list");
    h.key(list, Key::ArrowUp, Modifiers::empty());
    assert_eq!(active(&h), 0);
}

#[test]
fn enter_opens_the_active_recent_through_the_same_path_a_click_does() {
    let mut h = hero(recents(), true);
    let list = h.by_a11y_id("recents-list").unwrap();
    h.key(list, Key::ArrowDown, Modifiers::empty());
    h.key(list, Key::Enter, Modifiers::empty());
    assert_eq!(log(&h), "recent /tmp/two.dat0");
}

#[test]
fn the_first_run_band_carries_the_tour_and_the_demo_and_nothing_else_does() {
    let mut first = hero(Vec::new(), false);
    assert!(first.by_a11y_id("hero-take-tour").is_some());
    first.click("hero-take-tour");
    first.click("hero-open-demo");
    assert_eq!(log(&first), "tour,demo");

    let later = hero(Vec::new(), true);
    assert!(
        later.by_a11y_id("hero-take-tour").is_none(),
        "the band is first-run only"
    );
    assert!(later.by_a11y_id("hero-open-demo").is_none());
}

#[test]
fn the_product_statement_renders_in_both_modes() {
    for first_run_done in [true, false] {
        let h = hero(Vec::new(), first_run_done);
        assert!(
            h.has_label(&dat0_i18n::t("hero.title")),
            "the hero says what dat0 is in either mode"
        );
    }
}

#[test]
fn booting_swaps_the_drop_copy_but_keeps_the_privacy_claim() {
    // "No import wizard, no waiting" is false while the engine opens; "0 bytes
    // left this machine" is true in every state, and is the claim a user
    // watching a spinner most wants held.
    let h = Harness::new(
        HeroHost,
        HeroHostProps {
            recents: Vec::new(),
            first_run_done: true,
            booting: true,
        },
    );
    assert!(h.by_a11y_id("hero-booting").is_some());
    assert!(!h.text().contains(&dat0_i18n::t("hero.drop")));
    assert!(h.text().contains(&dat0_i18n::t("hero.privacy")));
}

// ── banners ─────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct LiveBannersProps {
    banners: Vec<Banner>,
}

/// Owns the list, so a dismiss actually removes the banner rather than merely
/// reporting that it was asked to.
#[component]
fn LiveBanners(props: LiveBannersProps) -> Element {
    let mut live = use_signal(|| props.banners.clone());
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        BannerHost {
            banners: live.read().clone(),
            on_action: move |id: String| log.write().push(id),
            on_dismiss: move |i: usize| {
                live.write().remove(i);
            },
        }
        div { "data-a11y-id": "log", "{log.read().join(\",\")}" }
    }
}

fn banners(list: Vec<Banner>) -> Harness {
    Harness::new(LiveBanners, LiveBannersProps { banners: list })
}

#[test]
fn nothing_renders_when_there_are_no_banners() {
    let h = banners(Vec::new());
    assert!(h.by_a11y_id("banner-host").is_none());
}

#[test]
fn only_an_error_interrupts_a_screen_reader() {
    // An alert role asks a reader to interrupt whatever it was saying. That is
    // right for a failed session, which is what the window IS until a retry, and
    // wrong for a notice you read when you get to it.
    let h = banners(vec![
        Banner::info("i"),
        Banner::warning("w"),
        Banner::error("e", "detail"),
    ]);
    assert_eq!(
        h.attr(h.by_a11y_id("banner-0").unwrap(), "role").as_deref(),
        Some("note")
    );
    assert_eq!(
        h.attr(h.by_a11y_id("banner-1").unwrap(), "role").as_deref(),
        Some("note")
    );
    assert_eq!(
        h.attr(h.by_a11y_id("banner-2").unwrap(), "role").as_deref(),
        Some("alert")
    );
}

#[test]
fn each_severity_paints_its_own_accent() {
    let h = banners(vec![
        Banner::info("i"),
        Banner::warning("w"),
        Banner::error("e", "d"),
    ]);
    for (i, expect) in ["is-info", "is-warning", "is-error"].iter().enumerate() {
        let class = h
            .attr(h.by_a11y_id(&format!("banner-{i}")).unwrap(), "class")
            .unwrap();
        assert!(class.contains(expect), "{class} must carry {expect}");
    }
}

#[test]
fn a_body_line_only_renders_when_there_is_one() {
    let titled = banners(vec![Banner::warning("just a title")]);
    assert!(titled.by_a11y_id("banner-0-body").is_none());

    let full = banners(vec![Banner::error("title", "the detail")]);
    assert_eq!(
        full.text_of(full.by_a11y_id("banner-0-body").unwrap()),
        "the detail"
    );
}

#[test]
fn action_buttons_dispatch_their_stored_registry_id() {
    // The banner carries an action *id*, not a closure — that is what lets a
    // background task push a banner whose button reaches the shell.
    let mut h = banners(vec![
        Banner::warning("changed")
            .with_primary("Refresh", "live.refresh")
            .with_secondary("Ignore", "banner.ignore"),
    ]);
    h.click("banner-0-act-primary");
    h.click("banner-0-act-secondary");
    assert_eq!(log(&h), "live.refresh,banner.ignore");
}

#[test]
fn a_title_only_banner_has_no_button_row() {
    let h = banners(vec![Banner::info("nothing to do here")]);
    assert!(h.by_a11y_id("banner-0-act-primary").is_none());
    assert!(h.by_a11y_id("banner-0-act-secondary").is_none());
}

#[test]
fn only_a_dismissible_banner_offers_a_dismiss() {
    let dismissible = banners(vec![Banner::warning("go away")]);
    assert!(dismissible.by_a11y_id("banner-0-dismiss").is_some());

    let sticky = Banner {
        dismissible: false,
        ..Banner::error("session failed", "disk full")
    };
    let pinned = banners(vec![sticky]);
    assert!(
        pinned.by_a11y_id("banner-0-dismiss").is_none(),
        "an error you can close before reading is worse than no error"
    );
}

#[test]
fn dismissing_removes_that_banner_and_renumbers_the_rest() {
    let mut h = banners(vec![Banner::info("first"), Banner::warning("second")]);
    h.click("banner-0-dismiss");
    assert!(
        h.text_of(h.by_a11y_id("banner-0").unwrap())
            .contains("second"),
        "the survivor takes slot 0 so its buttons stay addressable"
    );
    assert!(h.by_a11y_id("banner-1").is_none());
}

#[test]
fn stacked_banners_get_their_own_action_ids() {
    // One shared `banner-act-primary` id would leave the second banner's button
    // unreachable to a test and to a reader walking by id.
    let h = banners(vec![
        Banner::warning("a").with_primary("A", "act.a"),
        Banner::warning("b").with_primary("B", "act.b"),
    ]);
    assert!(h.by_a11y_id("banner-0-act-primary").is_some());
    assert!(h.by_a11y_id("banner-1-act-primary").is_some());
}

// ── filter popover ──────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct FilterHostProps {
    column_type: ColumnType,
    existing: Option<Transformation>,
    candidates: Vec<String>,
    total_distinct: u64,
}

#[component]
fn FilterHost(props: FilterHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        FilterPopover {
            column: "price".to_string(),
            column_type: props.column_type,
            existing: props.existing.clone(),
            at: (120.0, 60.0),
            candidates: props.candidates.clone(),
            total_distinct: props.total_distinct,
            on_outcome: move |o: Outcome| {
                log.write()
                    .push(match o {
                        Outcome::Apply(t) => format!("apply {t:?}"),
                        Outcome::Cancel => "cancel".to_string(),
                        Outcome::Clear { pre_populated } => format!("clear {pre_populated}"),
                    });
            },
        }
        div { "data-a11y-id": "log", "{log.read().join(\",\")}" }
    }
}

fn popover(column_type: ColumnType) -> Harness {
    Harness::new(
        FilterHost,
        FilterHostProps {
            column_type,
            existing: None,
            candidates: Vec::new(),
            total_distinct: 0,
        },
    )
}

/// A typed value, as an `oninput`/`onchange` payload.
fn typed(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

/// A primary-button press. Coordinates are zeroed: the harness has no layout, so
/// any other value would be a fiction.
fn press() -> dioxus::html::SerializedMouseData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    let c = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    dioxus::html::SerializedMouseData::new(
        Some(MouseButton::Primary),
        MouseButton::Primary.into(),
        c,
        Modifiers::empty(),
    )
}

/// The operator options, in presentation order.
fn options(h: &Harness) -> Vec<String> {
    let select = h.by_a11y_id("filter-op").expect("the operator select");
    h.dom()
        .get(select)
        .children
        .clone()
        .into_iter()
        .map(|c| h.text_of(c))
        .collect()
}

/// Pick the operator at `ix` in the current list.
fn pick_op(h: &mut Harness, ix: usize) {
    let select = h.by_a11y_id("filter-op").unwrap();
    h.dispatch(select, "change", typed(&ix.to_string()));
}

#[test]
fn the_operator_list_is_driven_by_the_column_type() {
    // Bool has exactly three operators; offering `Contains` on a boolean is how a
    // filter UI teaches users it does not understand their data.
    assert_eq!(
        options(&popover(ColumnType::Bool)),
        vec![
            dat0_i18n::t("filter.op.is_true"),
            dat0_i18n::t("filter.op.is_false"),
            dat0_i18n::t("filter.op.is_empty"),
        ]
    );

    let numeric = options(&popover(ColumnType::Numeric));
    assert_eq!(numeric.len(), 10);
    assert!(numeric.contains(&dat0_i18n::t("filter.op.between")));
    assert!(
        !numeric.contains(&dat0_i18n::t("filter.op.regex")),
        "regex is a string operator"
    );

    let text = options(&popover(ColumnType::String));
    assert!(text.contains(&dat0_i18n::t("filter.op.regex")));
    assert!(
        !text.contains(&dat0_i18n::t("filter.op.between")),
        "between is not offered on text"
    );
}

#[test]
fn the_first_operator_of_the_type_is_selected_on_open() {
    let h = popover(ColumnType::Bool);
    let first = *h
        .dom()
        .get(h.by_a11y_id("filter-op").unwrap())
        .children
        .first()
        .unwrap();
    assert_eq!(h.attr(first, "selected").as_deref(), Some("true"));
}

#[test]
fn a_nullary_operator_shows_no_value_field() {
    // Bool opens on `IsTrue`, which takes no value.
    let h = popover(ColumnType::Bool);
    assert!(h.by_a11y_id("filter-value").is_none());
    assert!(h.by_a11y_id("filter-range-lo").is_none());
    let apply = h.by_a11y_id("filter-apply").unwrap();
    assert_eq!(
        h.attr(apply, "aria-disabled").as_deref(),
        Some("false"),
        "a nullary operator is complete the moment it is chosen"
    );
}

#[test]
fn a_scalar_operator_needs_a_value_before_it_can_apply() {
    let mut h = popover(ColumnType::Numeric);
    let apply = h.by_a11y_id("filter-apply").unwrap();
    assert_eq!(h.attr(apply, "aria-disabled").as_deref(), Some("true"));

    let field = h.by_a11y_id("filter-value").unwrap();
    h.dispatch(field, "input", typed("10"));
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("false")
    );

    h.click("filter-apply");
    // Numeric text parses to an Int, not a Str — that is what makes the compiled
    // SQL a numeric comparison.
    assert!(
        log(&h).contains("Int(10)"),
        "numeric input must parse to an integer scalar: {}",
        log(&h)
    );
}

#[test]
fn between_needs_both_bounds() {
    let mut h = popover(ColumnType::Numeric);
    let between = options(&h)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.between"))
        .unwrap();
    pick_op(&mut h, between);

    assert!(h.by_a11y_id("filter-value").is_none());
    let lo = h.by_a11y_id("filter-range-lo").unwrap();
    h.dispatch(lo, "input", typed("1"));
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("true"),
        "one bound is half a range"
    );

    let hi = h.by_a11y_id("filter-range-hi").unwrap();
    h.dispatch(hi, "input", typed("9"));
    h.click("filter-apply");
    let emitted = log(&h);
    assert!(emitted.contains("Range"), "{emitted}");
    assert!(
        emitted.contains("inclusive: true"),
        "a range is inclusive unless the box is cleared: {emitted}"
    );
}

#[test]
fn clearing_the_inclusive_box_reaches_the_built_transformation() {
    let mut h = popover(ColumnType::Numeric);
    let between = options(&h)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.between"))
        .unwrap();
    pick_op(&mut h, between);
    h.dispatch(
        h.by_a11y_id("filter-range-lo").unwrap(),
        "input",
        typed("1"),
    );
    h.dispatch(
        h.by_a11y_id("filter-range-hi").unwrap(),
        "input",
        typed("9"),
    );

    let box_ = h.by_a11y_id("filter-range-inclusive").unwrap();
    h.dispatch(box_, "change", typed("false"));
    h.click("filter-apply");
    assert!(log(&h).contains("inclusive: false"), "{}", log(&h));
}

#[test]
fn a_regex_must_compile_before_it_can_apply() {
    let mut h = popover(ColumnType::String);
    let regex = options(&h)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.regex"))
        .unwrap();
    pick_op(&mut h, regex);

    let field = h.by_a11y_id("filter-value").unwrap();
    h.dispatch(field, "input", typed("a("));
    assert_eq!(
        h.text_of(h.by_a11y_id("filter-regex-hint").unwrap()),
        dat0_i18n::t("filter.regex.invalid")
    );
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("true"),
        "an uncompilable pattern would fail inside the engine"
    );

    h.dispatch(h.by_a11y_id("filter-value").unwrap(), "input", typed("a.*"));
    assert_eq!(
        h.text_of(h.by_a11y_id("filter-regex-hint").unwrap()),
        dat0_i18n::t("filter.regex.valid")
    );
    h.click("filter-apply");
    assert!(log(&h).contains("Regex"), "{}", log(&h));
}

#[test]
fn the_regex_hint_only_belongs_to_the_regex_operator() {
    let mut h = popover(ColumnType::String);
    assert!(h.by_a11y_id("filter-regex-hint").is_none());

    let regex = options(&h)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.regex"))
        .unwrap();
    pick_op(&mut h, regex);
    h.dispatch(h.by_a11y_id("filter-value").unwrap(), "input", typed("a.*"));
    assert!(h.by_a11y_id("filter-regex-hint").is_some());

    // Switching away drops both the hint and the validity flag.
    pick_op(&mut h, 0);
    assert!(h.by_a11y_id("filter-regex-hint").is_none());
}

#[test]
fn an_in_list_needs_at_least_one_value_and_chips_toggle() {
    let mut h = Harness::new(
        FilterHost,
        FilterHostProps {
            column_type: ColumnType::String,
            existing: None,
            candidates: vec!["alpha".into(), "beta".into()],
            total_distinct: 2,
        },
    );
    let in_op = options(&h)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.in"))
        .unwrap();
    pick_op(&mut h, in_op);

    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("true"),
        "an empty IN list matches nothing"
    );

    h.click("filter-list-0");
    assert_eq!(
        h.attr(h.by_a11y_id("filter-list-0").unwrap(), "aria-pressed")
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("false")
    );

    // Clicking the same chip again deselects it, which must take Apply back out.
    h.click("filter-list-0");
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("true")
    );
}

#[test]
fn a_typed_in_value_becomes_a_visible_chip() {
    // The GPUI panel rendered candidates only, so a typed value enabled Apply
    // while appearing nowhere — the user could not see or remove what they added.
    let mut h = Harness::new(
        FilterHost,
        FilterHostProps {
            column_type: ColumnType::String,
            existing: None,
            candidates: vec!["alpha".into()],
            total_distinct: 1,
        },
    );
    let in_op = options(&h)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.in"))
        .unwrap();
    pick_op(&mut h, in_op);

    // Typing is an `input`; Enter is what commits it.
    let entry = h.by_a11y_id("filter-list-entry").unwrap();
    h.dispatch(entry, "input", typed("gamma"));
    h.key(entry, Key::Enter, Modifiers::empty());
    let chip = h
        .by_a11y_id("filter-list-1")
        .expect("a typed value must be visible as a chip");
    assert!(h.text_of(chip).contains("gamma"));
    assert_eq!(
        h.attr(h.by_a11y_id("filter-list-entry").unwrap(), "value")
            .as_deref(),
        Some(""),
        "the field empties, so the next value is typed into an empty box"
    );

    // A duplicate is silently ignored, so holding Enter cannot fill the list.
    let entry = h.by_a11y_id("filter-list-entry").unwrap();
    h.dispatch(entry, "input", typed("gamma"));
    h.key(entry, Key::Enter, Modifiers::empty());
    assert!(h.by_a11y_id("filter-list-2").is_none());
}

#[test]
fn the_truncation_notice_appears_only_past_the_fetch_cap() {
    let small = Harness::new(
        FilterHost,
        FilterHostProps {
            column_type: ColumnType::String,
            existing: None,
            candidates: vec!["alpha".into()],
            total_distinct: 3,
        },
    );
    let mut small = small;
    let in_op = options(&small)
        .iter()
        .position(|l| *l == dat0_i18n::t("filter.op.in"))
        .unwrap();
    pick_op(&mut small, in_op);
    assert!(small.by_a11y_id("filter-list-truncated").is_none());

    let mut big = Harness::new(
        FilterHost,
        FilterHostProps {
            column_type: ColumnType::String,
            existing: None,
            candidates: vec!["alpha".into()],
            total_distinct: 4_000,
        },
    );
    pick_op(&mut big, in_op);
    let notice = big.text_of(big.by_a11y_id("filter-list-truncated").unwrap());
    assert!(notice.contains("4000"), "{notice}");
}

#[test]
fn reopening_on_a_filtered_column_pre_populates_and_can_clear() {
    // The edit flow: the funnel on an already-filtered column reopens the filter
    // it has, and `Clear` reports that there is one to retract.
    let existing = Transformation::Filter {
        column: "price".into(),
        op: FilterOp::Contains,
        value: FilterValue::Scalar {
            value: Scalar::Str("abc".into()),
        },
    };
    let mut h = Harness::new(
        FilterHost,
        FilterHostProps {
            column_type: ColumnType::String,
            existing: Some(existing),
            candidates: Vec::new(),
            total_distinct: 0,
        },
    );
    assert_eq!(
        h.attr(h.by_a11y_id("filter-value").unwrap(), "value")
            .as_deref(),
        Some("abc")
    );
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("false"),
        "a pre-populated filter is already valid"
    );

    h.click("filter-clear");
    assert_eq!(log(&h), "clear true");
}

#[test]
fn clear_on_a_fresh_column_reports_nothing_to_retract() {
    let mut h = popover(ColumnType::String);
    h.click("filter-clear");
    assert_eq!(
        log(&h),
        "clear false",
        "there is no stack op to remove for a column that was never filtered"
    );
}

#[test]
fn a_pre_populated_regex_is_applyable_without_a_keystroke() {
    // `regex_valid` is derived from the text. Leaving it unset after
    // pre-populating meant Apply was dead until the user typed into a pattern
    // that was already correct.
    let h = Harness::new(
        FilterHost,
        FilterHostProps {
            column_type: ColumnType::String,
            existing: Some(Transformation::Filter {
                column: "price".into(),
                op: FilterOp::Regex,
                value: FilterValue::Scalar {
                    value: Scalar::Str("^a.*z$".into()),
                },
            }),
            candidates: Vec::new(),
            total_distinct: 0,
        },
    );
    assert_eq!(
        h.attr(h.by_a11y_id("filter-apply").unwrap(), "aria-disabled")
            .as_deref(),
        Some("false")
    );
}

#[test]
fn escape_cancel_and_a_click_outside_all_close_the_same_way() {
    let mut by_escape = popover(ColumnType::String);
    by_escape.key_at("filter-popover", Key::Escape, Modifiers::empty());
    assert_eq!(log(&by_escape), "cancel");

    let mut by_button = popover(ColumnType::String);
    by_button.click("filter-cancel");
    assert_eq!(log(&by_button), "cancel");

    let mut by_outside = popover(ColumnType::String);
    let shield = by_outside.by_a11y_id("filter-popover-dismiss").unwrap();
    by_outside.dispatch(shield, "mousedown", press());
    assert_eq!(log(&by_outside), "cancel");
}

#[test]
fn the_popover_is_anchored_to_the_funnel_that_opened_it() {
    // Not geometry: the position the caller passed must reach the style, or the
    // popover lands in the middle of the window regardless of the column.
    let h = popover(ColumnType::String);
    let style = h
        .attr(h.by_a11y_id("filter-popover").unwrap(), "style")
        .unwrap();
    assert!(style.contains("left: 120px"), "{style}");
    assert!(style.contains("top: 60px"), "{style}");
}
