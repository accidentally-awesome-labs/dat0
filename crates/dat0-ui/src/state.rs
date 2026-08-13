//! The window's shared state.
//!
//! One struct of signals, provided at the root and read by every surface. It is
//! the Dioxus replacement for `WorkspaceShell`'s 60-odd fields: the same data,
//! but each piece independently subscribable, so a status-bar tick does not
//! re-render the grid.
//!
//! Everything here is **UI state**. Anything that is a fact about the data —
//! the catalog tree, the transform stack, the dock schema — lives in
//! `dat0-core` and is merely *held* here.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::session::dock_layout::DockLayout;
use dat0_core::session::slot::SessionSlot;

use dat0_core::session::queries::{HistoryEntry, SavedQuery};
use dat0_core::telemetry::crash::StagedCrash;

use crate::components::ai::AiController;
use crate::components::connections::Connections;
use crate::components::import_wizard::WizardModel;
use crate::components::modals::ModalReply;
use crate::components::update_ui::UpdateState;
use crate::components::workspace_in_use::InUse;

/// The design's default sidebar width (S1).
pub const SIDEBAR_WIDTH: u32 = 238;
/// The design's default right-column width (S5).
pub const RIGHT_WIDTH: u32 = 320;
/// The design's default console height (S4).
pub const BOTTOM_HEIGHT: u32 = 260;

/// One workspace tab.
#[derive(Clone, PartialEq, Debug)]
pub struct TabView {
    /// The DuckDB table backing the tab.
    pub table: String,
    /// Source file, when the tab came from one. Drives the S8 swatch.
    pub path: Option<PathBuf>,
}

impl TabView {
    /// The tab's display title: the file's stem when it has one, else the table.
    pub fn title(&self) -> &str {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(&self.table)
    }
}

/// What the status bar reports (S6).
///
/// Plain values, pushed by whoever knows them, rather than callbacks that
/// reach back into the engine: the status bar must never be able to block on
/// or fail because of a query.
#[derive(Clone, PartialEq, Debug)]
pub struct Status {
    /// False when the engine session failed — the dot goes red and stops
    /// pulsing.
    pub engine_ok: bool,
    /// Configured memory budget, MB.
    pub mem_mb: u64,
    /// The visible row window, 1-based and inclusive, and the total.
    pub rows: Option<(u64, u64, u64)>,
    /// Frames per second from the existing frame clock.
    pub fps: u32,
    /// Bytes sent off-device this session. Zero unless a cloud connection or
    /// the AI panel is in use, and shown always so that is visible.
    pub egress: u64,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            engine_ok: true,
            mem_mb: 0,
            rows: None,
            fps: 0,
            egress: 0,
        }
    }
}

/// Which modal is showing. At most one, ever — see `components::modals`.
///
/// Each variant carries what its panel needs to render, and — where the panel
/// produces a decision the shell has to act on — a [`ModalReply`] the opener
/// supplies. That mirrors the GPUI originals, which took their outcome as a
/// callback from the caller (`workspace_in_use_modal::open_conflict_dialog`
/// takes `on_open_anyway`, the export dialog's `Export` event is routed by
/// `window/data_io.rs`); the slot never invents a flow it cannot reach.
///
/// Re-setting the slot to the *same* variant with new data is a diff, not a
/// remount, so a panel that steps (the onboarding tour, the export dialog
/// gaining a destination) keeps its internal state.
/// `Debug` is by variant name only ([`Modal`]'s payloads include a
/// half-typed API key and a staged crash report; a log line is not the place
/// for either), and several panels' dependency bundles are not `Debug`
/// anyway.
#[derive(Clone, PartialEq)]
pub enum Modal {
    /// Version summary, and the Download button when an update is known.
    About {
        /// The newer version string, if the update check found one.
        newer: Option<String>,
        /// Whether the check actually ran; `false` renders neither outcome.
        check_latest: bool,
    },
    /// The first-run tour. Bare: the carousel's step is the component's own
    /// state, so driving it through the slot would remount and reset it.
    Onboarding,
    /// The shared single-line prompt. `reply` receives
    /// [`ModalOutcome::Named`] on confirm.
    NamePrompt {
        title: String,
        initial: String,
        placeholder: Option<String>,
        confirm_label: Option<String>,
        /// Render the field as a password — API keys and tokens.
        secret: bool,
        reply: ModalReply,
    },
    /// `destination` is `None` until the opener's directory picker answers
    /// [`ModalOutcome::BrowseDestination`] and re-sets the slot.
    Export {
        destination: Option<PathBuf>,
        reply: ModalReply,
    },
    Connections {
        state: Signal<Connections>,
        reply: ModalReply,
    },
    /// `staged` is `Some` after a crashed run, `None` for menu → Report a Bug.
    CrashReport {
        staged: Option<StagedCrash>,
        data_dir: PathBuf,
    },
    WorkspaceInUse {
        kind: InUse,
        reply: ModalReply,
    },
    LiveRefresh {
        dropped_edits: usize,
        dropped_deletes: usize,
        reply: ModalReply,
    },
    SavedQueries {
        queries: Vec<SavedQuery>,
        reply: ModalReply,
    },
    QueryLibrary {
        entries: Vec<HistoryEntry>,
        reply: ModalReply,
    },
    ImportWizard {
        model: Signal<WizardModel>,
        reply: ModalReply,
    },
    Recovery {
        scratch_root: PathBuf,
        recent_roots: Vec<PathBuf>,
        reply: ModalReply,
    },
    /// The AI provider panel. Phase 6 moves it out of the left dock, which no
    /// longer exists, and into the slot.
    ///
    /// The controller is cloned in, and its state is `Signal`s, so the panel
    /// survives being displaced: when it asks for a key through
    /// `AiState::entry`, the opener puts a [`Modal::NamePrompt`] in the slot
    /// and then puts this same `Modal::Ai` back, with every field intact.
    Ai {
        controller: AiController,
    },
    /// The updater's prompt.
    Update {
        state: UpdateState,
        /// True when the user chose "Check for Updates" themselves; a
        /// background check only interrupts for an actual update.
        is_manual: bool,
        reply: ModalReply,
    },
}

