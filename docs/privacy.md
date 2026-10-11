# dat0 Privacy Policy

> **Last updated:** 2026-10-11
>
> This policy separates local capture, crash submission, and other network
> channels. The Telemetry switch controls crash reports, not all networking.

[Documentation index](README.md)

---

## 1. Local capture (always on)

dat0 writes structured logs to `dat0.log` in its cache folder on your own machine
(`~/Library/Caches/dat0/logs` on macOS, `~/.cache/dat0/logs` on Linux;
**Settings → Advanced → Open logs folder** opens it). At launch, a log past 10 MB is
set aside as `dat0.log.1` and a new one begun. These logs are **purely local**:
nothing sends them anywhere, crash reports included (§2).

Crash reports redact absolute file-system paths before they leave the process:

- macOS paths (`/Users/<name>/…`)
- Linux paths (`/home/<name>/…`)
- Windows paths (`C:\<name>\…`)

…are all replaced with `<redacted>`, in the `before_send` hook in
`crates/dat0-core/src/telemetry/redaction.rs`. The local log is written by a
standard tracing subscriber and is **not** redacted — treat your local log files as
potentially containing absolute paths.

---

## 2. Crash submission (opt-in, default OFF)

dat0 can send crash reports to a **self-hosted** error tracker (GlitchTip) when
something goes wrong. This feature is:

- **Off by default.** The `crash_submission_enabled` field in `settings.toml`
  defaults to `false`. Crash reports are not submitted until you opt in.
- **Your choice, always.** You can toggle it at any time in
  **Settings → Telemetry → Enable crash report submission**. The setting is
  read when a report is sent, so turning it off stops the next one, and
  turning it on lets **Help → Report a Bug…** send one without a restart.
  With it off, Report a Bug says so and has nothing to send.
- **Counted.** A report sent is counted in the status bar's egress figure.

### 2.1 What is transmitted (only when opted in)

When `crash_submission_enabled = true` and a crash occurs, the following fields
are included in the crash report:

| Field | Description |
|---|---|
| Stack trace | Function call chain at the point of the crash, with file paths redacted (filenames only, no absolute paths — see §2.2) |
| OS name + version | e.g. `macOS 15.5` |
| dat0 version | e.g. `0.9.0` |
| Optional user note | Free-text note you may attach from the crash dialog |

A report you send from **Help → Report a Bug…** carries your note and the dat0
version, and no stack trace.

### 2.2 Redaction applied before transmission

Before any crash event leaves the process, the `before_send` hook in
`crates/dat0-core/src/telemetry/redaction.rs` performs the following:

- **Absolute paths** in stack frames (`filename`, `abs_path`) are reduced to
  `<redacted>/<basename>` — only the source filename is kept, never the full path.
- **Source context** (`vars`, `pre_context`, `post_context`, `context_line`)
  is cleared from every frame — no local variable values or surrounding code lines
  are transmitted.
- **User identity** (`event.user`) is cleared.
- **Server hostname** (`event.server_name`) is cleared.
- Exception message text is also scanned for path-like strings
  (`/Users/…`, `/home/…`, `C:\…`) and those spans are replaced with `<redacted>`.

Redaction is crash-specific and pattern-based, not a general sanitizer. Local
logs and free-text user notes can contain sensitive information; review notes
and any diagnostics before sharing them.

---

## 3. Local data and service credentials

Local file workflows do not automatically upload tables or package contents.
This does not prevent you from explicitly sharing a package, pasting data into
an AI prompt, or using a remote service.

AI keys and MotherDuck tokens normally use OS secret stores, with in-memory
fallback behavior when those stores are unavailable. They are not ordinary
settings or crash telemetry. They **are sent to the selected service for
authentication**. MotherDuck connections execute against a remote database;
that channel is separate from local file workflows.

## 4. AI and other network channels

AI requests go directly to the selected Anthropic, OpenAI, OpenRouter, or Custom
endpoint, not through dat0's servers. They include schema names/types and the
user prompt; **Explain SQL** includes the SQL being explained. Arbitrary user
text is not structurally sanitized, so anything pasted into it can be sent.

Current desktop stream builders in `crates/dat0-ui/src/components/ai.rs` pass
`sample_rows: None`. The stored **Include sample rows** toggle does not currently
attach preview rows. The schema builder itself maps names and types, not values.

Custom endpoints require HTTPS and reject local/private addresses by default.
The advanced override allows HTTP/private endpoints for local models. Resolved
addresses are checked before requests, but the HTTP client resolves again when
connecting: the check is not DNS connection pinning and leaves a re-resolution
race. See [AI providers](ai-providers.md) for configuration.

Other traffic has its own controls and triggers:

- **Update checks:** launch checks are default-on (`update_auto_check = true`);
  manual checks and selected update downloads also use the network.
- **Extensions:** DuckDB can acquire extensions when needed, including SQLite
  and MotherDuck extensions.
- **Remote samples:** selecting a remote sample downloads it.
- **Chosen services:** MotherDuck and AI use the selected remote service.

The egress display is application-layer accounting, **not packet capture**.
MotherDuck is unmetered; a `+` indicates the displayed total is a lower bound.

---

## 5. Self-hosted backend

dat0's crash reporting backend is **GlitchTip**, operated by Accidentally Awesome
Labs (the dat0 publisher). It is a self-hosted, open-source error tracker. Your
crash data is **not** sent to any third-party SaaS analytics or error-tracking
service (not Sentry.io, not Datadog, not Amplitude, etc.).

The GlitchTip DSN is compiled into the binary at build time; no per-invocation
network configuration is required.

---

## 6. Summary

| What | Transmitted? |
|---|---|
| Crash report | Only when opted in (default: off); crash-specific redaction applies |
| AI schema, prompt, Explain SQL | To the selected provider when requested; user text is not sanitized |
| Automatic AI preview rows | Not attached by current desktop streams |
| Keys / MotherDuck token | Authentication to the selected service; normally held in OS stores |
| Launch update check | Default-on, independent of crash consent |
| Extensions / selected remote samples | Downloaded when needed or selected |
| Local tables / packages | No automatic upload from local file workflows |

---

## 7. Questions

For non-sensitive questions, see [Support](../SUPPORT.md).
Report vulnerabilities or suspected data leaks privately via
[Security](../SECURITY.md), not a public issue.
