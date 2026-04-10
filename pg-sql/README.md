# pg-psql

Postgres-flavoured SQL parser based on `recursa`.

## Roadmap

- [ ] 100% accuracy with Postgres SQL syntax (as of version 17)
- [ ] Passes Postgres's SQL parser regression tests
- [ ] Generate accurate railroad diagrams from the syntax tree
- [ ] Impossible to represent invalid SQL with the AST
- [ ] Support quickcheck-style testing: optional Arbitrary impl feature
- [ ] PGPLSQL support behind feature flag
- [ ] Benchmark (runnable in CI to catch regressions)
- [ ] Support "super flat" ASTs https://jhwlr.io/super-flat-ast/

