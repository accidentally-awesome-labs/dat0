# dat0 Per-Release Privacy Review Process

> **Scope:** This checklist must be completed by a maintainer before signing and
> publishing any release. It verifies that the transmitted/never-transmitted
> contract in `docs/privacy.md` §2–§4 accurately reflects the current code.

---

## Overview

dat0 makes privacy guarantees in three code areas:

| Area | Key files |
|---|---|
| Crash-report redaction | `crates/dat0-app/src/telemetry/redaction.rs` |
| AI schema-only payload filter | `crates/dat0-app/src/ai/schema_ctx.rs`, `crates/dat0-app/src/ai/request.rs` |
| SSRF provider-URL guard | `crates/dat0-app/src/ai/ssrf.rs` |

Each area has a standing automated test suite. The checklist below verifies that
those tests still pass, that no new code bypasses the guards, and that
`docs/privacy.md` remains accurate.

---

## Pre-release checklist

### 1. Crash-report redaction (`telemetry/redaction.rs`)

**Goal:** Confirm no schema names, column names, query text, file paths, or key
material can leak through crash reports.

- [ ] Run `cargo test -p dat0-app telemetry` and confirm all tests pass.
- [ ] Open `crates/dat0-app/src/telemetry/redaction.rs` and verify:
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

### 2. AI schema-only payload filter (R17)

**Goal:** Confirm the AI feature sends only table names, column names, and column
types — never row values, query results, file paths, or `.dat0` contents.

- [ ] Run `cargo test -p dat0-app ai` and confirm all tests pass, including:
  - `maps_names_and_types_drops_surrogate` (schema_ctx.rs)
  - `r17_schema_context_carries_no_row_values` (schema_ctx.rs)
  - `schema_renders_names_and_types_only` (request.rs)
- [ ] Open `crates/dat0-app/src/ai/schema_ctx.rs` and verify:
  - `build_schema_context` maps only `.name` and `.data_type` from `ColumnInfo`
    — no `.nullable`, no row values, no origin metadata.
  - The `__dat0_rowid` surrogate column is still filtered out before the payload
    is built.
- [ ] Open `crates/dat0-app/src/ai/request.rs` and verify:
  - `AiRequest` has no field that can structurally carry row data other than
    `sample_rows: Option<SampleRows>`.
  - Confirm that `sample_rows` is only populated when the user has explicitly
    enabled the include-sample-rows toggle, and that it is `None` by default.
  - `SchemaContext` contains only `Vec<TableSchema>` (name + `Vec<ColumnSchema>`
    with name + type).
- [ ] Grep for any new call site that constructs `AiRequest` and confirm
  `sample_rows` is `None` or only set behind the user toggle:
  ```
  grep -rn "AiRequest {" crates/dat0-app/src/
  ```
- [ ] Cross-check against `docs/privacy.md` §3 and §4 — if the payload shape
  changed, update the doc to match.

---

### 3. SSRF provider-URL guard (`ai/ssrf.rs`)

**Goal:** Confirm that a user-supplied Custom provider URL cannot be used to
redirect AI requests to loopback, private, or link-local network addresses.

- [ ] Run `cargo test -p dat0-app ssrf` and confirm all tests pass, including:
  - `blocks_private_and_local_ranges`
  - `rejects_http_and_local_hosts_without_override`
  - `override_allows_http_and_private`
  - `blocks_ipv4_compatible_ipv6_loopback`
  - `rejects_trailing_dot_localhost`
- [ ] Open `crates/dat0-app/src/ai/ssrf.rs` and verify:
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
  grep -rn "validate_url\|ValidatedUrl" crates/dat0-app/src/
  ```
- [ ] Cross-check against `docs/privacy.md` §4 — if the guard behavior changed,
  update the doc.

---

### 4. §15.2/§15.3 transmitted/never-transmitted contract

**Goal:** Confirm that `docs/privacy.md` §2 (transmitted) and §3 (never
transmitted) are still accurate end-to-end.

- [ ] **Telemetry opt-in gate:** Open `crates/dat0-app/src/settings/schema.rs`
  and confirm `Telemetry.crash_submission_enabled` still defaults to `false`
  (via `#[serde(default)]` on the struct — `bool` defaults to `false`).
- [ ] **Keychain-only secrets:** Confirm no code path writes a BYOK API key or
  a MotherDuck token to `settings.toml`, a session file, a log line, or a
  telemetry event. Grep:
  ```
  grep -rn "motherduck_token\|api_key" crates/dat0-app/src/settings/
  grep -rn "api_key.*log\|log.*api_key" crates/dat0-app/src/
  ```
- [ ] **No new transmitted fields:** Review the diff since the last release for
  any new field added to `ClientOptions` in `telemetry/mod.rs`, any new tag or
  breadcrumb attached to Sentry events, or any new field added to `AiRequest`.
- [ ] Update `docs/privacy.md` with today's date in the "Last updated" line if
  any of the above verified items changed.

---

## Running all privacy-related tests at once

```bash
cargo test -p dat0-app -- telemetry ai ssrf 2>&1 | grep -E "^(test |FAILED|ok|error)"
```

Or the full workspace gate:

```bash
cargo test --workspace
```

All tests in the three areas above must be green before the release tag is
pushed.

---

## After the checklist

1. Update `docs/privacy.md` "Last updated" date if any behavior changed.
2. Sign the release tag — do not push it until this checklist is complete and
   committed to the branch.
3. Keep a dated record of who performed the review (a commit or PR comment is
   sufficient).
