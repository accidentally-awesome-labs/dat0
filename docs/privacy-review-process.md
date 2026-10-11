# dat0 Per-Release Privacy Review Process

> **Scope:** This checklist must be completed by a maintainer before signing and
> publishing any release. It checks that the channel-specific contract in
> [Privacy](privacy.md) accurately reflects current request builders and transport.

[Documentation index](README.md)

---

## Overview

Privacy boundaries span request construction, transport, and capture:

| Area | Key files |
|---|---|
| Crash-report redaction | `crates/dat0-core/src/telemetry/redaction.rs` |
| AI schema and desktop request builders | `crates/dat0-core/src/ai/schema_ctx.rs`, `crates/dat0-core/src/ai/request.rs`, `crates/dat0-ui/src/components/ai.rs`, `crates/dat0-ui/src/ai_flow.rs` |
| SSRF and AI transport | `crates/dat0-core/src/ai/ssrf.rs`, `crates/dat0-core/src/ai/transport.rs` |
| Other network seams | Update checks/download/apply, remote sample downloads, DuckDB extension acquisition, MotherDuck connections, and egress accounting |

Each area has a standing automated test suite. The checklist below verifies that
those tests still pass, that no new code bypasses the guards, and that
`docs/privacy.md` remains accurate.

---

## Pre-release checklist

### 1. Crash-report redaction (`telemetry/redaction.rs`)

**Goal:** Preserve crash-specific redaction and consent, and review new event
fields for sensitive content; pattern redaction is not a universal sanitizer.

- [ ] Run `cargo test -p dat0-core telemetry` and confirm all tests pass.
- [ ] Open `crates/dat0-core/src/telemetry/redaction.rs` and verify:
  - `redact_event` is still registered as `before_send` in `telemetry/mod.rs`.
  - `frame.vars`, `frame.pre_context`, `frame.post_context`, and
    `frame.context_line` are all still cleared unconditionally.
  - `event.user` and `event.server_name` are still set to `None`.
  - `redact_path` still reduces stack-frame paths to `<redacted>/<basename>`.
  - `redact_text` still covers macOS (`/Users/…`), Linux (`/home/…`), and
    Windows (`C:\…`) path patterns in exception message text.
- [ ] If any new field was added to the Sentry `Event` struct (e.g., a breadcrumb,
  a tag, or a request body), confirm it does not carry PII and add a redaction
  step if needed.
- [ ] Cross-check against `docs/privacy.md` §2.2 — if the redaction behavior
  changed, update the doc to match.

---

### 2. AI schema and desktop request builders

**Goal:** Confirm schema contains names/types only, desktop streams attach no
automatic sample rows, and documentation discloses prompts and Explain SQL.

- [ ] Run `cargo test -p dat0-core ai` and confirm all tests pass, including:
  - `maps_names_and_types_drops_surrogate` (schema_ctx.rs)
  - `r17_schema_context_carries_no_row_values` (schema_ctx.rs)
  - `schema_renders_names_and_types_only` (request.rs)
- [ ] Open `crates/dat0-core/src/ai/schema_ctx.rs` and verify:
  - `build_schema_context` maps only `.name` and `.data_type` from `ColumnInfo`
    — no `.nullable`, no row values, no origin metadata.
  - The `__dat0_rowid` surrogate column is still filtered out before the payload
    is built.
- [ ] Open `crates/dat0-core/src/ai/request.rs` and verify:
  - `AiRequest` has no field that can structurally carry row data other than
    `sample_rows: Option<SampleRows>`.
  - Confirm default `sample_rows` is `None`; do not infer the desktop behavior
    from the core type's ability to carry optional samples.
  - `SchemaContext` contains only `Vec<TableSchema>` (name + `Vec<ColumnSchema>`
    with name + type).
- [ ] Inspect every `AiRequest` builder in both `crates/dat0-core/src/` and
  `crates/dat0-ui/src/`, especially `components/ai.rs`: current NL→SQL and Explain
  streams pass `sample_rows: None` even with the stored toggle on.
- [ ] Verify prompts and Explain SQL go to the chosen provider, authentication
  uses its key, and arbitrary user text is not represented as sanitized.
