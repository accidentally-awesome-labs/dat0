//! The console's tab list, and the lifecycle rules that go with it.
//!
//! `view/sql_console.rs` kept `tabs` + `active` as two fields on the view and
//! spread the rules — clamp on switch, refuse to close the last one, title the
//! next one — across `new_tab`, `close_tab` and three key handlers. Pulling
//! them into one toolkit-free struct is what lets the component stay a
//! renderer: it emits an intent, the host applies it here, and there is exactly
//! one place where "what does Delete do to the last tab" is answered.
//!
//! The console is never empty. `Tabs` has no constructor that produces zero
//! tabs and [`Tabs::close_active`] refuses the last one, so `active_tab` cannot
//! fail.

use super::Tab;

/// The console's open tabs, and which one is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: usize,
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new()
    }
}

impl Tabs {
    /// One empty tab, titled `Query 1`.
    pub fn new() -> Self {
        let mut t = Self {
            tabs: Vec::new(),
            active: 0,
        };
        t.open();
        t
    }

    /// Adopt an existing list. `active` is clamped; an empty list is refused by
    /// falling back to [`Tabs::new`], because a console with no tab has no
    /// editor and no way to get one back.
    pub fn adopt(tabs: Vec<Tab>, active: usize) -> Self {
        if tabs.is_empty() {
            return Self::new();
        }
        let active = active.min(tabs.len() - 1);
        Self { tabs, active }
    }

    pub fn all(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Never true — the type maintains at least one tab. Present because
    /// `len` without `is_empty` is a lint, and answering it honestly is better
    /// than suppressing it.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn titles(&self) -> Vec<String> {
        self.tabs.iter().map(|t| t.title.clone()).collect()
    }

    /// Show tab `i`, clamped.
    pub fn select(&mut self, i: usize) {
        self.active = i.min(self.tabs.len() - 1);
    }

    /// Move the active tab by `delta`, clamping at both ends.
    pub fn step(&mut self, delta: i32) {
        self.active = step_index(self.active, delta, self.tabs.len());
    }

    /// Open an empty tab and make it active. Returns its id.
    pub fn open(&mut self) -> String {
        self.open_with(String::new())
    }

    /// Open a tab carrying `doc` and make it active. Returns its id.
    ///
    /// This is the history / saved-query load path: a picked statement never
    /// overwrites what is in front of you.
    pub fn open_with(&mut self, doc: impl Into<String>) -> String {
        let id = format!("console-{}", uuid::Uuid::now_v7());
        self.tabs.push(Tab {
            id: id.clone(),
            title: self.next_title(),
            doc: doc.into(),
        });
        self.active = self.tabs.len() - 1;
        id
    }

    /// Close the showing tab. Returns false — and changes nothing — when it is
    /// the only one, so Delete can never leave an empty console.
    pub fn close_active(&mut self) -> bool {
        if self.tabs.len() == 1 {
            return false;
        }
        self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len() - 1);
        true
    }

    /// Replace a tab's document, by id. Unknown ids are ignored: a `change`
    /// from an editor whose tab has just been closed is late, not wrong.
    pub fn set_doc(&mut self, id: &str, doc: String) {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
            t.doc = doc;
        }
    }

    /// `Query {n}`, where `n` is one past the highest number already in use.
    ///
    /// GPUI used `tabs.len() + 1`, which repeats a title as soon as a middle
    /// tab is closed — and the tab strip is the only thing distinguishing two
    /// otherwise identical editors.
    fn next_title(&self) -> String {
        let n = self
            .tabs
            .iter()
            .filter_map(|t| t.title.strip_prefix("Query "))
            .filter_map(|s| s.parse::<usize>().ok())
            .max()
            .unwrap_or(0);
        format!("Query {}", n + 1)
    }
}

/// Move an index by `delta` within `len`, clamping at both ends.
///
/// Clamping, not wrapping: a tab strip is a list. Only radio groups wrap, and
/// the grid, the history list and the command palette all clamp — a third
/// convention here would make ← at the first tab mean something different
/// depending on which surface has focus.
pub fn step_index(current: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as i64 + delta as i64;
    next.clamp(0, len as i64 - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_console_has_exactly_one_tab() {
        let t = Tabs::new();
        assert_eq!(t.titles(), vec!["Query 1"]);
        assert_eq!(t.active(), 0);
    }

    #[test]
    fn each_new_tab_is_titled_and_activated() {
        let mut t = Tabs::new();
        t.open();
        t.open();
        assert_eq!(t.titles(), vec!["Query 1", "Query 2", "Query 3"]);
        assert_eq!(t.active(), 2, "a new tab is the one you are looking at");
    }

    #[test]
    fn a_reopened_title_cannot_collide_with_a_surviving_one() {
        let mut t = Tabs::new();
        t.open();
        t.open();
        t.select(1);
        assert!(t.close_active());
        t.open();
        assert_eq!(t.titles(), vec!["Query 1", "Query 3", "Query 4"]);
    }

    #[test]
    fn stepping_clamps_at_both_ends() {
        let mut t = Tabs::new();
        t.open();
        t.open();
        assert_eq!(t.active(), 2);
        t.step(1);
        assert_eq!(t.active(), 2, "right clamps at the end");
        t.step(-1);
        t.step(-1);
        assert_eq!(t.active(), 0);
        t.step(-1);
        assert_eq!(t.active(), 0, "left clamps at the start");
        t.step(1);
        assert_eq!(t.active(), 1);
    }

    #[test]
    fn closing_removes_the_showing_tab_not_the_first_one() {
        let mut t = Tabs::new();
        t.open();
        t.open();
        t.select(1);
        assert!(t.close_active());
        assert_eq!(t.titles(), vec!["Query 1", "Query 3"]);
    }

    #[test]
    fn closing_the_last_tab_is_refused() {
        let mut t = Tabs::new();
        assert!(!t.close_active());
        assert_eq!(t.len(), 1, "the console is never empty");
    }

    #[test]
    fn closing_the_final_tab_clamps_the_active_index() {
        let mut t = Tabs::new();
        t.open();
        assert_eq!(t.active(), 1);
        assert!(t.close_active());
        assert_eq!(t.active(), 0);
    }

    #[test]
    fn a_picked_statement_arrives_in_its_own_tab() {
        let mut t = Tabs::new();
        t.open_with("SELECT 1");
        assert_eq!(t.len(), 2);
        assert_eq!(t.active_tab().doc, "SELECT 1");
    }

    #[test]
    fn tab_ids_are_unique() {
        let mut t = Tabs::new();
        let a = t.active_tab().id.clone();
        let b = t.open();
        assert_ne!(a, b, "the id is the editor instance and the mount element");
    }

    #[test]
    fn adopting_an_empty_list_still_yields_a_console() {
        let t = Tabs::adopt(Vec::new(), 7);
        assert_eq!(t.len(), 1);
        assert_eq!(t.active(), 0);
    }
}
