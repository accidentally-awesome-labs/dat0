# dat0 P0 — Infrastructure Runbook

**Date:** 2026-04-26 (revised)
**Phase:** P0 — Infrastructure setup, per [design spec §21.2](../specs/2026-04-26-dat0-design.md)
**Type:** Operational checklist (not a TDD-shaped implementation plan; P0 is mostly external/non-code work)

---

## Goal

Bring the operational substrate for dat0 online across its v1 lifecycle. Items are categorized by **which phase they actually block** — most do *not* block P1 implementation.

## How to use this runbook

The checklist is split into four categories:

- **A. Blocks P1 (hard)** — items P1 cannot start without
- **B. Recommended early** — improves P1 experience but does not block coding
- **C. Blocks P10** — items required for hardening / signing / telemetry-live
- **D. Blocks P11** — items required for the public launch

P1 coding can begin with **zero items complete from any category**. Most items have long external lead times (Apple Developer review takes 1-7 days, DNS propagation, etc.) — start them early so they are ready when their respective phase arrives, not because P1 is gated on them.

---

## Status as of 2026-04-26

### Already complete (no further action)

- [x] GitHub repo created locally at `/Users/salar/Projects/dat0` (not yet pushed to remote)
- [x] `LICENSE` (Apache-2.0) committed
- [x] `NOTICE.md` (initial third-party attribution list) committed
- [x] `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1 by reference) committed
- [x] `CONTRIBUTING.md` (DCO sign-off requirements) committed
- [x] `SECURITY.md` (private disclosure to security@dat0.dev) committed
- [x] `README.md` (project pitch + status) committed
- [x] `docs/upstream-watch.md` (cadence + dep list + escalation) committed
- [x] `.gitignore` configured for Rust + macOS + Linux + signing artifacts
- [x] [Design specification](../specs/2026-04-26-dat0-design.md) written and committed
- [x] AppImageUpdate license-compatibility decision documented in `NOTICE.md` (subprocess invocation; Apache-2.0 compatible)

---

## Category A — Blocks P1 (hard)

**No items.** P1 implementation can begin immediately. All P1 deliverables can be developed and tested locally; nothing in P1's spec exit criteria depends on external services.

---

## Category B — Recommended early (improves P1 experience; does not block start)

These reduce friction during P1 work but the engineer can start P1 without them.

### B1. Push repo to GitHub

**Why:** P1 Task 20 builds a CI matrix workflow. Running CI remotely needs a remote.
**When:** Anytime before opening the first PR. Pushing earlier lets CI catch issues from the first commit.
**Effort:** ~30 minutes.

- [ ] **B1.1** Create the GitHub repo: `gh repo create accidentally-awesome-labs/dat0 --private --description "Local-first data workbench for files from 2GB to TB"`
- [ ] **B1.2** Add `origin` remote: `git -C /Users/salar/Projects/dat0 remote add origin git@github.com:accidentally-awesome-labs/dat0.git`
- [ ] **B1.3** Push `main` with upstream tracking: `git -C /Users/salar/Projects/dat0 push -u origin main`
- [ ] **B1.4** Configure branch protection on `main`: require PR reviews (minimum 1), require status checks (DCO once installed; CI once T20 lands), block force-push, require linear history

**Verification:** `gh repo view accidentally-awesome-labs/dat0` shows the repo with `main` containing all four planning commits.

### B2. DCO bot installation

**Why:** Enforces sign-off on every PR per `CONTRIBUTING.md`. Until installed, sign-off is honor-system.
**When:** Anytime before the first external PR. Solo dev on local commits doesn't need it on day 1.
**Effort:** ~5 minutes.

- [ ] **B2.1** Install [DCO GitHub App](https://github.com/apps/dco) on the `accidentally-awesome-labs` org, scoped to `dat0`
- [ ] **B2.2** Add `DCO` as a required status check on `main` branch protection
- [ ] **B2.3** Verify by opening a test PR with a non-signed-off commit; confirm the bot blocks

---

## Category C — Blocks P10 (hardening + signing + telemetry-live)

P10 enforces signing, notarization, and live crash submission. These items must be in place before P10 can complete its exit gate. **Apple Developer enrollment has the longest lead time (1-7 day review) — start it early in the project, ideally during P1 or P2.**

### C1. Apple Developer Program enrollment

**Why:** P10 requires Developer ID Application certificate + notarization API key for signed/notarized macOS DMGs.
**When:** Start during P1 or P2 to absorb the review window.
**Effort:** 1-7 day external wait + ~30 minutes active work.

- [ ] **C1.1** Enroll at <https://developer.apple.com/programs/enroll/> ($99/yr USD). Individual or LLC depending on `accidentally-awesome-labs` legal structure.
- [ ] **C1.2** After approval, generate a **Developer ID Application** certificate via Xcode → Settings → Accounts, or Apple Developer portal manually
- [ ] **C1.3** Generate a **notarization API key** in App Store Connect → Users and Access → Keys → App Store Connect API
- [ ] **C1.4** Export the Developer ID certificate + private key as a password-protected `.p12` for CI use
- [ ] **C1.5** Store as GitHub Actions secrets on the `dat0` repo:
  - `APPLE_DEVELOPER_CERT_P12` (base64-encoded `.p12`)
  - `APPLE_DEVELOPER_CERT_PASSWORD`
  - `APPLE_NOTARIZATION_API_KEY` (base64-encoded `.p8`)
  - `APPLE_NOTARIZATION_KEY_ID`
  - `APPLE_NOTARIZATION_ISSUER_ID`

**Verification:** A test "hello world" macOS binary signed with `codesign --sign "Developer ID Application: ..."` produces a binary that `spctl --assess --type exec hello` reports as `accepted`.

### C2. EdDSA key pair for Sparkle update signing

**Why:** P10 publishes a real signed Sparkle appcast. P1 only embeds a public key (placeholder accepted for development).
**When:** Generate before P10. Public key needed in repo earlier (P1 Task 15.2a accepts a placeholder).
**Effort:** ~30 minutes.

- [ ] **C2.1** Generate the key pair: `openssl genpkey -algorithm Ed25519 -out ed25519_private.key` (or use Sparkle's `generate_keys` tool)
- [ ] **C2.2** Store the **private key** as `SPARKLE_ED25519_PRIVATE_KEY` GitHub Actions secret (base64-encoded)
- [ ] **C2.3** **Encrypted offline backup** of the private key on at least two physical media (passphrase-encrypted USB + password-manager secure note)
- [ ] **C2.4** Place the **public key** at `crates/dat0-app/assets/sparkle-public-key.txt` (replacing the P1 placeholder file)
- [ ] **C2.5** Document key rotation procedure in `docs/security-runbook.md` (file lands during P10)

**Verification:** A test appcast signed with the private key validates against the public key bundled in the app.

### C3. GlitchTip self-hosted instance

**Why:** P10 flips opt-in crash submission on; the exit gate verifies a test crash from a release build appears in the GlitchTip UI.
**When:** Provision before P10. P1's `.cargo/config.toml` uses a stub DSN; that's fine for development.
**Effort:** ~3-5 hours.

- [ ] **C3.1** Provision a small VPS (Hetzner CX21 / Fly.io shared-cpu-2x / DigitalOcean $6 Basic Droplet)
- [ ] **C3.2** Set up subdomain `glitchtip.dat0.dev` (A record). Requires Domain D1 first.
- [ ] **C3.3** TLS via Let's Encrypt (Caddy or nginx + certbot)
- [ ] **C3.4** Install GlitchTip via Docker Compose: <https://glitchtip.com/documentation/install>
- [ ] **C3.5** Create the dat0 organization + dat0 project; copy the DSN
- [ ] **C3.6** Store DSN as `GLITCHTIP_DSN_PUBLIC` GitHub Actions secret
- [ ] **C3.7** Postgres backups: nightly `pg_dump` to local cron + offsite copy (rclone to cloud storage)
- [ ] **C3.8** Uptime monitoring (UptimeRobot free tier or self-hosted Uptime Kuma); alert on `glitchtip.dat0.dev/_health` 5xx or down > 5 min
- [ ] **C3.9** Send a test event via curl; verify it appears in the UI

**Verification:** Test event ingested and visible in GlitchTip UI; backup runs and offsite copy verified.

### C4. AppImageUpdate license-compatibility verification ✓ DONE

Already documented in `NOTICE.md` during P0 scaffolding. Subprocess invocation confirms compatibility under Apache-2.0 §4 and GPLv2 boundaries.

### C5. `.dat0` MIME registration verification

**Why:** P10 ships Linux `.desktop` integration that associates `.dat0` files with the app.
**When:** Before P10 Linux desktop integration step.
**Effort:** ~1 hour research.

- [ ] **C5.1** Confirm chosen MIME (default `application/vnd.dat0+zip`) does not collide with existing `.dat` MIME on Linux desktop environments. Check `/usr/share/mime/packages/` defaults and Freedesktop's shared-mime-info.
- [ ] **C5.2** (Optional, takes 1-3 weeks) File an IANA Media Types registration request for `application/vnd.dat0+zip`. Improves first-class recognition; not required for v1.
- [ ] **C5.3** Document chosen MIME in `docs/dat0-format-v1.md` (lands in P8a)

---

## Category D — Blocks P11 (docs site + public launch)

P11 is the public launch. These items must be live and configured by then. They have moderate lead times (DNS propagation, Apple cert is C1, etc.) but are mostly fast once kicked off.

### D1. Domain registration

**Why:** `dat0.dev` hosts the Astro+Starlight site (P11) and the GlitchTip subdomain (C3).
**When:** Anytime before D2 / C3 / P11.
**Effort:** ~1-2 hours.

- [ ] **D1.1** Register `dat0.dev` (primary). Recommended registrar: Namecheap, Porkbun, or Cloudflare Registrar.
- [ ] **D1.2** Register `dat0.app` (defensive — common typo destination)
- [ ] **D1.3** Register `dato.dev` (defensive — typo against DatoCMS)
- [ ] **D1.4** Configure DNS for `dat0.dev`:
  - A or CNAME → static-host placeholder ("coming soon")
  - MX records → forwarding service for `security@`, `conduct@`, `noreply@` (ImprovMX free tier, ForwardEmail, paid email provider)
- [ ] **D1.5** Configure DNS for `dat0.app` and `dato.dev`: 301 redirect to `dat0.dev`

**Verification:** `dig dat0.dev` returns valid A record; placeholder reachable; email aliases deliver.

### D2. Companion site repo

**Why:** Astro+Starlight site source lives separately. P11 implements the site against this repo.
**When:** Before P11. Empty repo is enough at P0.
**Effort:** ~10 minutes.

- [ ] **D2.1** Create: `gh repo create accidentally-awesome-labs/dat0-site --private --description "dat0.dev — landing page and documentation"`
- [ ] **D2.2** Initial scaffolding (Astro+Starlight starter) deferred to P11 implementation; P0 just creates the empty repo

### D3. Discord server

**Why:** Public community launch surface for P11.
**When:** Before P11. Server can stay invite-only / private until launch day.
**Effort:** ~1 hour.

- [ ] **D3.1** Create the Discord server "dat0"
- [ ] **D3.2** Configure channels: `#announcements` / `#help` / `#dev` / `#showcase` / `#format-spec` / `#releases`
- [ ] **D3.3** Set roles: `@maintainer`, `@beta-tester`, `@everyone`
- [ ] **D3.4** Install basic anti-spam bot (Wick / Sapphire) with default settings
- [ ] **D3.5** GitHub-to-Discord webhook on `dat0` repo for releases → `#releases`
- [ ] **D3.6** Keep server invite-only until P11 (public launch day)

