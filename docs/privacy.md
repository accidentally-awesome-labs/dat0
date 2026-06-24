# dat0 Privacy Policy

> **Last updated:** 2026-06-23
>
> This document describes exactly what dat0 captures, what it sends, and what it
> never sends. It is the reference for the **Settings → Telemetry** section.

---

## 1. Local capture (always on)

dat0 writes structured logs to a rotating file on your own machine. These logs are
**purely local** — they are never transmitted anywhere unless you explicitly opt in
to crash submission (see §2).

The logging pipeline redacts absolute file-system paths before they are written:

- macOS paths (`/Users/<name>/…`)
- Linux paths (`/home/<name>/…`)
- Windows paths (`C:\<name>\…`)

…are all replaced with `<redacted>`. The redaction is applied in the `before_send`
hook in `crates/dat0-app/src/telemetry/redaction.rs` and to local log output.

---

## 2. Crash submission (opt-in, default OFF)

dat0 can send crash reports to a **self-hosted** error tracker (GlitchTip) when
something goes wrong. This feature is:

- **Off by default.** The `crash_submission_enabled` field in `Settings.toml`
  defaults to `false`. Nothing leaves your machine until you explicitly turn it on.
- **Your choice, always.** You can toggle it at any time in
  **Settings → Telemetry → Send crash reports**.

### 2.1 What is transmitted (only when opted in)

When `crash_submission_enabled = true` and a crash occurs, the following fields
are included in the crash report:

| Field | Description |
|---|---|
| Stack trace | Function call chain at the point of the crash, with file paths redacted (filenames only, no absolute paths — see §2.2) |
| OS name + version | e.g. `macOS 15.5` |
| dat0 version | e.g. `0.9.0` |
| Optional user note | Free-text note you may attach from the crash dialog |

### 2.2 Redaction applied before transmission

Before any crash event leaves the process, the `before_send` hook in
`crates/dat0-app/src/telemetry/redaction.rs` performs the following:

- **Absolute paths** in stack frames (`filename`, `abs_path`) are reduced to
  `<redacted>/<basename>` — only the source filename is kept, never the full path.
- **Source context** (`vars`, `pre_context`, `post_context`, `context_line`)
  is cleared from every frame — no local variable values or surrounding code lines
  are transmitted.
- **User identity** (`event.user`) is cleared.
- **Server hostname** (`event.server_name`) is cleared.
- Exception message text is also scanned for path-like strings
  (`/Users/…`, `/home/…`, `C:\…`) and those spans are replaced with `<redacted>`.

---

## 3. What is never transmitted

Regardless of your Telemetry setting, the following data **never leaves your
machine** via dat0's telemetry or AI features:

| Category | Detail |
|---|---|
| Schema and column names | Table names, column names, and data types stay local |
| Query text and results | SQL you write or results dat0 returns are never sent |
| File paths | Absolute file-system paths are redacted before any outbound network call |
| Source data | Row values from your tables or imported files are never transmitted |
| Derived data | Results of computed columns, transforms, or views are never transmitted |
| `.dat0` package contents | Contents of any `.dat0` workspace or package file are never transmitted |
| AI prompt content | Your natural-language queries to the AI feature stay on-device between your machine and the AI provider you configured |
| BYOK API keys | Your provider API keys are stored in the OS keychain only; they are never written to settings files, session files, logs, or telemetry |
| MotherDuck token | Your MotherDuck authentication token is stored in the OS keychain only; it is never written to settings files, session files, logs, or telemetry |

---

## 4. AI feature and outbound API calls

When you use the AI (NL→SQL) feature, dat0 sends a request to the AI provider
you configured (Anthropic, OpenAI, OpenRouter, or a Custom endpoint). The payload
is schema-only:

- **Sent:** table names, column names, and column types.
- **Never sent:** row values, query results, file paths, or `.dat0` package
  contents.

The schema-only filter is enforced structurally in
`crates/dat0-app/src/ai/schema_ctx.rs` and `crates/dat0-app/src/ai/request.rs`.
The `AiRequest` type has no field that can carry row data; the `SchemaContext`
built by `build_schema_context` maps names and types only.

dat0 does not proxy your AI requests through its own servers. Requests go directly
from your machine to the provider endpoint you configured. Custom provider URLs
are validated against an SSRF guard (`crates/dat0-app/src/ai/ssrf.rs`) that
blocks loopback, private, link-local, and unspecified IP ranges, and requires
HTTPS.

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
| Stack trace + OS + dat0 version (crash report) | Only if you opt in (default: off) |
| AI schema payload (table/column names + types) | Only when you use the AI feature, directly to your chosen provider |
| Query text, row data, results, file paths | Never |
| API keys, MotherDuck token | Never (OS keychain only) |
| `.dat0` package contents | Never |

---

## 7. Questions

Open an issue at
[github.com/accidentally-awesome-labs/dat0](https://github.com/accidentally-awesome-labs/dat0)
or email the maintainers listed in `NOTICE.md`.
