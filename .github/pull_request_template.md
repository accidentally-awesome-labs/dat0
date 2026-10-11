## Summary

<!-- What changes for users or contributors? -->

## Motivation / related issue

<!-- Explain the need and link any relevant issue or discussion. Substantial features and format, persistence, or privacy changes need discussion first; see CONTRIBUTING.md. Security reports belong in private reporting, not here. -->

## Validation

<!-- List the exact commands and behavior checks you ran, their results, and any checks not run with reasons. Do not mark unobserved results as passing. Use the change-specific checks in CONTRIBUTING.md and the advanced recipes in docs/ci.md. Live-service tests and release-host checks are not required for every PR. -->

- [ ] Every commit has a DCO sign-off (`git commit -s`); see [the contribution workflow](../CONTRIBUTING.md#pull-requests-review-and-dco).

## Conditional checks

<!-- Complete the relevant items and explain items that do not apply. -->

- [ ] **UI interaction / accessibility:** exercised user interaction through the existing harness and production keyboard-event routing, including Escape priority and release accessibility handles. See [accessibility guidance](../docs/a11y.md).
- [ ] **Appearance / geometry:** preserved token-only styling, checked theme contrast/switching, reviewed actual SSR snapshot changes, and exercised real-window geometry/focus where relevant. See [change-specific checks](../CONTRIBUTING.md#change-specific-checks).
- [ ] **Package / session compatibility:** documented any persistence or format impact, retained explicit versions/migrations/defaults, and ran relevant round-trip and compatibility checks. See [the format contract](../docs/dat0-format-v1.md).
- [ ] **Outbound data / security:** described any new network traffic, authentication, privacy, or trust-boundary changes and updated the [privacy guide](../docs/privacy.md) as needed. Undisclosed vulnerabilities use [private reporting](https://github.com/accidentally-awesome-labs/dat0/security/advisories/new).
- [ ] **Dependencies / licensing:** ran the relevant supply-chain checks and updated NOTICE through the canonical regeneration procedure when dependency attribution changed. See [contribution instructions](../CONTRIBUTING.md).
