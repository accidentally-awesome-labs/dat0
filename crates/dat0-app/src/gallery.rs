//! Token gallery — the dat0 design system rendered as one scrollable page
//! (UI redesign A4, master plan §5 row A4).
//!
//! Dev-only: gated on the `gallery` feature, which only the self-dev-dependency
//! turns on, so none of this reaches the shipped binary. Boot it with
//! `cargo run -p dat0-app --example gallery`.
//!
//! This is the manual-UAT vehicle for every later slice — the accumulated "owed
//! human glance" backlog (palette feel, HC legibility, focus ring, elevation
//! shadows, A5 icons, B1 modal scrim) is paid here in one window instead of by
//! booting the whole app once per theme.
//!
//! STRICT ZERO-LITERAL. Every color comes from `cx.theme()` / `cx.theme().d0()`,
//! every gap from `Sp`, every text size from `TextRole`, every surface from
//! `Elevation`. `tests/style_lint.rs` scans this file with an allowance of 0. If
//! a section cannot be expressed in tokens, that is a missing token, and adding
//! it is the point.

use gpui::{Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div, prelude::*};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme as _, Theme as ComponentTheme, h_flex, v_flex};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::theme::Theme;
use crate::theme::tokens::{
    Dat0Theme as _, Elevation, ElevationStyled as _, Sp, SpStyled as _, TextRole, TypoStyled as _,
};

pub struct GalleryView {
    /// Live widget for the components section (T4). Built once — `InputState`
    /// needs a `Window`, which `render` does not hand to child constructors.
    // A4 T4 removes this attribute when components_section starts reading the
    // field; it exists only so the T2 commit is clippy-clean at -D warnings.
    #[allow(dead_code)]
    sample_input: Entity<InputState>,
}

impl GalleryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Master-plan §8 invariant: every view entity holds a theme
        // subscription. `Theme::switch` also calls `refresh_windows()`, so this
        // is belt-and-braces rather than the only repaint path — but the
        // invariant is about not depending on that.
        cx.observe_global::<Theme>(|_, cx| cx.notify()).detach();
        Self {
            sample_input: cx.new(|cx| InputState::new(window, cx).placeholder("sample input")),
        }
    }
}

impl Render for GalleryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .id("gallery-root")
            .size_full()
            .overflow_y_scroll()
            .elevation(Elevation::Background, theme)
            .text_color(theme.foreground)
            .p_sp(Sp::S16)
            .gap_sp(Sp::S24)
            .child(theme_row(theme))
            .child(colors_section(theme))
    }
}

/// Shared section shell: title + a11y seam + body. Every section goes through
/// this so the smoke test's seam contract cannot drift per-section.
fn section(
    theme: &ComponentTheme,
    seam: &'static str,
    title: &str,
    body: impl IntoElement,
) -> impl IntoElement {
    v_flex()
        .gap_sp(Sp::S8)
        .child(
            div()
                .text_role(TextRole::Display)
                .text_color(theme.foreground)
                .a11y_label(AccessRole::Label, seam)
                .child(title.to_string()),
        )
        .child(body)
}

/// One named color chip: the swatch itself plus its token name.
fn swatch(theme: &ComponentTheme, name: &str, color: gpui::Hsla) -> impl IntoElement {
    v_flex()
        .gap_sp(Sp::S2)
        .w(Sp::S32.pixels() * 4.0)
        .child(
            div()
                .h(Sp::S32.pixels())
                .w_full()
                .bg(color)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius),
        )
        .child(
            div()
                .text_role(TextRole::Caption)
                .text_color(theme.muted_foreground)
                .child(name.to_string()),
        )
}

fn theme_row(theme: &ComponentTheme) -> impl IntoElement {
    section(
        theme,
        "gallery.theme",
        "Theme",
        h_flex()
            .gap_sp(Sp::S8)
            .child(
                Button::new("gallery-theme-dark")
                    .label("dark")
                    .primary()
                    .on_click(|_ev, _window, cx| Theme::switch(cx, "dark")),
            )
            .child(
                Button::new("gallery-theme-light")
                    .label("light")
                    .on_click(|_ev, _window, cx| Theme::switch(cx, "light")),
            )
            .child(
                Button::new("gallery-theme-hc")
                    .label("high-contrast")
                    .on_click(|_ev, _window, cx| Theme::switch(cx, "high-contrast")),
            ),
    )
}

fn colors_section(theme: &ComponentTheme) -> impl IntoElement {
    let d0 = theme.d0();
    // All 21 Dat0Colors fields — the derived layer A6 will consume.
    let dat0: Vec<(&str, gpui::Hsla)> = vec![
        ("focus_ring", d0.focus_ring),
        ("selection_tint", d0.selection_tint),
        ("fill_handle", d0.fill_handle),
        ("active_cell_tint", d0.active_cell_tint),
        ("marching_ants", d0.marching_ants),
        ("null_value_fg", d0.null_value_fg),
        ("banner_info", d0.banner_info),
        ("banner_warning", d0.banner_warning),
        ("banner_error", d0.banner_error),
        ("banner_tint", d0.banner_tint),
        ("hover_tint", d0.hover_tint),
        ("drag_over", d0.drag_over),
        ("pipeline_pill", d0.pipeline_pill),
        ("pipeline_accent", d0.pipeline_accent),
        ("pipeline_chip", d0.pipeline_chip),
        ("text_muted", d0.text_muted),
        ("text_error", d0.text_error),
        ("chart_placeholder_a", d0.chart_placeholder_a),
        ("chart_placeholder_b", d0.chart_placeholder_b),
        ("pager_dot_active", d0.pager_dot_active),
        ("pager_dot_inactive", d0.pager_dot_inactive),
    ];
    // The gpui-component families the A1 builtins define.
    let base: Vec<(&str, gpui::Hsla)> = vec![
        ("background", theme.background),
        ("foreground", theme.foreground),
        ("muted", theme.muted),
        ("muted_foreground", theme.muted_foreground),
        ("primary", theme.primary),
        ("primary_foreground", theme.primary_foreground),
        ("secondary", theme.secondary),
        ("danger", theme.danger),
        ("warning", theme.warning),
        ("success", theme.success),
        ("info", theme.info),
        ("ring", theme.ring),
        ("border", theme.border),
        ("popover", theme.popover),
        ("sidebar", theme.sidebar),
        ("list_hover", theme.list_hover),
        ("list_active", theme.list_active),
        ("drop_target", theme.drop_target),
    ];

    let grid = |title: &str, items: Vec<(&str, gpui::Hsla)>| {
        v_flex()
            .gap_sp(Sp::S4)
            .child(div().text_role(TextRole::Title).child(title.to_string()))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_sp(Sp::S8)
                    .children(items.into_iter().map(|(n, c)| swatch(theme, n, c))),
            )
    };

    section(
        theme,
        "gallery.colors",
        "Colors",
        v_flex()
            .gap_sp(Sp::S16)
            .child(grid("Dat0Colors (derived)", dat0))
            .child(grid("ThemeColor (gpui-component)", base)),
    )
}
