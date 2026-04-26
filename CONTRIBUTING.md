# Contributing to dat0

Thanks for your interest in dat0. This document covers how to contribute.

## Status

dat0 is **pre-implementation**. The artifact in this repo today is the [design specification](docs/specs/2026-04-26-dat0-design.md). Code-level contributions will open as the implementation phases (P1 onward, per spec §21) begin.

In the meantime, helpful contributions include:
- Spec feedback (open an issue or discussion)
- Use-case scenarios that reveal gaps
- `.dat0` format spec review when published
- Discussion of cross-platform tradeoffs (especially Linux desktop polish)

## Developer Certificate of Origin (DCO)

dat0 uses the **Developer Certificate of Origin** (DCO) — see <https://developercertificate.org>. By signing off on a commit, you certify that you wrote the code or have the right to submit it under this project's license (Apache-2.0).

### How to sign off

Add a `Signed-off-by` line to every commit. The simplest way is the `-s` flag:

```
git commit -s -m "your message"
```

This appends:

```
Signed-off-by: Your Name <your.email@example.com>
```

Use the same name and email as your `git config user.name` and `git config user.email` settings.

### DCO check

A bot enforces DCO sign-off on every pull request. Commits without a sign-off are flagged and must be amended before merge.

## How to propose changes

1. **Open an issue first** for non-trivial changes, especially around the spec. This avoids wasted work if the direction differs.
2. **Fork the repo**, branch from `main`.
3. **Make your changes**, with DCO-signed commits.
4. **Open a pull request**. The CI will run available checks; the DCO bot validates sign-off.
5. **Engage with review**. Maintainers may ask for changes or rationale.

## Coding standards

To be filled in once the implementation begins (Phase P1). The spec calls for:
- Rust 2024 edition
- `rustfmt` enforced in CI
- `clippy` warnings as errors in CI
- Unit + integration + GPUI snapshot tests required for new code
- All UI strings must pass through the `t("…")` i18n helper

## Code of Conduct

By participating, you agree to abide by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Contributions are accepted under the terms of [Apache License 2.0](LICENSE).

## Where to ask questions

- GitHub Discussions (when the repo is public)
- Discord (link to be published at v1 launch)
- Spec questions: open an issue tagged `spec`
