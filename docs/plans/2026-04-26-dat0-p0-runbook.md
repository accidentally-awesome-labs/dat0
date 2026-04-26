# dat0 P0 — Infrastructure Runbook

**Date:** 2026-04-26
**Phase:** P0 (Infrastructure setup, per [design spec §21.2](../specs/2026-04-26-dat0-design.md))
**Type:** Operational checklist (not a TDD-shaped implementation plan; P0 is mostly external/non-code work)

---

## Goal

Bring the operational substrate for dat0 online — repos, certificates, domains, error-reporting endpoint, community surfaces, signing keys, CI matrix — so that P1 (Foundation) can begin without external blockers.

## Status as of 2026-04-26

### Already complete

- [x] GitHub repo created: `accidentally-awesome-labs/dat0` (currently local only; push when ready)
- [x] `LICENSE` (Apache-2.0) committed
- [x] `NOTICE.md` (initial third-party attribution list) committed
- [x] `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1 by reference) committed
- [x] `CONTRIBUTING.md` (DCO sign-off requirements) committed
- [x] `SECURITY.md` (private disclosure to security@dat0.dev) committed
- [x] `README.md` (project pitch + status) committed
- [x] `docs/upstream-watch.md` (cadence + dep list + escalation) committed
- [x] `.gitignore` configured for Rust + macOS + Linux + signing artifacts
- [x] [Design specification](../specs/2026-04-26-dat0-design.md) written and committed

### Remaining items

The following must be completed before P1 begins, ordered by dependency.

---

## 1. Push repo to GitHub

- [ ] **1.1** Confirm `accidentally-awesome-labs` org access (you confirmed ownership 2026-04-26)
- [ ] **1.2** Create the GitHub repo: `gh repo create accidentally-awesome-labs/dat0 --private --description "Local-first data workbench for files from 2GB to TB"`
- [ ] **1.3** Add `origin` remote: `git -C /Users/salar/Projects/dat0 remote add origin git@github.com:accidentally-awesome-labs/dat0.git`
- [ ] **1.4** Push `main` with upstream tracking: `git -C /Users/salar/Projects/dat0 push -u origin main`
- [ ] **1.5** Verify branch protection on `main`: require PR reviews (minimum 1), require status checks (DCO, CI), block force-push, require linear history

**Success criterion:** `gh repo view accidentally-awesome-labs/dat0` shows the repo exists, `main` branch has both initial commits, branch protection rules visible in settings.

---

## 2. Companion site repo

- [ ] **2.1** Create the docs/marketing site repo: `gh repo create accidentally-awesome-labs/dat0-site --private --description "dat0.dev — landing page and documentation"`
- [ ] **2.2** Initial scaffolding from Astro+Starlight starter (deferred to P11 implementation; for now, just the empty repo with LICENSE + README placeholder)

**Success criterion:** Repo exists, accessible to maintainers, ready for P11.

---

## 3. DCO bot installation

- [ ] **3.1** Install [DCO GitHub App](https://github.com/apps/dco) on the `accidentally-awesome-labs` org, scoped to the `dat0` repo at minimum
- [ ] **3.2** Add a required-status-check entry on `main` branch protection: `DCO`
- [ ] **3.3** Verify by opening a test PR with a non-signed-off commit and confirming the bot blocks merge

**Success criterion:** DCO check runs on every PR; missing sign-offs fail the check.

---

## 4. Domain registration

- [ ] **4.1** Register `dat0.dev` (primary domain). Recommended registrar: Namecheap, Porkbun, or Cloudflare Registrar.
- [ ] **4.2** Register `dat0.app` (defensive — common typo destination)
- [ ] **4.3** Register `dato.dev` (defensive — typo against DatoCMS)
- [ ] **4.4** Configure DNS for `dat0.dev`:
  - A or CNAME pointing to a placeholder ("coming soon") page (any static-host service: GitHub Pages, Cloudflare Pages, Vercel, Netlify)
  - MX records for `dat0.dev` (for `security@`, `conduct@`, `noreply@`) — use a forwarding service (ImprovMX free tier, ForwardEmail, or paid email provider)
- [ ] **4.5** Configure DNS for `dat0.app` and `dato.dev`: 301 redirect to `dat0.dev`

**Success criterion:**
- `dig dat0.dev` returns a valid A record
- A placeholder page is reachable at `https://dat0.dev`
- Email aliases (`security@`, `conduct@`) deliver to a monitored inbox

---

## 5. Apple Developer Program

