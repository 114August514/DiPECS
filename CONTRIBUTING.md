# Contributing to DiPECS

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

DiPECS is a fast-moving Android/Linux AIOS research prototype. This guide covers
the minimum rules for contributing code, evaluation artifacts, and documentation.
For background and architecture, read the [documentation site](https://114august514.github.io/DiPECS/).

## Before You Start

- Bug fixes: check whether an issue already exists. New issues should include
  reproduction steps, expected behavior, and actual behavior.
- New features or protocol changes: open or update an RFC first, especially
  when the change touches `aios-spec`, cross-crate data structures, action
  governance, or Android bridge contracts.
- Large changes: state the scope in an issue or RFC before implementation. Do
  not mix architecture changes, feature work, formatting churn, and unrelated
  documentation rewrites in one PR.

## Setup

The repository pins its Rust toolchain and Android assumptions. Start with:

```bash
source scripts/setup-env.sh
```

Basic verification:

```bash
cargo build --workspace
cargo test --workspace
```

Android work requires Android SDK Platform 35 and NDK r27d. See the development
docs for detailed environment notes.

## Local Checks

Before opening a PR, run at least:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

For the broader local CI-style check:

```bash
bash scripts/dev/check-all.sh
```

For documentation changes:

```bash
cd docs
uv run env PYTHONPATH=. mkdocs build
```

## Architecture Rules

Module boundaries are part of the design, not implementation detail:

- `aios-spec` is the protocol source of truth and must not depend on business modules.
- `apps/android-collector` collects Android public-API data and hosts the
  action bridge; it does not produce final `StructuredContext`.
- `aios-collector` ingests app/system sources and outputs `CollectorEnvelope` /
  `RawEvent`.
- `aios-core` owns privacy sanitization, window aggregation, policy review,
  replay validation, and `AuthorizedAction` lifecycle sealing.
- `aios-agent` receives only sanitized context and outputs `IntentBatch`.
- `aios-action` executes only policy-authorized actions and Android-safe bridge
  subsets.
- `aios-daemon` composes runtime components and manages lifecycle.

Avoid circular dependencies. Do not let action execution read collector or
inference internals directly.

## Pull Requests

Use feature branches:

```text
feat/<short-name>
fix/<short-name>
docs/<short-name>
```

PRs should include:

- Problem statement, implementation summary, and verification commands.
- Linked issue or RFC for protocol, architecture, or evidence-policy changes.
- Focused diffs. Put unrelated formatting and refactors in separate PRs.
- Review from a relevant module owner; cross-module contracts need stricter review.

Use Conventional Commits when possible:

```text
feat(action): add volatile cache release target
fix(android): reject stale execute envelopes
docs(readme): clarify v0.3 evidence boundary
```

## Testing Expectations

- New `RawEvent`: update `aios-spec`, add `PrivacyAirGap` tests, and cover window aggregation.
- New decision rule: add `aios-agent` backend tests and document capability limits.
- New action: add `PolicyEngine` review tests, `aios-action` result tests, and Android bridge coverage if dispatched to device.
- Android collection capability: first prove stable fields in `apps/android-collector`, then connect it to `aios-collector`.
- End-to-end route changes: update or add scripts under `tests/scenarios/`.
- Evidence claims: keep n, device, source artifact, baseline, and acceptance gate explicit.

## Safety and Privacy

- Avoid `unwrap()` / `expect()` in non-test code; use structured errors.
- Raw text, full paths, contacts, notification body, tokens, and device-unique
  identifiers must not cross `PrivacyAirGap` or enter committed artifacts.
- Automatic actions must pass `PolicyEngine`, `CapabilityLevel`, and
  `ActionLifecycle`.
- New dependencies require a reason, Android cross-compilation notes when
  applicable, and binary-size impact.
- Real-device scripts must fail closed when bridge responses, pressure evidence,
  or cache artifacts are missing.

## Documentation and i18n

- Keep root user-facing docs available in English and Simplified Chinese:
  `README.md` / `README.zh-CN.md`, `CONTRIBUTING.md` / `CONTRIBUTING.zh-CN.md`.
- When changing one language, update the sibling file in the same PR.
- Keep technical numbers identical across languages; do not translate away
  evidence boundaries.
- Prefer concise root docs and link to `docs/src` for details.

## Useful Links

- [README](README.md)
- [中文 README](README.zh-CN.md)
- [Changelog](CHANGELOG.md)
- [Architecture overview](docs/src/architecture/index.md)
- [Action benefit coverage](docs/src/evaluation/action-benefit-coverage.md)
- [Tests guide](tests/README.md)
- [Third-party sources](third_party/README.md)
