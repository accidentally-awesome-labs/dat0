//! Pure SSE line framing. Accumulates raw bytes and yields the JSON payload of
//! each complete `data:` line, so the streaming transport (transport.rs) never
//! has to reason about chunk boundaries. Skips the `[DONE]` sentinel, blank
//! separators, `event:`/`id:` lines, and `:` comments.

/// Stateful line buffer for an SSE byte stream.
#[derive(Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `chunk` and return the payload of every complete `data:` line.
    /// A complete line is one terminated by `\n` (a trailing `\r` is trimmed).
    /// The final unterminated fragment is retained for the next call.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line = self.buf[..nl].trim_end_matches('\r').to_string();
            self.buf.drain(..=nl);
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
}