- [ ] **5.1** Enroll in Apple Developer Program ($99/yr USD) at <https://developer.apple.com/programs/enroll/>. Individual or LLC enrollment depending on `accidentally-awesome-labs` legal structure.
- [ ] **5.2** Once enrollment completes (1–7 days), generate a **Developer ID Application** certificate via Xcode → Settings → Accounts, or via Apple Developer portal manually
- [ ] **5.3** Generate a **notarization API key** (App Store Connect → Users and Access → Keys → App Store Connect API)
- [ ] **5.4** Export the Developer ID certificate + private key as a `.p12` (password-protected) for CI use
- [ ] **5.5** Store the `.p12`, the `.p12` password, the notarization API key, the API key ID, and the API key issuer ID as GitHub Actions secrets on the `dat0` repo:
  - `APPLE_DEVELOPER_CERT_P12` (base64-encoded `.p12`)
  - `APPLE_DEVELOPER_CERT_PASSWORD`
  - `APPLE_NOTARIZATION_API_KEY` (base64-encoded `.p8`)
  - `APPLE_NOTARIZATION_KEY_ID`
  - `APPLE_NOTARIZATION_ISSUER_ID`

**Success criterion:** A test "hello world" macOS binary built locally and signed with `codesign --sign "Developer ID Application: ..."` produces a signed binary; `spctl --assess --type exec hello` reports `accepted`.

---

## 6. EdDSA key pair for Sparkle update signing

