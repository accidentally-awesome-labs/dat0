# AI Providers

dat0 ships with a **bring-your-own-key (BYOK) AI layer**. AI is **off by default**
and does nothing until you supply an API key from a provider you already have an
account with. dat0 never proxies your key through its own servers.

[Documentation index](README.md)

## Opening the AI Providers panel

Open the panel from the **View** menu → **AI Providers…**, or search for it in the
Command Palette (`⇧⌘P`). It opens as a modal panel.

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

Press **Save key** to open an entry prompt, paste or type the key, and confirm.

- The key normally goes to the **OS secret store** (macOS Keychain or Linux
  Secret Service), not ordinary settings or telemetry. If the store cannot
  open, the app uses an in-memory fallback and warns that the key will not
  survive quitting.
- The panel shows **"Key set"** or **"No key"** — the key itself is never echoed
  back to the UI.
- To remove a stored key, press **Forget key**.

## Model

Press **Save model** to open the model-entry prompt and confirm a model ID.
Each provider shows example IDs. There is no hard default; enter a model.

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

AI requests authenticate with your key at the selected service and send schema
(table/column names and types) plus your prompt. **Explain SQL** sends the SQL
being explained. User text is not structurally sanitized: data, paths, or secrets
you paste into it can reach the provider.

The current desktop request builders send `sample_rows: None` for both streams.
The **Include sample rows in requests** preference is stored and defaults off,
but turning it on does **not** currently attach preview rows. This is not a
general guarantee about the contents of arbitrary prompt text.

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

The URL is validated and resolved IP addresses are rechecked at request time.
This is not connection pinning: the HTTP client independently resolves DNS
again when sending, leaving a DNS re-resolution race.

### Advanced override (local models)

If you are running a local model server (`http://localhost:11434` for Ollama,
`http://localhost:1234` for LM Studio), enable **Allow http / local endpoints
(advanced)** to lift the https-only and private-IP restrictions. Leave this off for
any internet-facing endpoint.

## Privacy summary

| Channel | What leaves the machine |
|---|---|
| AI prompt / Explain SQL | User text or SQL plus schema, to the chosen provider |
| Automatic sample rows | Not attached by current desktop streams, even with the stored toggle on |
| API key | Authentication to the chosen service |

See [Privacy](privacy.md) for separate crash, update, extension, and remote-data
channels and the limits of redaction.