impl std::fmt::Debug for Modal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Modal::{}", crate::components::modals::slug(self))
    }
}

/// The window's shared state.
///
/// `Copy`, because every field is a `Signal` — passing it into a closure costs
/// nothing and needs no clone dance.
#[derive(Clone, Copy)]
pub struct Workspace {
    /// Workspace name, shown in the titlebar.
    pub name: Signal<String>,
    /// Open tabs, in strip order.
    pub tabs: Signal<Vec<TabView>>,
    /// Index into `tabs`. `None` = the empty state.
    pub active: Signal<Option<usize>>,
    /// Whether this workspace was opened read-only.
    pub read_only: Signal<bool>,
    /// A live (non-file) source is attached, so the titlebar pill pulses.
    pub live: Signal<bool>,
    /// Dock geometry and the sidebar's own state, persisted.
    pub layout: Signal<DockLayout>,
    /// Status-bar telemetry.
    pub status: Signal<Status>,
    /// The single modal slot.
    pub modal: Signal<Option<Modal>>,
    /// Whether the command palette is open.
    pub palette: Signal<bool>,
    /// A file drag is over the window.
    pub drag_over: Signal<bool>,
    /// This window's DuckDB session.
    ///
    /// A slot, not an `Arc<Mutex<Session>>`: `Session::new` opens DuckDB, runs
    /// its `PRAGMA`s and applies migrations, and doing that before the first
    /// frame is what forced the GPUI build's `block_on` on the UI thread. The
    /// window opens `Booting` and the session lands when it is built.
    ///
    /// `Signal<Arc<…>>` rather than the bare slot so the value stays `Clone`:
    /// `SessionSlot` holds an `Arc<Mutex<Session>>`, which is cheap to clone,
    /// but the enum itself is not `Clone` because `Session` is not.
    pub session: Signal<Arc<SessionSlot>>,
    /// Paths dropped while the session was still opening.
    ///
    /// A drop is a gesture the user already made; swallowing it because DuckDB
    /// had not finished its `PRAGMA`s yet is the defect EN4 fixed under GPUI
    /// and the reason the window is allowed to exist before its session does.
    /// [`crate::session_boot::open_paths`] appends here whenever the slot is
    /// not `Ready`, and the boot drains it front-to-back the moment it is — so
    /// two drops land as two tabs in the order they were made, with the last
    /// one active, exactly as if the session had been ready all along.
    pub pending_open: Signal<Vec<PathBuf>>,
    /// This window's stable id, minted before the session so it exists in
    /// every slot state.
    pub window_id: uuid::Uuid,
}

impl Workspace {
    /// Create and provide the workspace to the tree below.
    pub fn provide() -> Self {
        let ws = Self {
            name: Signal::new("scratch".into()),
            tabs: Signal::new(Vec::new()),
            active: Signal::new(None),
            read_only: Signal::new(false),
            live: Signal::new(false),
            layout: Signal::new(DockLayout::default()),
            status: Signal::new(Status::default()),
            modal: Signal::new(None),
            palette: Signal::new(false),
            drag_over: Signal::new(false),
            session: Signal::new(Arc::new(SessionSlot::Booting)),
            pending_open: Signal::new(Vec::new()),
            window_id: uuid::Uuid::now_v7(),
        };
        use_context_provider(|| ws)
    }

    /// Read the workspace provided above.
    pub fn use_current() -> Self {
        use_context()
    }

