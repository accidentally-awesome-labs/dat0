//! Type-aware inline cell editor (P4b T6).
//!
//! This module owns [`CellEditor`] — the GPUI [`Entity`] that mounts the
//! type-appropriate `gpui-component` widget for editing a single grid cell and
//! emits a typed [`dat0_engine::Scalar`] on commit.
//!
//! # Mount pattern (P4a T10b / T0)
//!
//! `InputState::new` and `SelectState::new` both require a real `&mut Window`,
//! which is unavailable in headless test environments (T0 spike §3). The widget
//! state entities are therefore initialised **lazily on the first `render()`
//! call**, mirroring [`crate::view::filter_popover_entity::FilterPopoverEntity`]
//! and `WorkspaceShell`'s `TableState` promotion.
//!
//! # Subscription-storage trap
//!
//! Widget-event subscriptions (`cx.subscribe_in`) MUST be stored in a field, or
//! GPUI deregisters the callback when the returned `Subscription` is dropped and
//! the widget silently never fires its events (the P4a T10b post-review trap; T0
//! hit it too). They live in `self._subscriptions`.
//!
//! # Type → widget map (design §8 / T6)
//!
//! | [`ColumnType`]      | Widget        | Parse → [`Scalar`]                       |
//! |---------------------|---------------|------------------------------------------|
//! | `String`            | text `Input`  | [`Scalar::Str`]                          |
//! | `Numeric`           | text `Input`  | `i64` parse → [`Scalar::Int`]; else `f64` parse → [`Scalar::Float`] |
//! | `Date`              | text `Input`  | [`Scalar::validate_date`] → [`Scalar::Date`]       |
//! | `Timestamp`         | text `Input`  | [`Scalar::validate_timestamp`] → [`Scalar::Timestamp`] |
//! | `Bool`              | `Select`      | true/false item → [`Scalar::Bool`]       |
//!
//! Invalid input (un-parseable numeric, malformed date/timestamp) is REJECTED:
//! the commit is suppressed and the widget keeps focus so the user can fix it.
//! A `Scalar::Int(…)` is never built from `"abc"`.
//!
//! # Commit / cancel / focus (P4c T14)
//!
//! Text inputs commit on **Enter** ([`InputEvent::PressEnter`]) and on
//! **focus-out** ([`InputEvent::Blur`]); the bool select commits on
//! [`SelectEvent::Confirm`]. A Cancel button emits [`CellEditorEvent::Cancel`].
//! `WorkspaceShell::begin_cell_edit` subscribes to these events (storing the
//! subscription) and routes `Commit` → `WorkspaceShell::commit_cell_edit`.
//!
//! P4c T14 gives the editor a real [`gpui::FocusHandle`] (lazily built in
//! `ensure_widgets`, exposed via [`CellEditor::focus_handle`]) and focuses the
//! **inner** `InputState` / `SelectState` on mount, so the user types
//! immediately with no click. **Enter** commits *and advances* — it emits
//! [`CellEditorEvent::CommitAndMove`]`(value, `[`EditorAdvance::Down`]`)` so the
//! shell moves the active cell down a row and re-opens the editor (spreadsheet
//! "Enter walks down the column"); **focus-out (Blur)** commits in place without
//! advancing.
//!
//! **`Tab` → move RIGHT is PD-020 (NOT shipped).** The gpui-component `Input`
//! consumes Tab for its own tab-stop navigation and does not surface it as an
//! [`InputEvent`] variant (the enum is only `Change` / `PressEnter` / `Focus` /
//! `Blur`). Intercepting Tab would require a competing wrapper-level
//! `on_key_down` handler that holds focus — which would steal keystrokes from
//! the inner input. The safe subset (FocusHandle + focus-on-mount + Enter→down)
//! is shipped; Tab→right is deferred rather than ship broken focus.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, prelude::*,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    select::{Select, SelectEvent, SelectItem, SelectState},
    v_flex,
};

