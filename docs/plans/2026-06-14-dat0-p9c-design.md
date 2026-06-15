# dat0 P9c — AI BYOK cloud + onboarding design (design STUB)

**Status:** STUB — not yet brainstormed. Created alongside the P9a design so the P9
split is recorded. Do a full brainstorm before planning.
**Master-spec anchor:** §21.2 P9c (`docs/specs/2026-04-26-dat0-design.md:1147`);
risk R17 (`:805`); settings matrix (`:823`).
**Sibling docs:** P9a `2026-06-14-dat0-p9a-design.md` (charts, active); P9b
`2026-06-14-dat0-p9b-design.md` (MotherDuck, stub).

---

## 1. Framing — greenfield, largest P9 slice, has a security surface

**Zero AI/LLM code exists today** (grep `anthropic|openai|llm|api_key` over `crates/`
returns nothing). The only reusable primitive is the **keychain**:
`dat0_keychain::Keychain::{new(service), set, get, delete}` (`crates/dat0-keychain`) —
P9c stores the AI key under its own service string, exactly as P5c's
`KeychainTokenStore` does for the MotherDuck token (`connections/token_store.rs` is the
pattern to copy).

This slice carries the project's **only outbound-to-arbitrary-URL surface**, so the
security items are first-class, not polish.

---

## 2. Scope (from master-spec)

- **Settings → AI section**: provider picker (Anthropic / OpenAI / OpenRouter /
  Custom), API key (keychain), model name. Off by default; toggle **separate from
  telemetry**.
- **NL → SQL chip** in the SQL Console toolbar — emits SQL into the editor, **never
  auto-runs**.
- **"Explain" button** next to Run — explanation in a side panel.
- **Streaming side panel** — progressive render of AI responses.
- **Privacy banner** on first AI invocation; persists "don't show again".
- **SSRF mitigation (R17)** for Custom provider: **https only**; deny
  localhost/127.0.0.1, RFC 1918, link-local — unless an explicit "advanced override"
  toggle is on. Documented.
- **Outbound payload schema-only filter (R17)**: request carries only schema names +
  user prompt. **No row data, no query results, no source-file paths** unless an
  explicit "include sample rows" toggle is on. Verified by a **test fixture**.
- **Onboarding design work** (design-only here; implemented in P11): wireframes +
  content for first-run splash + sample workspace + skip-able tour →
  `docs/design/onboarding-v1.md`.

---

## 3. Key decisions to resolve at brainstorm

- **Provider abstraction**: a `LlmProvider` trait (chat + stream) with per-provider
  impls, vs a unified third-party crate. Anthropic / OpenAI / OpenRouter share the
  OpenAI-ish or Anthropic Messages shape; Custom is an OpenAI-compatible base-URL.
  Lean: thin hand-rolled trait + `reqwest` streaming (SSE) to avoid a heavy SDK and
  keep the SSRF/payload filter under our control.
- **Default Anthropic model**: a current Claude model — at writing, `claude-opus-4-8`
  (most capable) or `claude-sonnet-4-6` / `claude-haiku-4-5` for cost. **Resolve model
  IDs + streaming params via the `claude-api` skill at brainstorm time** (its TRIGGER
  fires on Anthropic API work — do not hand-type model IDs from memory).
- **NL→SQL schema context**: which catalog schema is sent (table+column names of the
  active workspace) and how it's bounded for the payload filter.
- **Streaming in GPUI**: how SSE chunks update the side-panel view (model + `cx.notify`
  on each delta; reuse the off-thread→notify pattern used across dat0).
- **Sub-slicing** (this is big — expect a split): candidate seams —
  - **P9c-1**: provider abstraction + Settings→AI section + keychain + SSRF/payload
    filter + privacy banner (the secure plumbing, no features yet).
  - **P9c-2**: NL→SQL chip + Explain button + streaming side panel (the user features).
  - **P9c-3 (design-only)**: onboarding wireframes/content doc.
- **HTTP client / runtime**: confirm `reqwest` (or existing transitive) + tokio (engine
  already runs on tokio — `runtime.enter()` lesson applies).

## 4. Security exit criteria (must not be deferred)

- Custom URL rejects `http://`, `localhost`/`127.0.0.1`, RFC 1918, link-local without
  the advanced-override toggle — **unit-tested**.
- Outbound request inspected in a **test fixture**: no row data / query results /
  source paths unless "include sample rows" is explicitly on.
- AI fully off without a key; **no spurious network calls** (testable: no key → no
  outbound).
- Key never in logs / `settings.toml` / telemetry (mirror the P9b token discipline).

## 5. Risks / unknowns

- **GPUI streaming UX** — progressive text render; spike a minimal SSE→panel loop (T0).
- **SSRF correctness** — IP-range parsing incl. IPv6 link-local + DNS-rebinding
  consideration; lean on a vetted approach, test exhaustively.
- **Provider drift** — API shapes change; the `claude-api` skill + context7 for
  OpenAI/OpenRouter docs at build time (do not trust memorized request schemas).
- Onboarding design depends on the app being feature-stable — fine, it's design-only
  and lands in P11.

## 6. Next step

Full brainstorm (`superpowers:brainstorming`) → invoke `claude-api` skill for
Anthropic model/streaming facts → decide the P9c sub-split → spec the first sub-slice
(likely P9c-1 secure plumbing) → plan.