    /// The sidebar's width in pixels, or 0 when collapsed.
    ///
    /// `window_w` is the window's width, and it is what makes a restored size
    /// safe. A layout saved on a 4K display and reopened on a laptop would
    /// otherwise mount a sidebar wider than the window: the centre gets zero,
    /// the splitter sits off screen, and the only way back is ⌘B. The drag
    /// path clamps to the same band (`components::dock::resized`), so a size
    /// is in band on the way out and on the way back in.
    pub fn sidebar_px(&self, window_w: f64) -> u32 {
        let l = self.layout.read();
        if l.sidebar_open {
            mounted(l.sidebar_size, SIDEBAR_WIDTH, window_w)
        } else {
            0
        }
    }

    /// The right column's width in pixels, or 0 when both its panes are closed.
    ///
    /// S5: the column is not a reserved split — when nothing is in it, the grid
    /// takes the space.
    pub fn right_px(&self, window_w: f64) -> u32 {
        let l = self.layout.read();
        if l.right_open() {
            mounted(l.right_size, RIGHT_WIDTH, window_w)
        } else {
            0
        }
    }

    /// The console's height in pixels, or 0 when closed.
    pub fn bottom_px(&self, window_h: f64) -> u32 {
        let l = self.layout.read();
        if l.console_open {
            mounted(l.bottom_size, BOTTOM_HEIGHT, window_h)
        } else {
            0
        }
    }

    /// Toggle the sidebar (⌘B).
    pub fn toggle_sidebar(&mut self) {
        let open = self.layout.read().sidebar_open;
        self.layout.write().sidebar_open = !open;
    }

    /// Toggle one sidebar section's collapse state.
    pub fn toggle_section(&mut self, section: &str) {
        let mut l = self.layout.write();
        if !l.sections_collapsed.remove(section) {
            l.sections_collapsed.insert(section.to_string());
        }
    }

    /// Whether a sidebar section is collapsed.
    pub fn section_collapsed(&self, section: &str) -> bool {
        self.layout.read().sections_collapsed.contains(section)
    }

    /// The active tab, if any.
    pub fn active_tab(&self) -> Option<TabView> {
        let i = (*self.active.read())?;
        self.tabs.read().get(i).cloned()
    }
}

/// A persisted dock size resolved into the pixels to mount with.
///
/// Whole pixels out, because that is what a CSS grid track wants and what
/// `DockLayout` stores; the clamp itself is `dat0_core`'s, shared with the
/// splitter drag so both ends of the round trip use one band.
fn mounted(persisted: Option<u32>, default_px: u32, axis_extent: f64) -> u32 {
    dat0_core::session::dock_layout::clamped_size(persisted, default_px as f32, axis_extent as f32)
        .round() as u32
}

/// Collapsed sections are stored by name; these are the three the shell has.
pub const SECTION_FILES: &str = "files";
pub const SECTION_CONNECTIONS: &str = "connections";
pub const SECTION_PACKAGES: &str = "packages";

/// Every section, in display order.
pub const SECTIONS: [&str; 3] = [SECTION_FILES, SECTION_CONNECTIONS, SECTION_PACKAGES];

/// Sections whose collapse state is worth persisting, as a fresh set.
pub fn no_sections() -> BTreeSet<String> {
    BTreeSet::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_titles_itself_from_its_file_then_its_table() {
        let from_file = TabView {
            table: "t_1".into(),
            path: Some(PathBuf::from("/data/sales.csv")),
        };
        assert_eq!(from_file.title(), "sales.csv");

        let from_query = TabView {
            table: "results".into(),
            path: None,
        };
        assert_eq!(from_query.title(), "results");
    }

    #[test]
    fn a_closed_surface_takes_no_width() {
        // S5's whole point: a closed right column is not a reserved split, so
        // the grid gets the pixels back.
        let mut l = DockLayout::default();
        assert!(!l.right_open());
        l.inspector_visible = true;
        assert!(l.right_open());
    }

    #[test]
    fn the_sidebar_defaults_open_at_the_design_width() {
        let l = DockLayout::default();
        assert!(l.sidebar_open);
        assert_eq!(l.sidebar_size.unwrap_or(SIDEBAR_WIDTH), SIDEBAR_WIDTH);
    }

    #[test]
    fn a_size_restored_from_a_bigger_display_is_clamped_at_mount() {
        // A 4K sidebar reopened in a 1440px window. Mounting it verbatim
        // leaves the centre at zero width with the splitter off screen and
        // ⌘B the only way back.
        assert_eq!(mounted(Some(30_000), SIDEBAR_WIDTH, 1440.0), 1152);
        // In band, so untouched.
        assert_eq!(mounted(Some(291), SIDEBAR_WIDTH, 1440.0), 291);
        // Absent, so the design's own width.
        assert_eq!(mounted(None, SIDEBAR_WIDTH, 1440.0), SIDEBAR_WIDTH);
    }

    #[test]
    fn a_window_with_no_measured_extent_still_mounts() {
        // `f32::clamp` panics when min > max, and a window reports a zero
        // extent before its first layout pass.
        assert_eq!(mounted(Some(291), SIDEBAR_WIDTH, 0.0), 291);
        assert_eq!(mounted(Some(291), SIDEBAR_WIDTH, f64::NAN), 291);
    }
}