use crate::a11y::A11yExt as _;
use crate::view::filter_popover::ColumnType;
use dat0_engine::Scalar;

// ---------------------------------------------------------------------------
// HeaderRenameEditor + HeaderRenameEvent
// ---------------------------------------------------------------------------

/// Terminal signal emitted by the column-header rename editor.
///
/// `WorkspaceShell::begin_column_rename` subscribes (storing the
/// `Subscription`) and routes `Commit` → `WorkspaceShell::commit_column_rename`.
#[derive(Debug, Clone)]
pub enum HeaderRenameEvent {
    /// User committed the new name (Enter / focus-out). Carries the raw text.
    Commit(String),
    /// User dismissed the editor without committing.
    Cancel,
}

/// Lightweight inline text editor for renaming a column header (P4c T7).
///
/// Construct with [`HeaderRenameEditor::new`] seeded with the current display
/// label. The `InputState` is initialised lazily on the first `render()` call
/// (mirrors [`CellEditor`] and `FilterPopoverEntity`). Subscribe to
/// [`HeaderRenameEvent`]; the owner must **store** the subscription.
pub struct HeaderRenameEditor {
    /// Pre-populated text (the current display label).
    seed: String,
    /// Lazily-initialised `InputState`. `None` until first `render`.
    text: Option<Entity<InputState>>,
    /// Widget-event subscription kept alive here — a dropped `Subscription`
    /// deregisters the callback silently (P4a T10b trap).
    _subscription: Option<Subscription>,
}

impl HeaderRenameEditor {
    /// Construct an editor seeded with the current display label.
    pub fn new(seed: impl Into<String>) -> Self {
        Self {
            seed: seed.into(),
            text: None,
            _subscription: None,
        }
    }

    /// Ensure the `InputState` entity exists. Called at the start of every
    /// `render()` so the first frame initialises it with a real `Window`.
    fn ensure_input(&mut self, cx: &mut Context<Self>, window: &mut Window) {
        if self.text.is_some() {
            return;
        }
        let seed = self.seed.clone();
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("column name");
            if !seed.is_empty() {
                s = s.default_value(seed);
            }
            s
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |_this, inp, ev: &InputEvent, _window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    let text = inp.read(cx).value().to_string();
                    cx.emit(HeaderRenameEvent::Commit(text));
                }
            },
        );
        self._subscription = Some(sub);
        self.text = Some(input);
    }
}

impl EventEmitter<HeaderRenameEvent> for HeaderRenameEditor {}

impl Render for HeaderRenameEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_input(cx, window);
        let text = self.text.as_ref().expect("ensure_input just ran");

        let entity_cancel = cx.entity();
        // Icon, not the word: a ghost button labelled "Cancel" is visibly wider
        // than the ✕ it replaces, and this slice must not move layout.
        let cancel_btn = Button::new("header-rename-cancel")
            .icon(gpui_component::IconName::Close)
            .ghost()
            .a11y_label(
                crate::a11y::AccessRole::Label,
                dat0_i18n::t("common.cancel"),
            )
            .on_click(move |_ev, _window, cx| {
                entity_cancel.update(cx, |_this, cx| {
                    cx.emit(HeaderRenameEvent::Cancel);
                });
            });

        h_flex()
            .gap_1()
            .p_1()
            .min_w(gpui::px(120.))
            .child(Input::new(text).appearance(true))
            .child(cancel_btn)
    }
}

// ---------------------------------------------------------------------------
// CellEditorEvent
// ---------------------------------------------------------------------------

/// Direction the active cell advances after an Enter commit (P4c T14).
///
/// Only `Down` is wired: `Enter` commits then walks one row down the column
/// (spreadsheet semantics). `Tab` → RIGHT is **PD-020** — the gpui-component
/// `Input` does not surface a Tab keystroke as an [`InputEvent`], and adding a
/// competing wrapper-level key handler would steal focus from the inner input
/// (so typing would land nowhere). See the module-level note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorAdvance {
    /// Move the active cell down one row and re-open the editor there.
    Down,
}

