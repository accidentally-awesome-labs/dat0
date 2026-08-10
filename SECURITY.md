# Security Policy

## Reporting a vulnerability

Please report suspected security vulnerabilities **privately** to:

**security@dat0.dev**

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
1. Publish a release containing the fix
2. Publish a security advisory on the GitHub repo
3. Credit the reporter (unless anonymity is requested)

## Supported versions

Until the v1 release, no version is officially supported for security purposes — the repo currently contains design artifacts only. Once code ships, this section will list the versions that receive security updates.

## Scope

In scope:
- Vulnerabilities in the dat0 application itself
- Supply-chain or dependency issues affecting dat0
- `.dat0` format vulnerabilities (e.g., zip-bomb / path-traversal in package extraction)
- Issues in dat0's signing, update, or telemetry pipelines

Out of scope (please report to the upstream project instead):
- Vulnerabilities in `gpui`, `gpui-component`, `duckdb` / `duckdb-rs`, MotherDuck, or any other upstream dependency unless dat0's specific use of the dependency creates a unique vulnerability
- Issues in third-party LLM providers (OpenAI, Anthropic, OpenRouter)
- Issues in user-supplied themes from external sources
