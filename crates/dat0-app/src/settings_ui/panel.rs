//! Stateful Settings window entity (P10b — discharges the deferred "T13").
//! Holds the InputState widgets + selected section; renders the sidebar from
//! `sections::all_sections()` and dispatches to per-section render methods.

use super::sections;
use crate::a11y::{A11yExt as _, AccessRole};
use crate::settings::store::SettingsStore;

/// Which right-dock panel to toggle in the focused workspace window.
enum DockKind {
    Ai,
    Connections,
}
use gpui::{
    Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div, prelude::*, px,
};
use gpui_component::Root;
use gpui_component::input::{Input, InputState};

pub struct SettingsPanel {
    selected_section: String,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    budget_input: Entity<InputState>,
    store: SettingsStore,
    last_name: String,
    last_email: String,
    last_budget: String,
}

impl SettingsPanel {
    pub fn new(store: SettingsStore, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let name0 = store.get_string("author.name").unwrap_or_default();
        let email0 = store.get_string("author.email").unwrap_or_default();
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(dat0_i18n::t("settings.profile.name_placeholder"))
                .default_value(name0.clone())
        });
        let email_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(dat0_i18n::t("settings.profile.email_placeholder"))
                .default_value(email0.clone())
        });
        let mb0 = store
            .load_or_default()
            .map(|s| s.memory_budget_mb)
            .unwrap_or(1024)
            .to_string();
        let budget_input = cx.new(|cx| InputState::new(window, cx).default_value(mb0.clone()));
        Self {
            selected_section: "profile".into(),
            name_input,
            email_input,
            budget_input,
            store,
            last_name: name0,
            last_email: email0,
            last_budget: mb0,
        }
    }

    fn render_sidebar(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let sel = self.selected_section.clone();
        div()
            .w(px(200.))
            .flex()
            .flex_col()
            .children(sections::all_sections().into_iter().map(|s| {
                let id = s.id().to_string();
                let static_id = s.id();
                let active = id == sel;
                div()
                    .id(gpui::SharedString::from(id.clone()))
                    .cursor_pointer()
                    .px_3()
                    .py_1()
                    .when(active, |d| d.bg(gpui::rgba(0x3b82f622)))
                    .child(dat0_i18n::t(s.name_key()))
                    // UAT settings-window slice (T0): `s.id()` is already
                    // `&'static str` (the `SettingsSection::id()` contract), so
                    // it doubles as both the on_click selection key AND the
                    // `.a11y` click id — no `sample_static_id`-style mapping
                    // needed (contrast `empty_state.rs::sample_column`, which
                    // maps a *runtime* index to a static id). Feature OFF
                    // (release) → identity no-op.
                    .a11y(static_id, AccessRole::Button, dat0_i18n::t(s.name_key()))
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        this.selected_section = id.clone();
                        cx.notify();
                    }))
            }))
    }

    fn toggle_row(
        &self,
        id: &'static str,
        label_key: &'static str,
        on: bool,
        cx: &mut gpui::Context<Self>,
        set: fn(&SettingsStore, bool) -> anyhow::Result<()>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .cursor_pointer()
            .flex()
            .flex_row()
            .gap_2()
            .px_3()
            .py_1()
            .child(if on { "[x]" } else { "[ ]" })
            .child(dat0_i18n::t(label_key))
            // UAT settings-window slice (T0): shared by telemetry/workspace/
            // updates toggles, so annotating once here covers all three.
            // Feature OFF (release) → identity no-op.
            .a11y(id, AccessRole::Button, dat0_i18n::t(label_key))
            .on_click(cx.listener(move |this, _ev, _w, cx| {
                let _ = set(&this.store, !on);
                cx.notify();
            }))
    }

    fn render_telemetry(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui_component::button::Button;
        let on = self
            .store
            .load_or_default()
            .map(|s| s.telemetry.crash_submission_enabled)
            .unwrap_or(false);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(self.toggle_row(
                "tg-telemetry",
                "settings.telemetry.toggle",
                on,
                cx,
                sections::telemetry::set_crash_submission_enabled,
            ))
            .child(
                Button::new("telemetry-privacy")
                    .label(dat0_i18n::t("settings.telemetry.learn_more"))
                    .on_click(|_ev, _w, _cx| {
                        let _ = crate::platform::open_url(
                            "https://github.com/accidentally-awesome-labs/dat0/blob/main/docs/privacy.md",
                        );
                    }),
            )
    }

    fn render_workspace(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let on = self
            .store
            .load_or_default()
            .map(|s| s.workspace.treat_all_as_networked)
            .unwrap_or(false);
        div().flex().flex_col().gap_2().p_3().child(self.toggle_row(
            "tg-workspace",
            "settings.workspace.toggle",
            on,
            cx,
            sections::workspace::set_treat_all_as_networked,
        ))
    }

    fn render_updates(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let on = self
            .store
            .load_or_default()
            .map(|s| s.update_auto_check)
            .unwrap_or(false);
        div().flex().flex_col().gap_2().p_3().child(self.toggle_row(
            "tg-updates",
            "settings.updates.toggle",
            on,
            cx,
            sections::updates::set_update_auto_check,
        ))
    }

    fn render_theme(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui_component::button::Button;
        let current = self
            .store
            .get_string("theme.id")
            .unwrap_or_else(|| "dark".into());
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(dat0_i18n::t("settings.theme.placeholder"))
            .child({
                // UAT settings-window slice (T0): the cycle button's label is a
                // dynamic `"{Theme}: {current}"` string, not a fixed i18n key
                // (no `settings.theme.cycle` key exists in
                // `crates/dat0-i18n/src/strings/en.json` — `dat0_i18n::t` would
                // silently fall back to echoing the missing key, which is not
                // a real assertable label). So `.a11y` mirrors the button's
                // OWN `.label(...)` text exactly, rather than inventing a new
                // i18n key. Feature OFF (release) → identity no-op.
                let cycle_label = format!("{}: {}", dat0_i18n::t("settings.theme"), current);
                Button::new("settings-theme-cycle")
                    .label(cycle_label.clone())
                    .a11y("settings-theme-cycle", AccessRole::Button, cycle_label)
                    .on_click(cx.listener(|this, _ev, _w, cx| {
                        const ORDER: [&str; 3] = ["dark", "light", "high-contrast"];
                        let cur = this
                            .store
                            .get_string("theme.id")
                            .unwrap_or_else(|| "dark".into());
                        let i = ORDER.iter().position(|t| *t == cur).unwrap_or(0);
                        let next = ORDER[(i + 1) % ORDER.len()];
                        let _ = this.store.set("theme.id", next);
                        crate::theme::Theme::switch(cx, next);
                        cx.notify();
                    }))
            })
    }

    fn render_profile(&self, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(dat0_i18n::t("settings.profile.placeholder"))
            .child(Input::new(&self.name_input))
            .child(Input::new(&self.email_input))
    }

    fn render_advanced(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui_component::button::{Button, ButtonVariants as _};
        let b = crate::about::build_info::BuildInfo::current();
        let level = self
            .store
            .load_or_default()
            .map(|s| s.log_level)
            .unwrap_or_else(|_| "info,dat0=debug".into());
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(format!("dat0 {} ({})", b.version, b.git_sha))
            .child(
                Button::new("adv-open-logs")
                    .label(dat0_i18n::t("settings.advanced.open_logs"))
                    .on_click(|_e, _w, _cx| {
                        if let Ok(d) = crate::platform::cache_dir() {
                            let _ = crate::platform::open_url(&d.to_string_lossy());
                        }
                    }),
            )
            .child(
                Button::new("adv-reveal-config")
                    .label(dat0_i18n::t("settings.advanced.reveal_config"))
                    .on_click(|_e, _w, _cx| {
                        if let Ok(d) = crate::platform::config_dir() {
                            let _ = crate::platform::open_url(&d.to_string_lossy());
                        }
                    }),
            )
            .child(
                Button::new("adv-log-level")
                    .label(format!(
                        "{}: {}",
                        dat0_i18n::t("settings.advanced.log_level"),
                        level
                    ))
                    .on_click(cx.listener(|this, _e, _w, cx| {
                        const LV: [&str; 4] = ["error", "warn", "info,dat0=debug", "debug"];
                        let cur = this
                            .store
                            .load_or_default()
                            .map(|s| s.log_level)
                            .unwrap_or_else(|_| "info,dat0=debug".into());
                        let i = LV.iter().position(|l| *l == cur).unwrap_or(2);
                        let _ = crate::settings::set_log_level(&this.store, LV[(i + 1) % LV.len()]);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("adv-reset")
                    .label(dat0_i18n::t("settings.advanced.reset"))
                    .ghost()
                    .on_click(cx.listener(|this, _e, window, cx| {
                        this.open_reset_confirm(window, cx);
                    })),
            )
    }

    fn open_reset_confirm(&self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        use gpui_component::WindowExt as _;
        use gpui_component::dialog::{Dialog, DialogButtonProps};
        // SettingsStore is not Clone; create a fresh store path-equivalent and
        // wrap in Rc so the Fn builder closure can clone it on each invocation.
        let store = std::rc::Rc::new(crate::settings::store::SettingsStore::with_path(
            crate::platform::config_dir()
                .expect("config dir")
                .join("settings.toml"),
        ));
        window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
            let store = std::rc::Rc::clone(&store);
            dialog
                .title(dat0_i18n::t("settings.advanced.reset.title"))
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(dat0_i18n::t("settings.advanced.reset.ok"))
                        .cancel_text(dat0_i18n::t("common.cancel")),
                )
                .child(dat0_i18n::t("settings.advanced.reset.body"))
                .on_ok(move |_e, _w, _cx| {
                    let _ = store.save(&crate::settings::Settings::default());
                    true
                })
                .on_cancel(|_e, _w, _cx| true)
        });
    }

    fn render_memory_budget(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        // UAT settings-window slice (T0): content-only annotation of the
        // input's CURRENT rendered value (not a click target — text inputs are
        // driven by typing / `set_value`, not clicks). Lets a headless test
        // assert the persisted value actually round-trips into the render,
        // the same "content assertion" role the grid-cell / inspector
        // `.a11y_label` annotations play elsewhere. `Input` (unlike `Button`)
        // does NOT implement `InteractiveElement` (it derives `IntoElement`
        // only — a plain `RenderOnce` struct), so `.a11y_label` cannot be
        // chained onto it directly; wrap it in a `div()` instead, mirroring
        // `empty_state.rs`'s tagline wrapper. Feature OFF (release) →
        // identity no-op.
        let budget_value = self.budget_input.read(cx).value().to_string();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(dat0_i18n::t("settings.memory_budget.placeholder"))
            .child(
                div()
                    .a11y_label(AccessRole::Label, budget_value)
                    .child(Input::new(&self.budget_input)),
            )
            .child(dat0_i18n::t("settings.memory_budget.footnote"))
    }

    /// Test-only: drive the budget input's value programmatically (UAT
    /// settings-window slice T0 spike). `budget_input` is a private field —
    /// production code never sets it directly (users type into it) — so this
    /// accessor exists ONLY under `a11y-capture` to let the harness prove
    /// `InputState::set_value` → `persist_inputs()` (below) round-trips
    /// through a real render tick, exactly as typing would. Compiled out
    /// entirely in release builds (no `a11y-capture` feature there).
    #[cfg(feature = "a11y-capture")]
    pub fn set_budget_input_value_for_test(
        &mut self,
        value: impl Into<gpui::SharedString>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let input = self.budget_input.clone();
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }

    /// Toggle a right-dock panel in the focused workspace window (cross-window).
    /// Because this reaches into a separate window's entity, it takes `&mut App`
    /// rather than a panel-scoped context — call via plain `on_click` closure.
    fn launch_dock(cx: &mut gpui::App, which: DockKind) {
        let Some(weak) = crate::window_registry::focused_workspace_weak() else {
            tracing::warn!("settings: no focused workspace to toggle dock");
            return;
        };
        let Some(any_entity) = weak.upgrade() else {
            return;
        };
        let Ok(shell) = any_entity.downcast::<crate::window::WorkspaceShell>() else {
            return;
        };
        shell.update(cx, |ws, cx| match which {
            DockKind::Ai => ws.toggle_ai_panel(cx),
            DockKind::Connections => {
                ws.connections_panel_visible = !ws.connections_panel_visible;
                cx.notify();
            }
        });
    }

    fn render_motherduck(&self) -> impl IntoElement {
        use gpui_component::button::Button;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(dat0_i18n::t("settings.motherduck.placeholder"))
            .child(
                Button::new("md-open")
                    .label(dat0_i18n::t("settings.motherduck.manage"))
                    .on_click(|_e, _w, cx| Self::launch_dock(cx, DockKind::Connections)),
            )
    }

    fn render_ai(&self) -> impl IntoElement {
        use gpui_component::button::Button;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(dat0_i18n::t("settings.ai.placeholder"))
            .child(
                Button::new("ai-open")
                    .label(dat0_i18n::t("settings.ai.configure"))
                    .on_click(|_e, _w, cx| Self::launch_dock(cx, DockKind::Ai)),
            )
    }

    fn persist_inputs(&mut self, cx: &gpui::App) {
        let name = self.name_input.read(cx).value().to_string();
        if changed(&self.last_name, &name) {
            let _ = sections::profile::ProfileSection::on_name_change(&self.store, &name);
            self.last_name = name;
        }
        let email = self.email_input.read(cx).value().to_string();
        if changed(&self.last_email, &email) {
            let _ = sections::profile::ProfileSection::on_email_change(&self.store, &email);
            self.last_email = email;
        }
        let budget = self.budget_input.read(cx).value().trim().to_string();
        if changed(&self.last_budget, &budget) {
            if let Ok(mb) = budget.parse::<u32>() {
                let _ = crate::settings::set_memory_budget_mb(&self.store, mb);
            }
            self.last_budget = budget;
        }
    }
}

/// True when the input value differs from the last value persisted this session.
pub fn changed(prev: &str, next: &str) -> bool {
    prev != next
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        // Persist Profile inputs on each render tick (cheap; values are short).
        self.persist_inputs(cx);
        let content = match self.selected_section.as_str() {
            "profile" => self.render_profile(cx).into_any_element(),
            "theme" => self.render_theme(cx).into_any_element(),
            "memory_budget" => self.render_memory_budget(cx).into_any_element(),
            "telemetry" => self.render_telemetry(cx).into_any_element(),
            "workspace" => self.render_workspace(cx).into_any_element(),
            "updates" => self.render_updates(cx).into_any_element(),
            "advanced" => self.render_advanced(cx).into_any_element(),
            "motherduck" => self.render_motherduck().into_any_element(),
            "ai" => self.render_ai().into_any_element(),
            _ => div()
                .p_3()
                .child(dat0_i18n::t("settings.section.placeholder"))
                .into_any_element(),
        };
        div()
            .flex()
            .flex_row()
            .size_full()
            .child(self.render_sidebar(cx))
            .child(div().flex_1().child(content))
            // Mandatory: dialog layer for the Reset confirm (T9).
            .children(Root::render_dialog_layer(window, cx))
    }
}
