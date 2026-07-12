# recursa

An AST-first parser generator for Rust — the AST is the grammar.

Production port of the design validated in the recursa spike.

- [`docs/api-design.md`](docs/api-design.md) — the developer-facing API design (A1–A13). **Canonical: edited here.**
- [`docs/port-plan.md`](docs/port-plan.md) — the production plan (M0–M9, four mandatory validation gates). **Canonical: edited here.**

Reference material: the spike (`recursa-spike`, sibling checkout) holds the
validated FIRST/FOLLOW model and its conclusions; the previous
implementation lives at `freshtonic/recursa-old`.

## Workspace

| Crate | Role |
|---|---|
| `recursa` | Facade: runtime re-exports + the `#[recursa::grammar]` macro |
| `recursa-core` | Runtime: `Parse`, `Input`, tokens, spans, containers |
| `recursa-codegen` | Grammar analysis (FIRST/FOLLOW) and code emission |
| `recursa-test` | Corpus, differential-oracle, and round-trip harnesses |
| `recursa-explorer` | Interactive AST explorer |
