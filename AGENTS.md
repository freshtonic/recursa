## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

The default five-label triage vocabulary is used. See `docs/agents/triage-labels.md`.

### Domain docs

This repository uses a single-context layout. See `docs/agents/domain.md`.

### Documentation gates

Authored library APIs, private implementation items, and documentation-hidden protocol items are
covered by the canonical documentation gate. See `docs/agents/documentation.md`.

### Test execution policy

Default local and CI suites exclude benchmarks, timing assertions, and aggregate evidence reruns.
See `docs/agents/testing.md`.

### Releasing

When the crate version changes, you must run `scripts/update-fixture-locks.sh`. A release PR from
release-plz does not run it. See `docs/agents/releasing.md`.
