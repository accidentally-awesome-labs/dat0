//! Connections panel (design §4). Left-dock content; a pure function of
//! `ConnectionManager` state, rendered inside `WorkspaceShell`. Buttons dispatch
//! `ConnectionsEvent` to `WorkspaceShell::handle_connections_event`, which runs
//! the async engine flows (T8) and updates the manager.
//!
//! The panel is a *free function* — not a GPUI `Render`/`EventEmitter` entity —
//! because every button needs to reach `WorkspaceShell` (to spawn the async
//! connect/disconnect engine flows and mutate the manager). Rendering it inside
//! `WorkspaceShell::render` lets each `on_click` use `cx.listener(|ws, …| …)`
//! and call `ws.handle_connections_event(…)` directly, so there is no event
//! plumbing to keep alive. The render reads `manager` and nothing else, so it
//! stays a pure function of the manager's state.

use crate::a11y::A11yExt as _;
use crate::connections::{AttachmentKind, ConnectionManager, ConnectionStatus};
use crate::window::WorkspaceShell;
use gpui::prelude::*;
use gpui::{Context, SharedString, div};

/// Intent emitted by a panel button, dispatched to
/// [`WorkspaceShell::handle_connections_event`]. A plain enum (not a GPUI
/// `EventEmitter`) — the panel is a free function bound to the shell's context,
/// so each button calls the handler directly via `cx.listener`.
#[derive(Clone, Debug)]
pub enum ConnectionsEvent {
    /// Connect (or Retry from an error state). Opens the token prompt if no
    /// token is stored, otherwise spawns the async connect.
    ConnectMd,
    /// Disconnect the MotherDuck attachment.
    DisconnectMd,
    /// Forget the stored token (and disconnect).
    ForgetMd,
    /// Probe MotherDuck with the stored token and report a transient pass/fail
    /// message (design §3.1). Routes to the token prompt if no token is stored.
    TestMd,
    /// Open the native file picker to attach a SQLite file (trim-valve ②).
    AttachSqlite,
    /// Detach the attachment with the given alias.
    Detach(String),
}

/// Localized status label for the MotherDuck status pill.
pub fn status_label(s: &ConnectionStatus) -> SharedString {
    SharedString::from(match s {
        ConnectionStatus::Disconnected => dat0_i18n::t("connections.md.status.disconnected"),
        ConnectionStatus::Connecting => dat0_i18n::t("connections.md.status.connecting"),
        ConnectionStatus::Connected => dat0_i18n::t("connections.md.status.connected"),
        ConnectionStatus::Error(_) => dat0_i18n::t("connections.md.status.error"),
    })
}

