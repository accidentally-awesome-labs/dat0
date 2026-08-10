//! Process identity for the cross-machine lock manifest + local pid liveness.
//! Unix-only (the app is already unix-only via its UDS singleton).

/// Whether a pid currently names a live process on THIS machine.
/// `kill(pid, 0)` sends no signal but performs error checking:
///   Ok / EPERM  -> process exists (alive)
///   ESRCH       -> no such process (dead)
pub fn pid_alive(pid: u32) -> bool {
    // SAFETY: kill with signal 0 is a pure existence check; no signal is sent.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    // errno == EPERM means it exists but we can't signal it -> alive.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// This machine's hostname (device name). Falls back to "unknown-host" if the
/// OS returns non-UTF8 (never observed on macOS).
pub fn hostname() -> String {
    gethostname::gethostname()
        .into_string()
        .unwrap_or_else(|_| "unknown-host".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn improbable_pid_is_dead() {
        // PID_MAX on macOS is 99998; 999_999 is guaranteed dead.
        assert!(!pid_alive(999_999));
    }

    #[test]
    fn hostname_is_nonempty() {
        assert!(!hostname().is_empty());
    }
}