/// Terminal signal emitted by the cell editor. `WorkspaceShell::begin_cell_edit`
/// subscribes (storing the `Subscription`) and routes `Commit` →
/// `WorkspaceShell::commit_cell_edit`.
#[derive(Debug, Clone)]
pub enum CellEditorEvent {
    /// The user committed a valid, typed value (focus-out / bool select).
    Commit(Scalar),
    /// The user committed a valid, typed value with **Enter** and the editor
    /// should advance to the next cell (P4c T14). Carries the value + the
    /// advance direction. The shell commits, moves the active cell, and re-opens
    /// the editor on the new cell.
    CommitAndMove(Scalar, EditorAdvance),
    /// The user dismissed the editor without committing.
    Cancel,
}

// ---------------------------------------------------------------------------
// BoolItem — SelectItem wrapper for the boolean toggle
// ---------------------------------------------------------------------------

/// Thin wrapper so `bool` satisfies the [`SelectItem`] bound (the trait is not
/// implemented for `bool` upstream — only for string-likes).
#[derive(Clone, Debug, PartialEq)]
struct BoolItem(bool);

impl SelectItem for BoolItem {
    type Value = bool;

    fn title(&self) -> SharedString {
        if self.0 { "true" } else { "false" }.into()
    }

    fn value(&self) -> &bool {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// CellEditor
// ---------------------------------------------------------------------------

/// GPUI entity that mounts the type-appropriate editor widget for one grid cell.
///
/// Construct with [`CellEditor::new`] (no `Window` needed — widget state is
/// deferred to the first `render()`). Subscribe via the emitted
/// [`CellEditorEvent`]; the owner (`WorkspaceShell`) must **store** the
/// subscription.
pub struct CellEditor {
    /// Coarse column type that selects the widget + parse path.
    column_type: ColumnType,
    /// Optional seed text used to pre-populate the text input (the current cell
    /// value). T6 mounts empty; a later polish task can seed it.
    seed: String,
    /// Lazily-initialised widget-state handles (created on first `render`).
    widgets: Option<EditorWidgets>,
    /// GPUI subscription handles for the widget events. Kept alive here — a
    /// dropped `Subscription` deregisters the callback silently (P4a T10b).
    _subscriptions: Vec<Subscription>,
    /// Focus handle for the editor wrapper (P4c T14). Built lazily on first
    /// access / render (it needs a `cx`, and `new` is `cx`-free to keep the
    /// lazy-mount construction Window-free). Exposed via
    /// [`CellEditor::focus_handle`] (the compile-level smoke guard) and tracked
    /// by the rendered container so the editor participates in the focus tree.
    /// The inner `InputState` / `SelectState` is what actually receives
    /// keystrokes — [`CellEditor::focus`] / first-render focusing forwards to it.
    focus_handle: Option<FocusHandle>,
    /// `true` until the editor has focused its inner widget once (P4c T14).
    /// `focus()` may be called before the first `render()` builds the inner
    /// widget (the lazy-mount pattern), so we set this flag and consume it in
    /// `ensure_widgets` to focus the real `InputState` / `SelectState` the moment
    /// it exists — so the editor takes focus on mount with no click required.
    focus_on_render: bool,
}

/// Lazily-initialised widget-state handles. Exactly one of `text` / `bool` is
/// `Some`, selected by [`CellEditor::column_type`].
struct EditorWidgets {
    /// Text input for String / Numeric / Date / Timestamp columns.
    text: Option<Entity<InputState>>,
    /// Boolean select for Boolean columns.
    boolean: Option<Entity<SelectState<Vec<BoolItem>>>>,
}

impl CellEditor {
    /// Construct an editor for a column of `column_type`.
    ///
    /// Widget state is **not** initialised here (it needs `&mut Window`); the
    /// first `render()` call builds it.
    pub fn new(column_type: ColumnType) -> Self {
        Self {
            column_type,
            seed: String::new(),
            widgets: None,
            _subscriptions: Vec::new(),
            focus_handle: None,
            // Take focus on mount (P4c T14): the editor is only ever constructed
            // in response to an explicit Enter/F2, so it should grab the cursor.
            focus_on_render: true,
        }
    }

    /// Construct an editor pre-populated with `seed` text (the current cell
    /// value). Used by the polish path that seeds the editor from the live cell.
    pub fn with_seed(column_type: ColumnType, seed: impl Into<String>) -> Self {
        Self {
            column_type,
            seed: seed.into(),
            widgets: None,
            _subscriptions: Vec::new(),
            focus_handle: None,
            focus_on_render: true,
        }
    }

    /// Return (lazily building) the editor's wrapper [`FocusHandle`] (P4c T14).
    ///
    /// The handle is created on first access because `new` is `cx`-free (the
    /// lazy-mount discipline — widget state needs `&mut Window`, unavailable
    /// headlessly). This accessor is the compile-level smoke guard the T14 test
    /// asserts against; it also lets the render container `track_focus` the
    /// editor.
    pub fn focus_handle(&mut self, cx: &mut gpui::App) -> FocusHandle {
        self.focus_handle
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Request that the editor focus its inner input on the next render (P4c
    /// T14). Safe to call before the first render builds the widget — the flag
    /// is consumed in `ensure_widgets` to focus the real `InputState` /
    /// `SelectState` the moment it exists, so the editor takes focus on mount
    /// with no click required.
    pub fn focus(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.focus_on_render = true;
    }

    /// Live subscription count. Non-zero after the first render once
    /// `ensure_widgets` has wired the widget-event subscriptions. Exposed only
    /// for structural tests (guards against the `let _ = cx.subscribe(...)`
    /// regression).
    #[doc(hidden)]
    pub fn subscription_count(&self) -> usize {
        self._subscriptions.len()
    }

    /// Parse a text input's raw value into a typed [`Scalar`] for the editor's
    /// column type. Returns `None` when the value is invalid (un-parseable
    /// numeric, malformed date/timestamp) — the commit is then suppressed.
    ///
    /// Pure + window-free so it is unit-testable headlessly.
    pub fn parse_text(column_type: ColumnType, raw: &str) -> Option<Scalar> {
        match column_type {
            ColumnType::String => Some(Scalar::Str(raw.to_string())),
            ColumnType::Numeric => {
                let trimmed = raw.trim();
                if let Ok(i) = trimmed.parse::<i64>() {
                    Some(Scalar::Int(i))
                } else if let Ok(f) = trimmed.parse::<f64>() {
                    Some(Scalar::Float(f))
                } else {
                    // Reject non-numeric input — never build Scalar::Int("abc").
                    None
                }
            }
            ColumnType::Date => Scalar::validate_date(raw.trim())
                .ok()
                .map(|s| Scalar::Date(s.to_string())),
            ColumnType::Timestamp => Scalar::validate_timestamp(raw.trim())
                .ok()
                .map(|s| Scalar::Timestamp(s.to_string())),
            // Bool columns never go through the text path (they use Select).
            ColumnType::Bool => match raw.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "t" | "yes" => Some(Scalar::Bool(true)),
                "false" | "0" | "f" | "no" => Some(Scalar::Bool(false)),
                _ => None,
            },
        }
    }

    /// Ensure widget state entities exist. Called at the start of every
    /// `render()` so the first frame initialises them with a real `Window` and
    /// wires (and stores) the widget-event subscriptions.
    fn ensure_widgets(&mut self, cx: &mut Context<Self>, window: &mut Window) {
        if self.widgets.is_some() {
            return;
        }

        // Build the wrapper focus handle now that a `cx` is available (P4c T14);
        // `new` is `cx`-free, so this is the first point one can exist.
        self.focus_handle.get_or_insert_with(|| cx.focus_handle());

        if self.column_type == ColumnType::Bool {
            let items = vec![BoolItem(true), BoolItem(false)];
            let boolean = cx.new(|cx| SelectState::new(items, None, window, cx));
            let sub = cx.subscribe_in(
                &boolean,
                window,
                |this, _select, ev: &SelectEvent<Vec<BoolItem>>, _window, cx| {
                    let SelectEvent::Confirm(maybe_val) = ev;
                    if let Some(b) = maybe_val {
                        this.emit_commit(Scalar::Bool(*b), cx);
                    }
                },
            );
            self._subscriptions.push(sub);
            // Focus the boolean select on mount (P4c T14) so the dropdown is
            // ready for keyboard interaction without a click.
            if self.focus_on_render {
                boolean.update(cx, |state, cx| state.focus(window, cx));
                self.focus_on_render = false;
            }
            self.widgets = Some(EditorWidgets {
                text: None,
                boolean: Some(boolean),
            });
            return;
        }

        let seed = self.seed.clone();
        let placeholder = match self.column_type {
            ColumnType::Numeric => "number",
            ColumnType::Date => "yyyy-mm-dd",
            ColumnType::Timestamp => "yyyy-mm-dd hh:mm:ss",
            _ => "value",
        };
        let text = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(placeholder);
            if !seed.is_empty() {
                s = s.default_value(seed);
            }
            s
        });

        // Commit on Enter and on focus-out (Blur); both go through the typed
        // parse so invalid input is rejected (the commit is simply suppressed).
        //
        // P4c T14: Enter advances (commit + move DOWN one row, re-open editor);
        // focus-out (Blur) commits in place WITHOUT advancing — a click away
        // should land the value but not walk the cursor.
        let sub = cx.subscribe_in(
            &text,
            window,
            |this, input, ev: &InputEvent, _window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    let raw = input.read(cx).value().to_string();
                    if let Some(scalar) = Self::parse_text(this.column_type, &raw) {
                        this.emit_commit_and_move(scalar, EditorAdvance::Down, cx);
                    }
                    // Invalid → no emit; the widget keeps its value so the user
                    // can correct it (or Cancel).
                }
                InputEvent::Blur => {
                    let raw = input.read(cx).value().to_string();
                    if let Some(scalar) = Self::parse_text(this.column_type, &raw) {
                        this.emit_commit(scalar, cx);
                    }
                }
                _ => {}
            },
        );
        self._subscriptions.push(sub);
        // Focus the text input on mount (P4c T14) so the user can type
        // immediately without clicking the field.
        if self.focus_on_render {
            text.update(cx, |state, cx| state.focus(window, cx));
            self.focus_on_render = false;
        }
        self.widgets = Some(EditorWidgets {
            text: Some(text),
            boolean: None,
        });
    }

    /// Emit a commit event with the typed value.
    fn emit_commit(&self, value: Scalar, cx: &mut Context<Self>) {
        cx.emit(CellEditorEvent::Commit(value));
    }

    /// Emit a commit-and-advance event with the typed value + direction (P4c
    /// T14). The shell commits, moves the active cell, and re-opens the editor.
    fn emit_commit_and_move(&self, value: Scalar, advance: EditorAdvance, cx: &mut Context<Self>) {
        cx.emit(CellEditorEvent::CommitAndMove(value, advance));
    }
}