**Verification:** All six channels exist; roles configured; GitHub webhook posts to `#releases` (test by tagging a throwaway release).

### D4. Trademark search (optional)

**Why:** Brand protection before public launch reduces collision risk.
**When:** Before P11. Optional but recommended.
**Effort:** ~1-2 hours search + optional $300 filing.

- [ ] **D4.1** Search the USPTO database (<https://tmsearch.uspto.gov>) for active trademarks containing "dat0" or close phonetic equivalents (`data`, `dato`, `dat-zero`)
- [ ] **D4.2** If clear, optionally file an Intent-to-Use application (~$300, 6-9 month review)
- [ ] **D4.3** Document outcome in a private maintainers' note

---

## Recommended timeline (parallel with P1+ work, not sequential)

| Window | Action |
|---|---|
| **Day 0** (now) | Start B1 (push repo) + B2 (DCO bot) — ~1 hour total. Then begin P1 Task 0. |
| **Week 1** | Kick off C1 (Apple Developer enrollment — review window starts). |
| **Week 1-2** | Apple cert approves; complete C1.4-C1.5 (CI secrets). |
| **Anywhere mid-P1** | C2 (EdDSA key pair) — needs the public key in repo for T15.2a. |
| **Anywhere before P10** | C3 (GlitchTip VPS) — depends on D1 (domain). C5 (MIME verification). |
| **~2-4 weeks before P11** | D1 (domains live) + D2 (companion site repo) + D3 (Discord ready) + D4 (trademark search). |
| **P11 launch day** | Discord opens to public; v1.0.0 release published. |

---

## Phase entry gates (revised)

| Phase | Required from P0 |
|---|---|
| **P1 entry** | Nothing required. Optionally B1 + B2 for cleaner workflow. |
| **P2-P9 entry** | Nothing required from P0. (P1 exit gate is the entry condition.) |
| **P10 entry** | Category C complete: C1, C2, C3, C5. C4 is already done. |
| **P11 entry** | Category D complete: D1, D2, D3. D4 optional. |

---

## On completion of each category

- **Category B done:** P1 work is unblocked at full velocity (CI runs remotely, PRs can land cleanly)
- **Category C done:** P10 hardening + signing + telemetry-live can complete its exit gate
- **Category D done:** P11 public launch can proceed