- [ ] **6.1** Generate an EdDSA key pair using `generate_keys` (Sparkle's tool) or `openssl genpkey -algorithm Ed25519 -out ed25519_private.key`
- [ ] **6.2** Store the **private key** in the repo's GitHub Actions secrets as `SPARKLE_ED25519_PRIVATE_KEY` (base64-encoded)
- [ ] **6.3** Store an **encrypted offline backup** of the private key (passphrase-encrypted, on at least two physical media — e.g., one local encrypted USB + one in a password manager's secure-note feature)
- [ ] **6.4** Document the **public key** in the repo (it will be bundled into the app at build time during P1) — record it temporarily in `docs/security/sparkle-public-key.txt`
- [ ] **6.5** Document key rotation procedure in `docs/security-runbook.md` (file lands during P10, but a stub can land here)

**Success criterion:** Private key in GitHub secrets; encrypted backup verified retrievable; public key recorded in repo.

---

## 7. GlitchTip self-hosted instance

- [ ] **7.1** Provision a small VPS (Hetzner CX21 / Fly.io shared-cpu-2x / DigitalOcean $6 Basic Droplet — any will do). Single instance is fine for v1.
- [ ] **7.2** Set up a subdomain: `glitchtip.dat0.dev` (A record to the VPS)
- [ ] **7.3** Provision TLS via Let's Encrypt (Caddy or nginx + certbot)
- [ ] **7.4** Install GlitchTip via the official Docker Compose: <https://glitchtip.com/documentation/install>
- [ ] **7.5** Create the dat0 organization within GlitchTip; create the dat0 project; copy the DSN (Data Source Name)
- [ ] **7.6** Store the DSN as a GitHub Actions secret: `GLITCHTIP_DSN_PUBLIC` (the public key portion that's safe to embed in builds)
- [ ] **7.7** Configure Postgres backups: nightly `pg_dump` to a local cron + offsite copy (rclone to a cloud storage provider, or S3-compatible object storage)
- [ ] **7.8** Configure basic uptime monitoring (UptimeRobot free tier, or self-hosted Uptime Kuma) — alert on `glitchtip.dat0.dev/_health` 5xx or down > 5 min
- [ ] **7.9** Send a test event from a `curl` request to verify the endpoint accepts and displays it

**Success criterion:** GlitchTip UI accessible at `https://glitchtip.dat0.dev`; test event sent via curl appears in the UI; nightly Postgres backup runs and offsite copy verified.

---

## 8. Discord server

- [ ] **8.1** Create the Discord server: "dat0"
- [ ] **8.2** Configure channels:
  - `#announcements` (read-only for non-mods, mods post)
  - `#help` (everyone)
  - `#dev` (everyone)
  - `#showcase` (everyone, encourage `.dat0` package shares)
  - `#format-spec` (everyone, focused on `.dat0` format discussion)
  - `#releases` (read-only for non-mods, GitHub webhook posts releases)
- [ ] **8.3** Set roles: `@maintainer`, `@beta-tester`, `@everyone`
- [ ] **8.4** Install a basic anti-spam bot (e.g., Wick, Sapphire) with default settings
- [ ] **8.5** Configure a GitHub-to-Discord webhook on the `dat0` repo to post releases to `#releases` (Discord server settings → Integrations → Webhooks; on GitHub repo, Settings → Webhooks → add the Discord webhook URL with content type `application/json`)
- [ ] **8.6** Keep the server private (invite-only) until P11 (public launch)

**Success criterion:** All six channels exist; roles configured; GitHub webhook posts to `#releases` (test by tagging a throwaway test release).

---

## 9. CI scaffolding

- [ ] **9.1** Create `.github/workflows/ci.yml` with a matrix:
  - `runs-on: macos-latest` (arm64) — runs `cargo check --workspace`
  - `runs-on: macos-13` (x86_64 — last Intel Mac runner image) — runs `cargo check --workspace`
  - `runs-on: ubuntu-latest` (x86_64) — runs `cargo check --workspace`
  - `runs-on: ubuntu-latest-arm64` (aarch64) — runs `cargo check --workspace`
- [ ] **9.2** At this stage, `cargo check` runs against an effectively empty workspace (no Rust source yet). Goal is to validate the matrix shape.
- [ ] **9.3** Commit a stub `Cargo.toml` (workspace root) plus a stub `dat0-app` binary crate that prints "hello, dat0" so `cargo check` has something to validate
- [ ] **9.4** Run a test commit + PR against the matrix; all four targets must pass

**Success criterion:** A pull request triggers all four matrix jobs; all pass.

---

## 10. AppImageUpdate license-compatibility verification

This is research, not implementation.

- [ ] **10.1** Read the AppImageUpdate license: <https://github.com/AppImage/AppImageUpdate/blob/master/LICENSE> (GPL-2.0 or later)
- [ ] **10.2** Confirm the planned linkage model: dat0 invokes `appimageupdatetool` as a separate subprocess at runtime (not as a linked library)
- [ ] **10.3** Verify with the [GPL FAQ](https://www.gnu.org/licenses/gpl-faq.html#MereAggregation) that subprocess invocation does not impose copyleft on dat0
- [ ] **10.4** Document the conclusion in `NOTICE.md` (already done in initial NOTICE under "Mixed / project-specific" — verify wording is accurate after research confirms)

**Success criterion:** Decision documented in NOTICE; no compatibility concerns blocking the Linux distribution path.

---

## 11. `.dat0` MIME registration verification

- [ ] **11.1** Choose a MIME type: candidate `application/vnd.dat0+zip` (RFC 6838-compliant; signals "dat0-vendor zip-based")
- [ ] **11.2** Verify no existing collision: search the IANA Media Types registry (<https://www.iana.org/assignments/media-types/media-types.xhtml>) for `vnd.dat0` and the `application/x-dat0` shortform
- [ ] **11.3** Verify the `.dat0` extension does not already collide with established `.dat` MIME associations on Linux: check `/usr/share/mime/packages/` defaults and Freedesktop's shared-mime-info. `.dat` typically maps to `application/octet-stream` or `application/vnd.iccprofile` depending on context — neither matches `.dat0` (note the trailing `0`).
- [ ] **11.4** (Optional) File an IANA registration request for `application/vnd.dat0+zip` (free, takes 1-3 weeks). Not required for v1 launch but improves first-class recognition.
- [ ] **11.5** Document the chosen MIME in `docs/dat0-format-v1.md` (lands in P8a, but draft a stub now if convenient)

**Success criterion:** A MIME type chosen, no collision found, decision documented.

---

## 12. Trademark search (optional but recommended)

- [ ] **12.1** Search the USPTO database (<https://tmsearch.uspto.gov>) for active trademarks containing "dat0" or close phonetic equivalents (`data`, `dato`, `dat-zero`)
- [ ] **12.2** If clear, optionally file an Intent-to-Use application (~$300, takes 6-9 months) to lock the name. Not required for OSS launch.
- [ ] **12.3** Document outcome in a private maintainers' note (not committed)

**Success criterion:** No blocking trademark conflict identified; IP risk understood.

---

## P0 exit gate

P0 is complete when **all** of the following hold:

- [ ] Repo pushed to GitHub with branch protection
- [ ] Companion site repo exists
- [ ] DCO bot installed and verified
- [ ] All three domains registered with placeholder + redirects
- [ ] Apple Developer Program enrolled, cert + notarization key in CI secrets
- [ ] EdDSA Sparkle key pair generated, private in CI secrets, public stored, offline backup verified
- [ ] GlitchTip live with TLS, backups, monitoring; test event ingested
- [ ] Discord server live with six channels, GitHub release webhook posting
- [ ] CI matrix runs `cargo check` on all 4 targets; passes
- [ ] AppImageUpdate license decision documented
- [ ] `.dat0` MIME chosen and verified non-conflicting
- [ ] Trademark search completed (clear or risk-acknowledged)

**On exit, P1 (Foundation) implementation can begin.**

---

## Estimated time

- **External signups + waits** (Apple cert review, domain DNS propagation, AppleID checks): 1-7 days elapsed; ~30-90 minutes of active work
- **VPS + GlitchTip provisioning:** ~3-5 hours
- **Discord setup:** ~1 hour
- **Domain registration + DNS:** ~1-2 hours
- **CI matrix + smoke test:** ~2-4 hours
- **DCO bot + branch protection:** ~30 minutes
- **AppImageUpdate research + MIME research + trademark search:** ~2-4 hours

**Total active time: roughly 1-2 days, spread across 1-2 weeks of elapsed time** (waiting on Apple cert review, DNS propagation, etc.).

---

## On completion

After all checkboxes above are ticked, return to the [design spec](../specs/2026-04-26-dat0-design.md) and confirm P0 exit criteria from §21.2 line up. Then begin [P1 implementation plan](2026-04-26-dat0-p1-foundation-plan.md).
