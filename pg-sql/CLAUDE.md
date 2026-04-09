# pg-sql Development Guidelines

## Principles

1. **Derive Parse/Scan/Visit wherever possible.** Manual impls only when the derive macro can't handle the case (e.g., Pratt postfix operators, non-standard parsing like psql directives). When a manual impl is needed, document why with a comment.

2. **Use method syntax, not UFCS.** Write `T::parse(input, rules)` not `<T as Parse>::parse(input, rules)`. Since `Scan` no longer has `peek`/`parse` methods, there is no ambiguity.

3. **Test against real Postgres.** Use testcontainers for regression tests. Each test gets a private Postgres 17 instance.

4. **Grow the grammar incrementally.** Each new test file drives new token/AST additions. Don't build grammar that isn't tested.
