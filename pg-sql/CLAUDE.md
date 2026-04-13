# pg-sql Development Guidelines

## Principles

1. **NEVER manually implement Parse/Scan/Visit/FormatTokens/Debug** ONLY derive those traits! When encountering a piece of SQL syntax that seems to make derivation with `recursa` impossible STOP what you are doing, explain the problem and ask for feedback.

2. **Use method syntax, not UFCS.** Write `T::parse(input, rules)` not `<T as Parse>::parse(input, rules)`. Since `Scan` no longer has `peek`/`parse` methods, there is no ambiguity.

3. **Test against real Postgres.** Use testcontainers for regression tests. Each test gets a private Postgres 17 instance.

4. **Grow the grammar incrementally.** Each new test file drives new token/AST additions. Don't build grammar that isn't tested.
