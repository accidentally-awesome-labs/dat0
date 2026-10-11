# Security Policy

## Reporting a vulnerability

Please report suspected security vulnerabilities **privately** through
[GitHub private vulnerability reporting](https://github.com/accidentally-awesome-labs/dat0/security/advisories/new)
or by email to **security@dat0.app**.

Do **not** open public GitHub issues for security reports. Public disclosure before a fix is available puts users at risk.

When reporting, please include (as much as you can):
- A description of the issue and its potential impact
- Steps to reproduce
- The version or commit hash you tested against
- Any suggested mitigation

## Response timeline

- **Acknowledgment:** within 7 days of receipt
- **Initial assessment:** within 14 days
- **Fix or mitigation plan:** within 30 days for high-severity issues

These are best-effort targets for a small team; they are not a contractual SLA.

## Coordinated disclosure

Once a fix is available, we will:
1. Publish a fixed revision and, when releases are available, a release containing the fix
2. Publish a security advisory on the GitHub repo
3. Credit the reporter (unless anonymity is requested)

## Supported versions

Reports against current development code are accepted, and fixes land on `main`.
There is no released-version or backport support matrix yet. Before releases
exist, a fix may be published as a fixed revision and security advisory.

## Scope

In scope:
- Vulnerabilities in the dat0 application itself
- Supply-chain or dependency issues affecting dat0
- `.dat0` format vulnerabilities (e.g., zip-bomb / path-traversal in package extraction)
- Issues in dat0's signing, update, or telemetry pipelines

Report upstream-only vulnerabilities to the upstream project, but we accept
reports about their impact on dat0, including its use of `dioxus`, `wry` / `tao`,
`duckdb` / `duckdb-rs`, MotherDuck, or other dependencies.

Out of scope (please report to the relevant service or project instead):
- Issues in third-party LLM providers (OpenAI, Anthropic, OpenRouter)
- Issues in user-supplied themes from external sources
