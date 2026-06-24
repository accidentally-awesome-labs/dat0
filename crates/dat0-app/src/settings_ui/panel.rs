//! Stateful Settings window entity (P10b — discharges the deferred "T13").
//! Holds the InputState widgets + selected section; renders the sidebar from
//! `sections::all_sections()` and dispatches to per-section render methods.

use super::sections;
use crate::settings::store::SettingsStore;
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
}

impl SettingsPanel {
    pub fn new(store: SettingsStore, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let name0 = store.get_string("author.name").unwrap_or_default();
        let email0 = store.get_string("author.email").unwrap_or_default();
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name")
                .default_value(name0)
        });
        let email_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Email")
                .default_value(email0)
        });
        let mb0 = store
            .load_or_default()
            .map(|s| s.memory_budget_mb)
            .unwrap_or(1024)
            .to_string();
        let budget_input = cx.new(|cx| InputState::new(window, cx).default_value(mb0));
        Self {
            selected_section: "profile".into(),
            name_input,
            email_input,
            budget_input,
            store,
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
                let active = id == sel;
                div()
                    .id(gpui::SharedString::from(id.clone()))
                    .cursor_pointer()
                    .px_3()
                    .py_1()
                    .when(active, |d| d.bg(gpui::rgba(0x3b82f622)))
                    .child(dat0_i18n::t(s.name_key()))
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
            .child(
                Button::new("settings-theme-cycle")
                    .label(format!("{}: {}", dat0_i18n::t("settings.theme"), current))
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
                    })),
            )
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

    fn render_memory_budget(&self, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(dat0_i18n::t("settings.memory_budget.placeholder"))
            .child(Input::new(&self.budget_input))
            .child(dat0_i18n::t("settings.memory_budget.footnote"))
    }

    fn persist_inputs(&self, cx: &gpui::App) {
        let name = self.name_input.read(cx).value().to_string();
        let email = self.email_input.read(cx).value().to_string();
        let _ = sections::profile::ProfileSection::on_name_change(&self.store, &name);
        let _ = sections::profile::ProfileSection::on_email_change(&self.store, &email);
        if let Ok(mb) = self.budget_input.read(cx).value().trim().parse::<u32>() {
            let _ = crate::settings::set_memory_budget_mb(&self.store, mb);
        }
    }
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
