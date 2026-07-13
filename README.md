# recursa

An AST-first parser generator for Rust — the AST is the grammar.

Production port of the design validated in the recursa spike. See the
spike repo (`recursa-spike`) for `docs/api-design.md` (A1–A13) and
`docs/port-plan.md` (M0–M9, four mandatory validation gates). The
previous implementation lives at `freshtonic/recursa-old`.

## Workspace

| Crate | Role |
|---|---|
| `recursa` | Facade: runtime re-exports + the `#[recursa::grammar]` macro |
| `recursa-core` | Runtime: `Parse`, `Input`, tokens, spans, containers |
| `recursa-codegen` | Grammar analysis (FIRST/FOLLOW) and code emission |
| `recursa-test` | Corpus, differential-oracle, and round-trip harnesses |
| `recursa-explorer` | Interactive AST explorer |
