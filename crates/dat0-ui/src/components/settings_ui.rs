//! Settings: nine sections, in a window of their own.
//!
//! It stays a separate OS window rather than folding into the shell's modal
//! slot, for the reason it was one under GPUI: settings is a place you leave
//! open beside the workbench while you change a memory budget and watch what
//! it does. A modal would make that impossible, and the slot holds exactly one
//! surface — you could not read the About box while editing a log level.
//!
//! [`open_settings_window`] is the entry point; the shell's action router
//! calls it for `ids::SETTINGS_OPEN`. Each window is its own `VirtualDom`, so
//! the settings window carries its own theme signal and is unaffected by
//! whatever the workbench window is doing.
//!
//! ## What persists, and when
//!
//! Every control writes through [`SettingsStore`]'s atomic load-mutate-save
//! path the moment it changes — there is no Apply button and never was. Text
//! fields are guarded by [`changed`] so an unmodified field does not rewrite
//! `settings.toml` on every keystroke, which is the GPUI panel's
//! `persist_inputs` contract, kept.
//!
//! ## Cross-window controls
//!
//! Three controls act on the *workbench* window, not on this one: the theme
//! cycle, "Open AI Panel" and "Open Connections Panel". They post on the event
//! bus ([`AppEvent::ThemeChanged`] / [`AppEvent::RunAction`]) rather than
//! reaching for another window's state, because a separate `VirtualDom` has no
//! handle on the other one — and because the bus is what "the shell decides in
//! which window" already means everywhere else.

use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::actions::builtin::ids;
use dat0_core::events::{AppEvent, AppEvents};
use dat0_core::settings::Settings;
use dat0_core::settings::store::SettingsStore;
use dat0_core::theme::{BUILTIN_IDS, DEFAULT_ID, ThemeTokens};

use crate::a11y::AccessRole;

/// The connections surface, named for the shell's router.
///
/// Not an `ActionRegistry` id: `AppEvent::RunAction` carries a plain string
/// and the registry exists for the palette and the menu bar, neither of which
/// offers this. Registering it would also add a row `dat0-app`'s keymap
/// ratchet would demand a chord for.
pub const CONNECTIONS_OPEN: &str = "connections.open";

/// One sidebar entry: a stable id and the i18n key for its label.
///
/// The GPUI original was a `SettingsSection` trait with nine unit structs and
/// a `Vec<Box<dyn …>>` registry. The trait had two methods, both returning
/// `&'static str`, and nothing ever implemented it outside this list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Section {
    pub id: &'static str,
    pub name_key: &'static str,
}

/// Every section, in display order.
pub const SECTIONS: [Section; 9] = [
    Section {
        id: "profile",
        name_key: "settings.profile",
    },
    Section {
        id: "theme",
        name_key: "settings.theme",
    },
    Section {
        id: "memory_budget",
        name_key: "settings.memory_budget",
    },
    Section {
        id: "motherduck",
        name_key: "settings.motherduck",
    },
    Section {
        id: "ai",
        name_key: "settings.ai",
    },
    Section {
        id: "telemetry",
        name_key: "settings.telemetry",
    },
    Section {
        id: "workspace",
        name_key: "settings.workspace",
    },
    Section {
        id: "updates",
        name_key: "settings.updates",
    },
    Section {
        id: "advanced",
        name_key: "settings.advanced",
    },
];

/// The log-level directives the Advanced cycle steps through, and the index it
/// falls back to when the persisted value is not one of them.
pub const LOG_LEVELS: [&str; 4] = ["error", "warn", "info,dat0=debug", "debug"];
const LOG_LEVEL_FALLBACK: usize = 2;

/// The privacy note the Telemetry section links to.
const PRIVACY_URL: &str =
    "https://github.com/accidentally-awesome-labs/dat0/blob/main/docs/privacy.md";

/// True when an input differs from the value last persisted this session.
///
/// The guard that stops a settings write per keystroke on an untouched field.
pub fn changed(prev: &str, next: &str) -> bool {
    prev != next
}

/// Persist the crash-report opt-in.
pub fn set_crash_submission_enabled(store: &SettingsStore, value: bool) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.telemetry.crash_submission_enabled = value;
    store.save(&s)
}

/// Persist the global "treat every workspace as networked" override.
pub fn set_treat_all_as_networked(store: &SettingsStore, value: bool) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.workspace.treat_all_as_networked = value;
    store.save(&s)
}