impl EventEmitter<CellEditorEvent> for CellEditor {}

impl Render for CellEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_widgets(cx, window);
        // `ensure_widgets` builds the focus handle on first render; clone it so
        // the container can `track_focus` (P4c T14) — the inner input still owns
        // keystroke focus, but tracking keeps the editor in the focus tree.
        let focus_handle = self
            .focus_handle
            .clone()
            .expect("ensure_widgets builds the focus handle");
        let widgets = self.widgets.as_ref().expect("ensure_widgets just ran");

        let field: gpui::AnyElement = if let Some(boolean) = widgets.boolean.as_ref() {
            Select::new(boolean)
                .placeholder("true / false")
                .into_any_element()
        } else if let Some(text) = widgets.text.as_ref() {
            Input::new(text).appearance(true).into_any_element()
        } else {
            gpui::div().into_any_element()
        };

        let entity_cancel = cx.entity();
        let cancel_btn = Button::new("cell-edit-cancel")
            .label("Cancel")
            .ghost()
            .on_click(move |_ev, _window, cx| {
                entity_cancel.update(cx, |_this, cx| {
                    cx.emit(CellEditorEvent::Cancel);
                });
            });

        v_flex()
            .track_focus(&focus_handle)
            .gap_2()
            .p_2()
            .min_w(gpui::px(180.))
            .child(field)
            .child(h_flex().gap_2().child(cancel_btn))
    }
}

