//! Application-layer egress accounting (SH1).
//!
//! The marketing page promises `0 bytes left this machine`. Before this module
//! nothing measured that: the claim was a constant in a design doc. The status
//! bar renders [`total_sent`], so the number a user reads is a *measurement* of
//! what dat0 actually put on the wire, and a new network call that forgets to
//! record here is a privacy-claim regression — which is exactly what
//! `crates/dat0-app/tests/egress_seams.rs` gates.
//!
//! ## What is counted
//!
//! Application-layer request bytes that dat0 itself originates: the HTTP
//! request line, the header lines dat0 sets, and the request body. That is the
//! layer at which "we sent your data somewhere" is a meaningful statement.
//!
//! ## What is NOT counted, and why that is honest rather than convenient
//!
//! - **Transport framing.** TLS records, TCP/IP headers, HTTP/2 HPACK savings,
//!   `Content-Length`/`Host`/`Accept-Encoding` and the other headers the HTTP
//!   client adds on its own. None of it is observable from the call site and
//!   none of it carries user data.
//! - **Response bytes.** This counter is about egress. A download is ingress.
//! - **Channels dat0 does not own.** The DuckDB MotherDuck extension carries
//!   its own query traffic over its own connection; dat0 hands it a token and
//!   never sees a byte after that. Recording a small number there would be
//!   worse than recording nothing, so that seam calls
//!   [`note_unmetered_channel`] instead and the status bar renders a `+` on the
//!   total. A measured floor marked as a floor beats a precise-looking lie.
//!
//! ## Request-line arithmetic
//!
//! [`request_bytes`] counts `METHOD <url> HTTP/1.1\r\n` using the *absolute*
//! URL, while the wire carries origin-form (`/path`) plus a mandatory
//! `Host: <host>\r\n` header this module never sees. Those two deltas are
//! `scheme://host[:port]` versus `6 + host.len() + 2`, i.e. within a dozen
//! bytes of each other for every URL dat0 contacts. The approximation is
//! deliberate and documented rather than hidden behind a fake precision.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Monotonic total. `Relaxed` throughout: this is a display counter with no
/// happens-before relationship to anything, and a `SeqCst` fence on every
/// outbound request would buy nothing a user could observe.
static SENT: AtomicU64 = AtomicU64::new(0);

/// Set once an unmetered channel has been opened. Never cleared — the bytes
/// already left, and a disconnect does not un-send them.
static UNMETERED: AtomicBool = AtomicBool::new(false);

/// Add `bytes` to the process-wide egress total.
pub fn record_sent(bytes: u64) {
    SENT.fetch_add(bytes, Ordering::Relaxed);
}

/// Application-layer bytes dat0 has sent since process start.
pub fn total_sent() -> u64 {
    SENT.load(Ordering::Relaxed)
}

/// Record that dat0 has opened a network channel whose volume it cannot
/// observe (today: the DuckDB MotherDuck extension's own connection).
///
/// Once this is set, [`total_sent`] is a *floor*, and the status bar must say
/// so — see [`has_unmetered_channel`].
pub fn note_unmetered_channel() {
    UNMETERED.store(true, Ordering::Relaxed);
}

/// Whether [`total_sent`] is a floor rather than the whole story.
pub fn has_unmetered_channel() -> bool {
    UNMETERED.load(Ordering::Relaxed)
}

/// Wire size of one `name: value\r\n` header line.
pub fn header_line_bytes(name: &str, value: &str) -> u64 {
    // name + ": " + value + CRLF
    (name.len() + 2 + value.len() + 2) as u64
}

/// Application-layer size of one HTTP/1.1 request: request line, the header
/// lines the caller set (pre-summed via [`header_line_bytes`]), the blank line
/// that ends the header block, and the body.
///
/// Pure, so the arithmetic is unit-tested rather than asserted by inspection.
pub fn request_bytes(method: &str, url: &str, header_bytes: u64, body_len: u64) -> u64 {
    // "METHOD" SP "url" SP "HTTP/1.1" CRLF
    let request_line = (method.len() + 1 + url.len() + 1 + "HTTP/1.1".len() + 2) as u64;
    request_line + header_bytes + 2 + body_len
}

/// [`request_bytes`] + [`record_sent`], returning what it recorded so a caller
/// can log or assert it.
pub fn record_request(method: &str, url: &str, header_bytes: u64, body_len: u64) -> u64 {
    let n = request_bytes(method, url, header_bytes, body_len);
    record_sent(n);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_line_is_name_colon_space_value_crlf() {
        // "a: b\r\n" is 6 bytes.
        assert_eq!(header_line_bytes("a", "b"), 6);
        assert_eq!(
            header_line_bytes("content-type", "application/json"),
            (12 + 2 + 16 + 2) as u64
        );
    }

    #[test]
    fn request_bytes_sums_line_headers_terminator_and_body() {
        // "GET / HTTP/1.1\r\n" = 3+1+1+1+8+2 = 16, + 0 headers + 2 + 0 body.
        assert_eq!(request_bytes("GET", "/", 0, 0), 18);
        // A body is added verbatim; headers are passed through unchanged.
        assert_eq!(request_bytes("GET", "/", 10, 100), 128);
    }

    /// The two counter assertions live in ONE test on purpose: `SENT` is
    /// process-global and cargo runs unit tests on parallel threads, so two
    /// separate delta-measuring tests would interleave and flake.
    #[test]
    fn counter_accumulates_and_record_request_returns_what_it_added() {
        let before = total_sent();
        record_sent(7);
        record_sent(11);
        assert_eq!(total_sent(), before + 18);

        let n = record_request("POST", "https://x/y", 40, 200);
        assert_eq!(n, request_bytes("POST", "https://x/y", 40, 200));
        assert_eq!(total_sent(), before + 18 + n);
    }

    #[test]
    fn unmetered_flag_latches() {
        note_unmetered_channel();
        assert!(has_unmetered_channel(), "the flag never clears once set");
    }
}