/// Persist the launch-time update check opt-in.
pub fn set_update_auto_check(store: &SettingsStore, value: bool) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.update_auto_check = value;
    store.save(&s)
}

/// The next theme in the cycle, wrapping. Driven off [`BUILTIN_IDS`] so adding
/// a builtin does not need a second list here.
pub fn next_theme(current: &str) -> &'static str {
    let i = BUILTIN_IDS.iter().position(|t| *t == current).unwrap_or(0);
    BUILTIN_IDS[(i + 1) % BUILTIN_IDS.len()]
}

/// The next log-level directive in the cycle, wrapping.
pub fn next_log_level(current: &str) -> &'static str {
    let i = LOG_LEVELS
        .iter()
        .position(|l| *l == current)
        .unwrap_or(LOG_LEVEL_FALLBACK);
    LOG_LEVELS[(i + 1) % LOG_LEVELS.len()]
}

/// A shared settings store.
///
/// A newtype only because Dioxus props must be `PartialEq` and
/// `SettingsStore` is a file handle: two stores are the same store when they
/// are the same allocation, and comparing paths would call two independently
/// opened stores on one file "different" and re-render forever.
#[derive(Clone)]
pub struct Store(pub Arc<SettingsStore>);

impl Store {
    pub fn open(path: std::path::PathBuf) -> Self {
        Self(Arc::new(SettingsStore::with_path(path)))
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for Store {
    type Target = SettingsStore;
    fn deref(&self) -> &SettingsStore {
        &self.0
    }
}

/// The event bus, or nothing.
///
/// `None` in a headless mount: the three cross-window controls then persist
/// and do not post. Equality is presence, because a channel handle has no
/// identity worth comparing.
#[derive(Clone, Default)]
pub struct Bus(pub Option<AppEvents>);

impl PartialEq for Bus {
    fn eq(&self, other: &Self) -> bool {
        self.0.is_some() == other.0.is_some()
    }
}

/// A counter every control that writes bumps, and every control that displays
/// reads.
///
/// `settings.toml` is a file, not a signal. Without this a toggle writes the
/// new value and then re-renders from the value it read *before* the write —
/// the control snaps back and the user concludes the click did nothing. GPUI
/// had `cx.notify()` for exactly this; a `Signal` is the same idea with the
/// subscription made explicit.
#[derive(Clone, Copy)]
struct Revision(Signal<u64>);

impl Revision {
    /// Subscribe this scope to settings writes. Every section that reads the
    /// store during render must call it, or it will not re-read.
    fn subscribe() -> Self {
        let r: Self = use_context();
        let _ = (r.0)();
        r
    }

    fn bump(mut self) {
        let next = (self.0)().wrapping_add(1);
        self.0.set(next);
    }
}

#[derive(Clone, PartialEq, Props)]
pub struct SettingsProps {
    pub store: Store,
    /// The bus, for the three controls that act on the workbench window.
    pub events: Bus,
}

/// The settings surface: sidebar plus the selected section.
#[component]
pub fn SettingsPanel(props: SettingsProps) -> Element {
    let mut selected = use_signal(|| SECTIONS[0].id);
    use_context_provider(|| Revision(Signal::new(0)));
    let store = props.store.clone();
    let current = selected();

    rsx! {
        div {
            class: "d0-settings",
            "data-a11y-id": "settings",
            role: AccessRole::Dialog.aria(),
            "aria-label": dat0_i18n::t("settings.window.title"),

            nav {
                class: "d0-settings-nav",
                role: AccessRole::Navigation.aria(),
                "aria-label": dat0_i18n::t("settings"),
                for s in SECTIONS {
                    button {
                        key: "{s.id}",
                        class: if s.id == current { "d0-settings-tab is-active" } else { "d0-settings-tab" },
                        "data-a11y-id": "{s.id}",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t(s.name_key),
                        "aria-current": if s.id == current { "page" },
                        onclick: move |_| selected.set(s.id),
                        {dat0_i18n::t(s.name_key)}
                    }
                }
            }

            section { class: "d0-settings-body", "data-a11y-id": "settings-body-{current}",
                match current {
                    "profile" => rsx! { Profile { store: store.clone() } },
                    "theme" => rsx! { ThemeSection { store: store.clone(), events: props.events.clone() } },
                    "memory_budget" => rsx! { MemoryBudget { store: store.clone() } },
                    "motherduck" => rsx! { CrossWindow {
                        id: "md-open",
                        blurb_key: "settings.motherduck.placeholder",
                        label_key: "settings.motherduck.manage",
                        action: CONNECTIONS_OPEN,
                        events: props.events.clone(),
                    } },
                    "ai" => rsx! { CrossWindow {
                        id: "ai-open",
                        blurb_key: "settings.ai.placeholder",
                        label_key: "settings.ai.configure",
                        action: ids::AI_PANEL_OPEN,
                        events: props.events.clone(),
                    } },
                    "telemetry" => rsx! { Telemetry { store: store.clone() } },
                    "workspace" => rsx! { WorkspaceSection { store: store.clone() } },
                    "updates" => rsx! { Updates { store: store.clone() } },
                    "advanced" => rsx! { Advanced { store: store.clone() } },
                    // Unreachable through the sidebar, but a section id can
                    // also arrive from a deep link one day.
                    _ => rsx! { p { class: "d0-body", {dat0_i18n::t("settings.section.placeholder")} } },
                }
            }
        }
    }
}

/// A section's explanatory paragraph. Annotated so a test can prove the key
/// resolved rather than echoing itself back.
#[component]
fn Blurb(text_key: &'static str) -> Element {
    let text = dat0_i18n::t(text_key);
    rsx! {
        p {
            class: "d0-body",
            role: AccessRole::Label.aria(),
            "aria-label": "{text}",
            "{text}"
        }
    }
}

/// A checkbox-style row. `<button>` gives Enter/Space and the focus ring for
/// free — the whole of what GPUI's hand-rolled `focus_stop` provided.
#[component]
fn Toggle(
    id: &'static str,
    label_key: &'static str,
    on: bool,
    onflip: EventHandler<bool>,
) -> Element {
    let label = dat0_i18n::t(label_key);
    rsx! {
        button {
            class: if on { "d0-settings-toggle is-on" } else { "d0-settings-toggle" },
            "data-a11y-id": "{id}",
            role: "switch",
            "aria-checked": if on { "true" } else { "false" },
            "aria-label": "{label}",
            onclick: move |_| onflip.call(!on),
            span { class: "d0-mono d0-settings-box", if on { "[x]" } else { "[ ]" } }
            span { "{label}" }
        }
    }
}

#[component]
fn Profile(store: Store) -> Element {
    // Seeded once from the store, then owned by the field: re-reading on every
    // render would fight the user's cursor.
    let seed = store.clone();
    let mut name = use_signal(|| seed.get_string("author.name").unwrap_or_default());
    let seed = store.clone();
    let mut email = use_signal(|| seed.get_string("author.email").unwrap_or_default());
    // `clippy::redundant_closure` misfires here: `Signal<T>` is only callable
    // through the nightly `Fn` impls, so on stable `use_signal(name)` passes the
    // signal itself where a closure is required and does not compile.
    #[allow(clippy::redundant_closure)]
    let mut last_name = use_signal(|| name());
    #[allow(clippy::redundant_closure)]
    let mut last_email = use_signal(|| email());

    let name_store = store.clone();
    let email_store = store.clone();

    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.profile.placeholder" }
            label { class: "d0-settings-field",
                span { class: "d0-label", {dat0_i18n::t("settings.profile.name_placeholder")} }
                input {
                    class: "d0-field",
                    "data-a11y-id": "settings-name-input",
                    "aria-label": dat0_i18n::t("settings.profile.name_placeholder"),
                    value: "{name}",
                    oninput: move |e| {
                        let v = e.value();
                        name.set(v.clone());
                        if changed(&last_name(), &v) {
                            let _ = name_store.set("author.name", &v);
                            last_name.set(v);
                        }
                    },
                }
            }
            label { class: "d0-settings-field",
                span { class: "d0-label", {dat0_i18n::t("settings.profile.email_placeholder")} }
                input {
                    class: "d0-field",
                    "data-a11y-id": "settings-email-input",
                    "aria-label": dat0_i18n::t("settings.profile.email_placeholder"),
                    value: "{email}",
                    oninput: move |e| {
                        let v = e.value();
                        email.set(v.clone());
                        if changed(&last_email(), &v) {
                            let _ = email_store.set("author.email", &v);
                            last_email.set(v);
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn ThemeSection(store: Store, events: Bus) -> Element {
    let rev = Revision::subscribe();
    let current = store
        .get_string("theme.id")
        .unwrap_or_else(|| DEFAULT_ID.to_string());
    // The label is dynamic, so it is its own accessible name rather than a
    // fixed i18n key that would disagree with what is painted.
    let label = format!("{}: {}", dat0_i18n::t("settings.theme"), current);
    // Absent in a headless mount, which is why this is a `try_`: the section
    // must render and persist without a theme provider above it.
    let local = try_use_context::<Signal<ThemeTokens>>();
    let cycle_store = store.clone();

    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.theme.placeholder" }
            button {
                class: "d0-btn",
                "data-a11y-id": "settings-theme-cycle",
                role: AccessRole::Button.aria(),
                "aria-label": "{label}",
                onclick: move |_| {
                    let cur = cycle_store
                        .get_string("theme.id")
                        .unwrap_or_else(|| DEFAULT_ID.to_string());
                    let next = next_theme(&cur);
                    let _ = cycle_store.set("theme.id", next);
                    rev.bump();
                    // This window repaints immediately…
                    if let Some(mut tokens) = local {
                        tokens.set(dat0_core::theme::builtin_or_default(next));
                    }
                    // …and every other window is told, because a theme is an
                    // application-wide choice and half a repaint is worse than
                    // none.
                    if let Some(events) = &events.0 {
                        events.send(AppEvent::ThemeChanged { id: next.to_string() });
                    }
                },
                "{label}"
            }
        }
    }
}

#[component]
fn MemoryBudget(store: Store) -> Element {
    let seed = store.clone();
    let mut budget = use_signal(|| {
        seed.load_or_default()
            .map(|s| s.memory_budget_mb)
            .unwrap_or(dat0_core::settings::budget::DEFAULT_MEMORY_BUDGET_MB)
            .to_string()
    });
    // See `Profile` above: `Signal` is not callable on stable, so this closure
    // is load-bearing despite what `redundant_closure` says.
    #[allow(clippy::redundant_closure)]
    let mut last = use_signal(|| budget());
    let input_store = store.clone();

    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.memory_budget.placeholder" }
            label { class: "d0-settings-field",
                span { class: "d0-label", {dat0_i18n::t("settings.memory_budget")} }
                input {
                    class: "d0-field d0-mono",
                    "data-a11y-id": "settings-budget-input",
                    "aria-label": "{budget}",
                    r#type: "number",
                    value: "{budget}",
                    oninput: move |e| {
                        let v = e.value().trim().to_string();
                        budget.set(v.clone());
                        if changed(&last(), &v) {
                            // A half-typed number is not a budget. Nothing is
                            // written until it parses, so clearing the field
                            // does not persist a 0 MB limit.
                            if let Ok(mb) = v.parse::<u32>() {
                                let _ = dat0_core::settings::set_memory_budget_mb(&input_store, mb);
                            }
                            last.set(v);
                        }
                    },
                }
            }
            p { class: "d0-small", {dat0_i18n::t("settings.memory_budget.footnote")} }
        }
    }
}

/// A section whose only control acts on the workbench window.
#[component]
fn CrossWindow(
    id: &'static str,
    blurb_key: &'static str,
    label_key: &'static str,
    action: &'static str,
    events: Bus,
) -> Element {
    let label = dat0_i18n::t(label_key);
    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: blurb_key }
            button {
                class: "d0-btn",
                "data-a11y-id": "{id}",
                role: AccessRole::Button.aria(),
                "aria-label": "{label}",
                "data-action": "{action}",
                onclick: move |_| {
                    if let Some(events) = &events.0 {
                        events.send(AppEvent::RunAction { id: action, window: None });
                    }
                },
                "{label}"
            }
        }
    }
}

