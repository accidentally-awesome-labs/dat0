# macOS-on-Mac CI Runner (tart VM)

> Operational runbook for running dat0's macOS CI job inside a tart-managed
> macOS VM on an Apple Silicon host. Sibling of `docs/ci.md`.
>
> Goal: cut the hosted `macos-14` **10× billing multiplier** to **0×**
> while keeping the dev environment uncontaminated by CI work.
>
> Status: **prototype-only**. Targets D-013 ("Self-hosted macOS CI runner
> — gated on dedicated Mac mini purchase"). Dev-machine hosting works for
> a solo-dev cadence; migrate to a dedicated Mac mini before opening the
> repo publicly (P11).

---

## When to use

| Scenario | Use this runbook? |
|---|---|
| Solo-dev, M-series laptop, daily PR cadence | Yes — prototype |
| Multiple concurrent PRs | No — Apple EULA caps macOS VMs to 2 per host; queue up fast |
| Public OSS, community PR storms | No — go to dedicated Mac mini (D-013 close) |
| Heavy nightly suites (`heavy.yml`) | Acceptable if scheduled overnight; otherwise dedicate |

---

## Why tart (and not Docker, multipass, anka)

- **Docker**: cannot run macOS — Apple licensing + XNU kernel design.
- **multipass**: Ubuntu/Linux only.
- **anka**: commercial; works well but licensed per-host.
- **tart** (cirruslabs): OSS (Fair Source), built on Apple's Virtualization.framework, runs macOS guests on Apple Silicon hosts. Designed for CI. Snapshot + clone primitives are first-class.

Apple's macOS EULA permits **up to 2 macOS VMs per physical Mac host** for general use. Dat0 needs 1; headroom is fine.

---

## Prerequisites

Host (your dev Mac):

- Apple Silicon (M1 or newer).
- macOS 14+ (host).
- ~80 GB free disk (base VM ~50 GB + work dir ~20 GB + snapshots).
- ~8 GB RAM headroom while VM is running (VM gets 8 GB; host needs the rest).
- Homebrew.

GitHub side:

- Repo admin access (to register a self-hosted runner).
- A runner registration token (`gh api -X POST repos/<owner>/<repo>/actions/runners/registration-token`).

---

## One-time host setup

```bash
brew install cirruslabs/cli/tart
tart --version  # confirm
```

Create a dedicated APFS volume for the VM image + runner work dir so a
`df=0` blowout (PR #5 incident, runnerkit) doesn't take down the dev Mac.

```bash
# 80 GB volume on the internal SSD, mounted at /Volumes/dat0-ci
sudo diskutil apfs addVolume disk3 APFS dat0-ci -reserve 60GB -quota 120GB
```

Adjust `disk3` to match your container disk (find with `diskutil list`).

---

## Provision the base VM

Pull a recent macOS image from tart's catalog:

```bash
tart pull ghcr.io/cirruslabs/macos-sequoia-xcode:latest
tart clone ghcr.io/cirruslabs/macos-sequoia-xcode:latest dat0-runner
```

The base image already includes Xcode + command-line tools + Homebrew + Rust.
The `clone` creates an editable local copy named `dat0-runner`.

Run it once to log in interactively + bake dat0-specific deps:

```bash
tart run dat0-runner
# Inside the VM:
#   - confirm `xcrun metal --version` works (Metal Toolchain is the P3a Lesson 1 gate)
#   - `rustup default stable && rustup update`
#   - `cargo install cargo-about` (NOTICE regen)
# Shut down cleanly: `sudo shutdown -h now` inside guest
```

Snapshot the clean state so re-creation is cheap:

```bash
tart clone dat0-runner dat0-runner-base
```

`dat0-runner-base` is the gold image. Re-clone from it whenever the
work VM accumulates cruft.

---

## Install the GitHub Actions runner inside the VM

Boot the VM:

```bash
tart run dat0-runner --dir=work:/Volumes/dat0-ci/work
```

The `--dir` mount lets the runner work dir live on the host's dedicated
APFS volume — bounded disk, easy to reset.

Inside the VM, install the runner the standard way:

```bash
mkdir -p ~/actions-runner && cd ~/actions-runner
curl -O -L https://github.com/actions/runner/releases/download/v2.319.1/actions-runner-osx-arm64-2.319.1.tar.gz
tar xzf actions-runner-osx-arm64-2.319.1.tar.gz

# Get a registration token (from host or browser):
#   gh api -X POST repos/accidentally-awesome-labs/dat0/actions/runners/registration-token

./config.sh \
  --url https://github.com/accidentally-awesome-labs/dat0 \
  --token <TOKEN> \
  --name macvm-dat0-local \
  --labels "self-hosted,macOS,ARM64,dat0-mac-vm" \
  --work /work \
  --replace

# Run as a service so it survives VM reboots:
./svc.sh install
./svc.sh start
```

Verify the runner shows up green in
`https://github.com/accidentally-awesome-labs/dat0/settings/actions/runners`.

---

## Wire into `ci.yml`

Edit the matrix in `.github/workflows/ci.yml`:

```yaml
target:
  # Hosted macOS (10× billing). Drop once mac-vm runner is stable.
  # - { labels: '["macos-14"]', triple: aarch64-apple-darwin, name: macos-arm64 }

  # Self-hosted macOS VM on dev Mac. 0× billing. See docs/ci-mac-vm-runner.md.
  - { labels: '["self-hosted", "macOS", "ARM64", "dat0-mac-vm"]', triple: aarch64-apple-darwin, name: macos-arm64 }

  - { labels: '["ubuntu-latest"]', triple: x86_64-unknown-linux-gnu, name: linux-x86_64 }
```

The existing `Metal Toolchain` step guards on `runner.os == 'macOS'` and
falls through cleanly if Metal is already present (which it is in the
cirruslabs base image).

---

## Operations

### Start / stop the VM

```bash
tart run dat0-runner --dir=work:/Volumes/dat0-ci/work        # foreground
tart run --no-graphics dat0-runner --dir=work:/Volumes/dat0-ci/work &   # background
tart stop dat0-runner                                         # graceful
```

For unattended operation, wrap in a `launchd` plist so the VM boots when
the dev Mac wakes. Sample plist in `docs/internal/mac-vm-launchd.plist`
(create as needed).

### Snapshot + reset

```bash
# After a successful CI run, snapshot the warm cargo cache:
tart clone dat0-runner dat0-runner-warm-$(date +%Y%m%d)

# When the work VM gets cruft, reset from the gold image:
tart delete dat0-runner
tart clone dat0-runner-base dat0-runner
# Re-register the runner inside (token expires every hour; fresh token each time).
```

### Refresh the base image

cirruslabs publishes new Xcode/macOS images monthly. Pull + re-clone
quarterly or when a Rust toolchain bump fights the existing base:

```bash
tart pull ghcr.io/cirruslabs/macos-sequoia-xcode:latest
tart delete dat0-runner-base dat0-runner
tart clone ghcr.io/cirruslabs/macos-sequoia-xcode:latest dat0-runner-base
tart clone dat0-runner-base dat0-runner
# Re-bake dat0-specific deps inside; re-register runner.
```

### Disk hygiene

The PR #5 runnerkit lesson applies — debug-profile link of the dat0-ui
test binary chain can balloon past 50 GB:

```bash
# Inside the VM, weekly:
cargo clean
rm -rf ~/.cargo/registry/cache
rm -rf /work/_temp
```

Or just snapshot a clean state and revert.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| `tart run` says "image not found" | Pull base image again; re-clone. |
| VM boots but runner doesn't appear in GH UI | Re-register; tokens expire after 60 minutes. |
| `xcrun metal --version` errors inside VM | `xcodebuild -downloadComponent MetalToolchain`. |
| Link step OOM (`lld` killed) | Increase VM RAM (`tart set dat0-runner --memory 16384`); cap concurrent `cargo build -j N` jobs via `.cargo/config.toml` `[build] jobs = 4`. |
| Disk fills mid-run (`df=0`) | Resize APFS quota; clean cargo target dir from snapshot. |
| GH says runner offline overnight | Host slept. Disable App Nap on `tart` + plug in power + set "Wake for network access". |
| Two concurrent CI jobs queue | Apple EULA cap = 2 VMs/host; either clone a second VM or accept serial execution. |

---

## Cost + maintenance

| Item | Cost |
|---|---|
| Hosted macos-14 (current) | ~$0.16/min × ~70 min × 10× multiplier ≈ $1.12 per CI run |
| dat0-mac-vm (this runbook) | $0 GitHub minutes + electricity + dev-Mac wear |
| Setup time | ~2h first time |
| Recurring maintenance | ~30 min/quarter (base image refresh) |

At the current PR cadence (~1-3 per week), the savings cover any dev-Mac
wear-and-tear well within a month. The migration to a dedicated Mac mini
(D-013 close) is a hardware decision, not a software one — this runbook
applies identically to a Mac mini host.

---

## Promotion path to D-013

When a dedicated Mac mini is provisioned:

1. Run this runbook on the Mac mini (identical steps).
2. Update `docs/deferrals.md` D-013 → `Status: closed` with the Mac mini
   model + label set.
3. Leave the dev-Mac runner as a fallback (or de-register).
4. Add `runs-on: ${{ matrix.labels }}` selection logic so heavy `heavy.yml`
   runs target the Mac mini while per-PR `ci.yml` can target either.

---

## See also

- `docs/ci.md` — overall CI setup, runnerkit Linux runner, NOTICE drift gate.
- `docs/deferrals.md` — D-006 (macOS Intel coverage), D-013 (self-hosted macOS).
- `https://tart.run/` — upstream tart docs.
- `https://github.com/cirruslabs/macos-image-templates` — base image sources.
