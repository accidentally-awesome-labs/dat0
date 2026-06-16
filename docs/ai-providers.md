# AI Providers

dat0 ships with a **bring-your-own-key (BYOK) AI layer**. AI is **off by default**
and does nothing until you supply an API key from a provider you already have an
account with. dat0 never proxies your key through its own servers.

## Opening the AI Providers panel

Open the panel from the **View** menu → **AI Providers…**, or search for it in the
Command Palette (`⇧⌘P`). It is a standalone dock, independent of the Connections
panel.

## Supported providers

dat0 supports four providers. You pick exactly one; switching providers keeps the
stored key for each slot.

| Provider | Wire format | Default endpoint |
|---|---|---|
| **Anthropic** | Anthropic Messages API | `https://api.anthropic.com` |
| **OpenAI** | OpenAI Chat Completions | `https://api.openai.com/v1` |
| **OpenRouter** | OpenAI-compatible | `https://openrouter.ai/api/v1` |
| **Custom** | OpenAI-compatible | user-supplied (see [Custom endpoints](#custom-endpoints)) |

Select a provider from the panel's picker, then fill in the fields below.

## Entering your API key

Type or paste your key into the **API key** field and press **Save key**.

- The key is written to the **OS keychain** (macOS Keychain / the platform
  equivalent). It is **never** written to `settings.toml`, session files, logs, or
  telemetry.
- The panel shows **"Key set"** or **"No key"** — the key itself is never echoed
  back to the UI.
- To remove a stored key, press **Forget key**.

## Model

Enter the model name in the **Model** field. Each provider shows a hint with
example model IDs (e.g. `claude-opus-4-8` for Anthropic, `gpt-4o` for OpenAI).
There is no hard default; you must enter one.

## Enabling AI

AI is **off** by default. Flip the **AI enabled** toggle to turn it on. This
requires a provider to be selected and a key to be stored; if either is missing,
the toggle has no effect.

## Testing the connection

Press **Test connection** to send a trivial fixed prompt through the full
provider + security + payload path and verify that your key is accepted.

- If no provider is selected or no key is stored, pressing it shows an inline
  notice (**"Select a provider first"** / **"Enter an API key first"**) instead
  of sending a request.
- A green **✓ Connected** result confirms your key works and the endpoint is
  reachable. A red **✗** result shows the error returned by the provider.

## What leaves your machine

The first time you invoke a Test-connection or enable AI, a one-time notice
appears:

> **AI requests leave your machine.**
> Schema names and your prompt are sent to the selected provider. No row data is
> sent unless 'Include sample rows' is on.

dat0 enforces this structurally: the outbound request carries only table and column
**names** (no values), plus your text prompt. Row data is **never** included by
default.

### Include sample rows (opt-in)

The **Include sample rows in requests** toggle adds a small number of representative
row values to the payload — useful if you want the AI to reason about actual content
rather than just schema shape. This toggle is off by default. When it is on, the
provider receives real row values from your data.

## Custom endpoints

The **Custom** provider lets you point dat0 at any OpenAI-compatible endpoint —
for example a local [Ollama](https://ollama.com) or [LM Studio](https://lmstudio.ai)
instance.

By default, dat0 applies **SSRF protection** to the Custom endpoint URL:

- Only `https://` URLs are accepted.
- Private/loopback addresses are blocked (`127.x`, `10.x`, `172.16–31.x`,
  `192.168.x`, `::1`).
- Link-local and cloud-metadata addresses are blocked
  (`169.254.x.x`, `fe80::/10`, including the AWS/GCP metadata IP `169.254.169.254`).
- IPv6 ULA ranges (`fc00::/7`) are blocked.

The protection validates the URL when you save it **and** re-checks all resolved
IP addresses at request time to defend against DNS-rebinding.

### Advanced override (local models)

If you are running a local model server (`http://localhost:11434` for Ollama,
`http://localhost:1234` for LM Studio), enable **Allow http / local endpoints
(advanced)** to lift the https-only and private-IP restrictions. Leave this off for
any internet-facing endpoint.

## Privacy summary

| What is sent | Always | Only with "Include sample rows" on |
|---|---|---|
| Your text prompt | yes | — |
| Table names | yes | — |
| Column names and types | yes | — |
| Row values | **never** | yes |
| Session files / file paths | **never** | **never** |
| API key | in auth header to your chosen provider only | — |