#[component]
fn Telemetry(store: Store) -> Element {
    let rev = Revision::subscribe();
    let on = store
        .load_or_default()
        .map(|s| s.telemetry.crash_submission_enabled)
        .unwrap_or(false);
    let flip_store = store.clone();
    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.telemetry.placeholder" }
            Toggle {
                id: "tg-telemetry",
                label_key: "settings.telemetry.toggle",
                on,
                onflip: move |v| {
                    let _ = set_crash_submission_enabled(&flip_store, v);
                    rev.bump();
                },
            }
            button {
                class: "d0-btn is-ghost",
                "data-a11y-id": "telemetry-privacy",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("settings.telemetry.learn_more"),
                onclick: move |_| open_url(PRIVACY_URL),
                {dat0_i18n::t("settings.telemetry.learn_more")}
            }
        }
    }
}

#[component]
fn WorkspaceSection(store: Store) -> Element {
    let rev = Revision::subscribe();
    let s = store.load_or_default().unwrap_or_default();
    let on = s.workspace.treat_all_as_networked;
    let flip_store = store.clone();
    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.workspace.placeholder" }
            Toggle {
                id: "tg-workspace",
                label_key: "settings.workspace.toggle",
                on,
                onflip: move |v| {
                    let _ = set_treat_all_as_networked(&flip_store, v);
                    rev.bump();
                },
            }
            // The force-on list is read-only in v1, and shown rather than
            // hidden: a path on it silently changes how a workspace locks.
            if !s.workspace.treat_paths_as_networked.is_empty() {
                ul { class: "d0-settings-paths", "data-a11y-id": "workspace-paths",
                    for (i, p) in s.workspace.treat_paths_as_networked.iter().enumerate() {
                        li { key: "{i}", class: "d0-mono", "{p.display()}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Updates(store: Store) -> Element {
    let rev = Revision::subscribe();
    let on = store
        .load_or_default()
        .map(|s| s.update_auto_check)
        .unwrap_or(false);
    let flip_store = store.clone();
    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.updates.placeholder" }
            Toggle {
                id: "tg-updates",
                label_key: "settings.updates.toggle",
                on,
                onflip: move |v| {
                    let _ = set_update_auto_check(&flip_store, v);
                    rev.bump();
                },
            }
        }
    }
}

#[component]
fn Advanced(store: Store) -> Element {
    let rev = Revision::subscribe();
    let mut confirming = use_signal(|| false);
    let b = dat0_core::about::build_info::BuildInfo::current();
    let version = format!("dat0 {} ({})", b.version, b.git_sha);
    let level = store
        .load_or_default()
        .map(|s| s.log_level)
        .unwrap_or_else(|_| LOG_LEVELS[LOG_LEVEL_FALLBACK].to_string());
    let level_label = format!("{}: {}", dat0_i18n::t("settings.advanced.log_level"), level);
    let level_store = store.clone();
    let reset_store = store.clone();

    rsx! {
        div { class: "d0-settings-fields",
            Blurb { text_key: "settings.advanced.placeholder" }
            div {
                class: "d0-mono",
                "data-a11y-id": "adv-version",
                role: AccessRole::Label.aria(),
                "aria-label": "{version}",
                "{version}"
            }
            button {
                class: "d0-btn",
                "data-a11y-id": "adv-open-logs",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("settings.advanced.open_logs"),
                onclick: move |_| {
                    if let Ok(d) = dat0_core::platform::cache_dir() {
                        open_url(&d.to_string_lossy());
                    }
                },
                {dat0_i18n::t("settings.advanced.open_logs")}
            }
            button {
                class: "d0-btn",
                "data-a11y-id": "adv-reveal-config",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("settings.advanced.reveal_config"),
                onclick: move |_| {
                    if let Ok(d) = dat0_core::platform::config_dir() {
                        open_url(&d.to_string_lossy());
                    }
                },
                {dat0_i18n::t("settings.advanced.reveal_config")}
            }
            button {
                class: "d0-btn",
                "data-a11y-id": "adv-log-level",
                role: AccessRole::Button.aria(),
                "aria-label": "{level_label}",
                onclick: move |_| {
                    let cur = level_store
                        .load_or_default()
                        .map(|s| s.log_level)
                        .unwrap_or_else(|_| LOG_LEVELS[LOG_LEVEL_FALLBACK].to_string());
                    let _ = dat0_core::settings::set_log_level(&level_store, next_log_level(&cur));
                    rev.bump();
                },
                "{level_label}"
            }
            button {
                class: "d0-btn is-ghost",
                "data-a11y-id": "adv-reset",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("settings.advanced.reset"),
                onclick: move |_| confirming.set(true),
                {dat0_i18n::t("settings.advanced.reset")}
            }

            // Confirmation lives in this window, not in the shell's modal slot:
            // the slot belongs to the workbench window, and this window has no
            // handle on it. Resetting every setting from one click is not
            // something to do without asking.
            if confirming() {
                div { class: "d0-scrim", "data-a11y-id": "adv-reset-scrim",
                    div {
                        class: "d0-modal d0-settings-confirm",
                        "data-a11y-id": "adv-reset-confirm",
                        role: AccessRole::Dialog.aria(),
                        "aria-modal": "true",
                        "aria-label": dat0_i18n::t("settings.advanced.reset.title"),
                        h2 { class: "d0-head-title", {dat0_i18n::t("settings.advanced.reset.title")} }
                        p { class: "d0-body", {dat0_i18n::t("settings.advanced.reset.body")} }
                        div { class: "d0-settings-confirm-foot",
                            button {
                                class: "d0-btn is-ghost",
                                "data-a11y-id": "adv-reset-cancel",
                                "aria-label": dat0_i18n::t("common.cancel"),
                                onclick: move |_| confirming.set(false),
                                {dat0_i18n::t("common.cancel")}
                            }
                            button {
                                class: "d0-btn is-primary",
                                "data-a11y-id": "adv-reset-ok",
                                "aria-label": dat0_i18n::t("settings.advanced.reset.ok"),
                                onclick: move |_| {
                                    let _ = reset_store.save(&Settings::default());
                                    rev.bump();
                                    confirming.set(false);
                                },
                                {dat0_i18n::t("settings.advanced.reset.ok")}
                            }
                        }
                    }
                }
            }
        }
    }
}

fn open_url(url: &str) {
    if let Err(e) = dat0_core::platform::open_url(url) {
        tracing::warn!("settings: could not open {url}: {e}");
    }
}

/// The settings window's root component.
///
/// Carries its own stylesheet link and theme block, because a `VirtualDom` is
/// per window and inherits nothing from the workbench.
#[component]
pub fn SettingsWindow(props: SettingsProps) -> Element {
    dioxus::desktop::use_asset_handler("dat0", crate::protocol::serve);
    crate::theme::Theme::provide(Some(&props.store));
    rsx! {
        crate::theme::ThemeStyle {}
        SettingsPanel { store: props.store.clone(), events: props.events.clone() }
    }
}

/// The window itself: 720×560, decorated, titled, its own `VirtualDom`.
///
/// Decorated, unlike the workbench window — settings has no custom titlebar to
/// draw, and `Config::with_window` would clear the menu if it were not.
fn window_config() -> dioxus::desktop::Config {
    use dioxus::desktop::tao::dpi::LogicalSize;
    use dioxus::desktop::{Config, WindowBuilder};
    Config::new()
        .with_window(
            WindowBuilder::new()
                .with_title(dat0_i18n::t("settings.window.title"))
                .with_inner_size(LogicalSize::new(720.0, 560.0))
                .with_min_inner_size(LogicalSize::new(560.0, 400.0)),
        )
        .with_menu(crate::menu::build())
}

/// Open the settings window. The shell's action router calls this for
/// `ids::SETTINGS_OPEN`.
///
/// The store is opened on the real config path here rather than taken as an
/// argument: settings are per install, and a window that edited some other
/// store would silently write nowhere the app reads.
pub async fn open_settings_window(
    events: AppEvents,
) -> Option<dioxus::desktop::tao::window::WindowId> {
    let dir = match dat0_core::platform::config_dir() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("settings: no config dir, cannot open the window: {e:#}");
            return None;
        }
    };
    let props = SettingsProps {
        store: Store::open(dir.join("settings.toml")),
        events: Bus(Some(events)),
    };
    let dom = VirtualDom::new_with_props(SettingsWindow, props);
    let pending = dioxus::desktop::window()
        .new_window(dom, window_config())
        .await;
    Some(pending.window.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_order_is_the_gpui_order() {
        // The order users learned. Changing it is a decision, not a diff.
        let ids: Vec<&str> = SECTIONS.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            [
                "profile",
                "theme",
                "memory_budget",
                "motherduck",
                "ai",
                "telemetry",
                "workspace",
                "updates",
                "advanced"
            ]
        );
    }

    #[test]
    fn every_section_label_resolves() {
        // `t()` echoes a missing key straight into the sidebar.
        for s in SECTIONS {
            assert_ne!(dat0_i18n::t(s.name_key), s.name_key, "{}", s.id);
        }
    }

    #[test]
    fn the_theme_cycle_wraps_and_tolerates_an_unknown_id() {
        let mut seen = Vec::new();
        let mut cur = BUILTIN_IDS[0];
        for _ in 0..BUILTIN_IDS.len() {
            seen.push(cur);
            cur = next_theme(cur);
        }
        assert_eq!(cur, BUILTIN_IDS[0], "the cycle wraps");
        assert_eq!(seen.len(), BUILTIN_IDS.len());
        // A theme file that no longer exists must not strand the button.
        assert_eq!(next_theme("solarized"), BUILTIN_IDS[1]);
    }

    #[test]
    fn the_log_level_cycle_starts_from_the_default_when_unrecognised() {
        assert_eq!(next_log_level("error"), "warn");
        assert_eq!(next_log_level("debug"), "error");
        // Hand-edited settings.toml: resume from the shipped default.
        assert_eq!(next_log_level("trace,hyper=off"), "debug");
    }
}