- [ ] Cross-check [Privacy](privacy.md) §3–§4 and [AI providers](ai-providers.md).

---

### 3. SSRF provider-URL guard (`ai/ssrf.rs`)

**Goal:** Preserve default endpoint restrictions and accurately disclose the
advanced HTTP/private-address override and DNS re-resolution caveat.

- [ ] Run `cargo test -p dat0-core ssrf` and confirm all tests pass, including:
  - `blocks_private_and_local_ranges`
  - `rejects_http_and_local_hosts_without_override`
  - `override_allows_http_and_private`
  - `blocks_ipv4_compatible_ipv6_loopback`
  - `rejects_trailing_dot_localhost`
- [ ] Open `crates/dat0-core/src/ai/ssrf.rs` and verify:
  - `validate_url` still rejects `http://` URLs without `advanced_override`.
  - `is_blocked_ip` still covers loopback, private (RFC 1918), link-local,
    unspecified, ULA (`fc00::/7`), and link-local IPv6 (`fe80::/10`).
  - IPv4-mapped (`::ffff:…`) and IPv4-compatible (`::…`) IPv6 addresses are
    still re-checked via `to_ipv4()` to catch embedded private v4 addresses.
  - Trailing-dot localhost (`localhost.`) is still normalized before the
    localhost check (`strip_suffix('.')` or equivalent).
- [ ] Confirm `validate_url` is called for every new code path that sends a
  request to a user-supplied URL. Grep:
  ```
  grep -rn "validate_url\|ValidatedUrl" crates/dat0-core/src/
  ```
- [ ] Inspect `ai/transport.rs`: every Custom send uses the guard and resolved-IP
  check. The HTTP client's independent DNS resolution leaves a race; do not
  describe the existing check as connection pinning.
- [ ] Cross-check against `docs/privacy.md` §4 — if the guard behavior changed,
  update the doc.

---

### 4. Channel-specific end-to-end contract

**Goal:** Confirm consent, secret storage, and all network triggers match
[Privacy](privacy.md), without treating crash opt-in as a global offline switch.

- [ ] **Telemetry opt-in gate:** Open `crates/dat0-core/src/settings/schema.rs`
  and confirm `Telemetry.crash_submission_enabled` still defaults to `false`
  (via `#[serde(default)]` on the struct — `bool` defaults to `false`).
- [ ] **Secret storage and authentication:** Review OS-store/in-memory fallback
  behavior and service authentication. Confirm no path writes a BYOK key or
  MotherDuck token to settings, sessions, log lines, or telemetry events. Grep:
  ```
  grep -rn "motherduck_token\|api_key" crates/dat0-core/src/settings/
  grep -rn "api_key.*log\|log.*api_key" crates/dat0-core/src/
  ```
- [ ] **No new transmitted fields:** Review the diff since the last release for
  any new field added to `ClientOptions` in `telemetry/mod.rs`, any new tag or
  breadcrumb attached to Sentry events, or any new field added to `AiRequest`.
- [ ] Review UI request builders, update-check/download seams, remote sample
  fetches, extension acquisition and MotherDuck connections. Check default-on
  update checks and default-off crash reports separately.
- [ ] Check egress accounting remains described as application-layer only;
  MotherDuck's unmetered channel must retain the `+` lower-bound indication.
- [ ] Review local logs and user notes as potentially sensitive; crash-pattern
  redaction does not sanitize all arbitrary text.
- [ ] Update `docs/privacy.md` with today's date in the "Last updated" line if
  any of the above verified items changed.

---

## Running privacy-related tests

Run each filter separately so Cargo applies it correctly:

```bash
cargo test -p dat0-core telemetry
cargo test -p dat0-core ai
cargo test -p dat0-core ssrf
```

Use [CONTRIBUTING](../CONTRIBUTING.md) and [CI](ci.md) for the workspace nextest
and doctest gates. No live credentials or sensitive submissions are needed for
this documentation review.

All tests in the three areas above must be green before the release tag is
pushed.

---

## After the checklist

1. Update `docs/privacy.md` "Last updated" date if any behavior changed.
2. Sign the release tag — do not push it until this checklist is complete and
   committed to the branch.
3. Keep a dated record of who performed the review (a commit or PR comment is
   sufficient).