#[cfg(feature = "a11y-capture")]
impl CellEditor {
    /// The column type this editor was built for. Lets a test assert the Bool
    /// column mounted the `Select` path (not a text `Input`). `ColumnType` is `Copy`.
    pub fn column_type_for_test(&self) -> ColumnType {
        self.column_type
    }

    /// Set the inner text input's value directly — the reliable headless drive for
    /// the typed characters (raw per-char keystrokes into a gpui-component `Input`
    /// are unreliable; the Settings-slice finding, which also used
    /// `InputState::set_value`). No-op when the widget is the Bool `Select` or
    /// hasn't rendered its `InputState` yet.
    pub fn set_text_value_for_test(
        &mut self,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(text) = self.widgets.as_ref().and_then(|w| w.text.clone()) {
            text.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_string_is_always_some() {
        assert_eq!(
            CellEditor::parse_text(ColumnType::String, "hello"),
            Some(Scalar::Str("hello".into()))
        );
        // Even empty / weird strings round-trip as Str.
        assert_eq!(
            CellEditor::parse_text(ColumnType::String, ""),
            Some(Scalar::Str(String::new()))
        );
    }

    #[test]
    fn parse_numeric_prefers_int_then_float_and_rejects_garbage() {
        assert_eq!(
            CellEditor::parse_text(ColumnType::Numeric, "42"),
            Some(Scalar::Int(42))
        );
        assert_eq!(
            CellEditor::parse_text(ColumnType::Numeric, "  -7 "),
            Some(Scalar::Int(-7))
        );
        assert_eq!(
            CellEditor::parse_text(ColumnType::Numeric, "2.5"),
            Some(Scalar::Float(2.5))
        );
        // Non-numeric must be rejected — never Scalar::Int("abc").
        assert_eq!(CellEditor::parse_text(ColumnType::Numeric, "abc"), None);
        assert_eq!(CellEditor::parse_text(ColumnType::Numeric, ""), None);
    }

    #[test]
    fn parse_date_validates_format() {
        assert_eq!(
            CellEditor::parse_text(ColumnType::Date, "2026-05-30"),
            Some(Scalar::Date("2026-05-30".into()))
        );
        assert_eq!(CellEditor::parse_text(ColumnType::Date, "2026/05/30"), None);
        assert_eq!(CellEditor::parse_text(ColumnType::Date, "nope"), None);
    }

    #[test]
    fn parse_timestamp_validates_format() {
        assert_eq!(
            CellEditor::parse_text(ColumnType::Timestamp, "2026-05-30 12:30:00"),
            Some(Scalar::Timestamp("2026-05-30 12:30:00".into()))
        );
        assert_eq!(
            CellEditor::parse_text(ColumnType::Timestamp, "2026-05-30"),
            None,
            "date-only must not parse as a timestamp"
        );
    }

    #[test]
    fn parse_bool_text_path_accepts_common_forms() {
        assert_eq!(
            CellEditor::parse_text(ColumnType::Bool, "true"),
            Some(Scalar::Bool(true))
        );
        assert_eq!(
            CellEditor::parse_text(ColumnType::Bool, "FALSE"),
            Some(Scalar::Bool(false))
        );
        assert_eq!(CellEditor::parse_text(ColumnType::Bool, "maybe"), None);
    }
}