/// Render the panel from the current manager state. Called from
/// `WorkspaceShell::render`. A pure function of `manager` — the only state read
/// is `manager.md_status()` and `manager.sqlite()`.
pub fn render_connections(
    manager: &ConnectionManager,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let status = manager.md_status();

    // State-driven action buttons for the MotherDuck section.
    let md_actions = match status {
        ConnectionStatus::Disconnected => div()
            .flex()
            .flex_row()
            .gap_2()
            .child(action_button(
                "connections-md-connect",
                dat0_i18n::t("connections.md.connect"),
                ConnectionsEvent::ConnectMd,
                cx,
            ))
            .child(action_button(
                "connections-md-test",
                dat0_i18n::t("connections.md.test"),
                ConnectionsEvent::TestMd,
                cx,
            )),
        ConnectionStatus::Connecting => {
            // No buttons while connecting — just the status text above.
            div()
        }
        ConnectionStatus::Connected => div()
            .flex()
            .flex_row()
            .gap_2()
            .child(action_button(
                "connections-md-disconnect",
                dat0_i18n::t("connections.md.disconnect"),
                ConnectionsEvent::DisconnectMd,
                cx,
            ))
            .child(action_button(
                "connections-md-forget",
                dat0_i18n::t("connections.md.forget"),
                ConnectionsEvent::ForgetMd,
                cx,
            ))
            .child(action_button(
                "connections-md-test",
                dat0_i18n::t("connections.md.test"),
                ConnectionsEvent::TestMd,
                cx,
            )),
        ConnectionStatus::Error(msg) => div()
            .flex()
            .flex_col()
            .gap_1()
            .a11y_label(crate::a11y::AccessRole::Label, msg.clone())
            // The localized error message carried by the status.
            .child(SharedString::from(msg.clone()))
            .child(action_button(
                "connections-md-retry",
                dat0_i18n::t("connections.md.retry"),
                ConnectionsEvent::ConnectMd,
                cx,
            )),
    };

    // Shallow catalog enumeration (design §4.3): when Connected, list the cached
    // database names indented under the MotherDuck section. Pure function of
    // `manager.md_databases()` — empty (e.g. not Connected) renders nothing.
    let mut md_databases = div().flex().flex_col().gap_1();
    if matches!(status, ConnectionStatus::Connected) {
        for name in manager.md_databases() {
            md_databases = md_databases.child(
                div()
                    .pl_4()
                    .a11y_label(crate::a11y::AccessRole::Label, name.clone())
                    .child(SharedString::from(name.clone())),
            );
        }
    }

    let md_section = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .child(div().child(SharedString::from(dat0_i18n::t("connections.md.heading"))))
        .child(div().child(status_label(status)))
        .child(md_actions)
        .child(md_databases);
    // Transient Test-connection result (design §3.1); only appended when one is pending,
    // so the parent gap_2 does not leave a phantom 8 px gap when there is no message.
    let md_section = match manager.md_test_result() {
        Some(msg) => md_section.child(
            div()
                .a11y_label(crate::a11y::AccessRole::Label, msg.to_string())
                .child(SharedString::from(msg.to_string())),
        ),
        None => md_section,
    };

    // Attached-files section: one row per sqlite attachment + an "Attach…" button.
    let mut files = div().flex().flex_col().gap_1();
    files = files.child(div().child(SharedString::from(dat0_i18n::t(
        "connections.files.heading",
    ))));
    for att in manager.sqlite() {
        let path = match &att.kind {
            AttachmentKind::Sqlite { path } => path.clone(),
            // `sqlite()` only ever yields Sqlite attachments (md is tracked
            // separately); a Md entry here means a broken invariant.
            AttachmentKind::MotherDuck => unreachable!("sqlite() only yields Sqlite attachments"),
        };
        let label = SharedString::from(format!("{} · {}", att.alias, path));
        let alias = att.alias.clone();
        files = files.child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .gap_2()
                .child(label)
                .child(action_button(
                    SharedString::from(format!("connections-detach-{alias}")),
                    dat0_i18n::t("connections.files.detach"),
                    ConnectionsEvent::Detach(alias.clone()),
                    cx,
                )),
        );
    }
    files = files.child(action_button(
        "connections-attach-sqlite",
        dat0_i18n::t("connections.files.attach"),
        ConnectionsEvent::AttachSqlite,
        cx,
    ));

    let files_section = div().flex().flex_col().gap_2().p_2().child(files);

    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        // B7: the title row moved into `ConnectionsPanel::title` (dock chrome).
        .child(md_section)
        .child(files_section)
        .into_any_element()
}

/// A clickable panel button that dispatches `ev` to the shell handler. Mirrors
/// the button idiom in `name_prompt.rs` / `sql_console.rs`
/// (`div().id(..).cursor_pointer().on_click(cx.listener(..))`).
fn action_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    ev: ConnectionsEvent,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .px_2()
        .py_1()
        .border_1()
        .cursor_pointer()
        .a11y_label(crate::a11y::AccessRole::Label, label.to_string())
        .child(label)
        .on_click(cx.listener(move |ws, _ev, window, cx| {
            ws.handle_connections_event(ev.clone(), window, cx);
        }))
}
