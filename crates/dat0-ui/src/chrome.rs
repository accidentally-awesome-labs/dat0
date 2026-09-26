//! What the shell's chrome says about the process: the bytes dat0 has sent off
//! this machine, how many windows are open, and the chord that opens the
//! command palette (PD-023, step 5.8b).
//!
//! Each was a constant. The status bar's egress figure was a field nothing
//! wrote, so it read `egress 0 B` whatever dat0 had sent; the sidebar's footer
//! said `1 window` however many were open; and ⌘K, shown in the tab strip and
//! the status bar, was bound to nothing.

use dioxus::prelude::*;

use dat0_core::telemetry::egress;

use crate::state::{Status, Workspace};

/// Keep the window's egress figure the one `telemetry::egress` measures: what
/// dat0 has sent since it started, from any window, and whether a channel it
/// cannot meter is open. Redrawn when either changes, not polled.
pub fn use_egress(ws: Workspace) {
    use_future(move || async move {
        // Subscribed before the first read, so a send between the two wakes
        // the loop once more rather than slipping past both.
        let mut changed = egress::subscribe();
        loop {
            let (sent, floor) = (egress::total_sent(), egress::has_unmetered_channel());
            let mut status = ws.status;
            let stale = {
                let s = status.peek();
                s.egress != sent || s.egress_floor != floor
            };
            if stale {
                let mut s = status.write();
                s.egress = sent;
                s.egress_floor = floor;
            }
            if changed.changed().await.is_err() {
                return;
            }
        }
    });
}

/// How many workbench windows this process has open, kept current as they
/// open and close. One for a window mounted without a [`Boot`], as a test's
/// can be.
///
/// [`Boot`]: crate::launch::Boot
pub fn use_window_count() -> Signal<usize> {
    let mut count = use_signal(|| 1);
    use_future(move || {
        let windows = try_consume_context::<crate::launch::Boot>().map(|b| b.windows);
        async move {
            let Some(windows) = windows else {
                return;
            };
            let mut open = windows.count();
            loop {
                let n = (*open.borrow_and_update()).max(1);
                if *count.peek() != n {
                    count.set(n);
                }
                if open.changed().await.is_err() {
                    return;
                }
            }
        }
    });
    count
}

/// The chord that opens the palette on this platform, as the chrome shows it:
/// `⌘K` on macOS, `Ctrl+K` elsewhere. Read from the keymap's first palette
/// row, so the hint and the key cannot disagree.
pub fn palette_chord() -> String {
    dat0_core::keymap::chord_for_gpui_action(crate::keys::OPEN_PALETTE)
        .map(crate::components::command_palette::pretty_chord)
        .unwrap_or_default()
}

/// The sidebar footer's first line: `session · 2 windows · 1 tab`.
pub fn session_line(windows: usize, tabs: usize) -> String {
    format!(
        "session · {windows} {} · {tabs} {}",
        plural(windows, "sidebar.window", "sidebar.windows"),
        plural(tabs, "sidebar.tab", "sidebar.tabs"),
    )
}

/// The egress figure, `egress 1.2 KB`, with a `+` once a channel dat0 cannot
/// meter is open, since the figure is then a floor. Always shown, zero
/// included: "no bytes left this machine" is a claim, and a hidden counter
/// cannot make it.
pub fn egress_line(status: &Status) -> String {
    let floor = if status.egress_floor { "+" } else { "" };
    format!("egress {}{floor}", human_bytes(status.egress))
}

/// Bytes at one decimal place, binary units.
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

fn plural(n: usize, one: &str, many: &str) -> String {
    dat0_i18n::t(if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_reads_zero_rather_than_disappearing() {
        assert_eq!(egress_line(&Status::default()), "egress 0 B");
    }

    #[test]
    fn an_unmetered_channel_makes_the_figure_a_floor() {
        let status = Status {
            egress: 3 * 1024,
            egress_floor: true,
            ..Status::default()
        };
        assert_eq!(egress_line(&status), "egress 3.0 KB+");
    }

    #[test]
    fn the_footer_counts_one_and_many() {
        assert_eq!(session_line(1, 1), "session · 1 window · 1 tab");
        assert_eq!(session_line(2, 0), "session · 2 windows · 0 tabs");
    }
}
