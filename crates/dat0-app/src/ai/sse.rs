//! Pure SSE line framing. Accumulates raw bytes and yields the JSON payload of
//! each complete `data:` line, so the streaming transport (transport.rs) never
//! has to reason about chunk boundaries. Skips the `[DONE]` sentinel, blank
//! separators, `event:`/`id:` lines, and `:` comments.
//!
//! The buffer is kept as raw `Vec<u8>` so that multibyte UTF-8 codepoints split
//! across network chunk boundaries are reassembled before decoding. Only a
//! complete line (terminated by `\n`) is passed to `from_utf8_lossy`; since
//! line terminators are ASCII, a complete line never straddles a codepoint
//! boundary, making lossy decode harmless for well-formed server output.

/// Stateful line buffer for an SSE byte stream.
#[derive(Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `chunk` and return the payload of every complete `data:` line.
    /// A complete line is one terminated by `\n` (a trailing `\r` is trimmed).
    /// The final unterminated fragment is retained for the next call.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        // Process every complete `\n`-terminated line.
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            // Clone the line bytes before draining so the borrow ends before `drain`.
            let mut line_bytes: Vec<u8> = self.buf[..nl].to_vec();
            // Trim a trailing `\r` if present.
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }
            // Advance the buffer past the `\n`.
            self.buf.drain(..=nl);
            // Decode the complete line — safe: no split codepoints in a full line.
            let line = String::from_utf8_lossy(&line_bytes);

            let Some(rest) = line.strip_prefix("data:") else {
                continue; // event:/id:/comment/blank → ignore
            };
            let payload = rest.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            out.push(payload.to_string());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yields_complete_data_lines_skips_control() {
        let mut d = SseDecoder::new();
        let out = d.feed(b"event: content_block_delta\ndata: {\"a\":1}\n\n");
        assert_eq!(out, vec![r#"{"a":1}"#.to_string()]);
    }

    #[test]
    fn buffers_partial_line_across_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.feed(b"data: {\"x\":").is_empty()); // no newline yet → nothing
        let out = d.feed(b"42}\n");
        assert_eq!(out, vec![r#"{"x":42}"#.to_string()]);
    }

    #[test]
    fn skips_done_sentinel_and_blanks() {
        let mut d = SseDecoder::new();
        let out = d.feed(b"data: {\"y\":1}\n\ndata: [DONE]\n");
        assert_eq!(out, vec![r#"{"y":1}"#.to_string()]); // [DONE] dropped
    }

    #[test]
    fn handles_crlf_and_no_space_after_colon() {
        let mut d = SseDecoder::new();
        let out = d.feed(b"data:{\"z\":1}\r\n");
        assert_eq!(out, vec![r#"{"z":1}"#.to_string()]);
    }

    /// Regression: a multibyte UTF-8 codepoint split across a chunk boundary
    /// must NOT be corrupted into replacement characters (U+FFFD / `\xEF\xBF\xBD`).
    ///
    /// RED rationale: with the old `push_str(&String::from_utf8_lossy(chunk))`
    /// per-chunk approach, feeding only the first byte(s) of a multibyte sequence
    /// produced `\u{FFFD}` in the buffer. The second chunk's remaining bytes then
    /// decoded as another `\u{FFFD}` or garbage, so the emitted payload would
    /// contain `"→"` replaced by `"���"` instead of `"→"`.
    /// The new byte-buffer approach reassembles the raw bytes before any decoding,
    /// so `from_utf8_lossy` only ever sees a complete line — no split codepoints.
    #[test]
    fn reassembles_multibyte_utf8_split_across_chunks() {
        // "→" is U+2192, encoded as 3 bytes: 0xE2 0x86 0x92.
        // Build the full SSE line as bytes so the split index can be computed exactly.
        // Payload: {"arrow":"→"}  — the arrow's first UTF-8 byte is 0xE2.
        // Line layout (all ASCII except the 3-byte arrow):
        //   d a t a :   { " a  r  r  o  w  "  :  "  →(3b)  "  }  \n
        //   0 1 2 3 4 5 6 7 8  9  10 11 12 13 14 15 16 17 18 19 20 21
        let full_line: &[u8] = b"data: {\"arrow\":\"\xE2\x86\x92\"}\n";

        // Find the position of 0xE2 (first byte of →) to split there.
        let arrow_start = full_line
            .iter()
            .position(|&b| b == 0xE2u8)
            .expect("0xE2 must be present");
        // Split after the first byte of the 3-byte sequence, i.e. no \n in chunk1.
        let split = arrow_start + 1;
        let chunk1 = &full_line[..split];
        let chunk2 = &full_line[split..];

        // Sanity-check the split is genuinely inside the multibyte sequence.
        assert_eq!(
            chunk1.last(),
            Some(&0xE2u8),
            "chunk1 must end with first byte of →"
        );
        assert_eq!(chunk2[0], 0x86u8, "chunk2 must start with second byte of →");

        let mut d = SseDecoder::new();
        // First chunk has no \n → nothing emitted yet.
        assert!(
            d.feed(chunk1).is_empty(),
            "no newline in chunk1, nothing emitted"
        );
        // Second chunk completes the line.
        let out = d.feed(chunk2);
        assert_eq!(out.len(), 1);
        // The arrow must be intact, NOT replaced by U+FFFD.
        assert!(
            out[0].contains('→'),
            "expected intact → in payload, got: {:?}",
            out[0]
        );
        assert!(
            !out[0].contains('\u{FFFD}'),
            "payload must not contain replacement char U+FFFD, got: {:?}",
            out[0]
        );
    }
}
