# Authored-hook migration ledger

This is the reviewed contraction ledger for
[issue #95](https://github.com/freshtonic/recursa/issues/95). The inventory is
pinned to `freshtonic/pg-sql` revision
`abb5152b60fdfe31e7cac8c361c71dcc02e3ea73`. Every production-authored lexer
callback, post-lex repair, and parser postcondition at that revision has a
declarative replacement; there are zero unmatched entries and no deferrals.

| Production behavior | Pinned source | Declarative replacement | Status |
| --- | --- | --- | --- |
| Dollar-quoted string scanning | `src/tokens.rs:888` (`DollarStringLit`) | `matchers { DollarStringLit => same_delimiter(...) }` | Replaced |
| Numeric trailing-word rejection | `src/tokens.rs:949` (`NumericLit`) | `next_exclusion(pattern = ..., excluded = ...)` | Replaced |
| Integer trailing-word rejection | `src/tokens.rs:957` (`IntegerLit`) | `next_exclusion(pattern = ..., excluded = ...)` | Replaced |
| Dollar-number trailing-word rejection | `src/tokens.rs:973` (`DollarNum`) | `next_exclusion(pattern = ..., excluded = ...)` | Replaced |
| Nested block-comment discard | `src/tokens.rs:996` (`BlockComment`) | `ignore { BlockComment => nested(...) }` | Replaced |
| Operator/comment-boundary repair | `src/tokens.rs:1101` (`split_bang_eq_minus_before_dash_comment`) | `operator_run(..., fences = ["/*", "--"], ...)` plus line-comment ignore | Replaced |
| psql-variable keyword repair | `src/tokens.rs:1166` (`split_psql_var_keyword_tokens`) | lexical punctuation/content decomposition plus the `PsqlVariableName` named admission | Replaced |
| Window reference-name postcondition | `src/tokens.rs:2059` (`WindowRefNameIdent`) | `WindowRefName = ColId - { ROWS, RANGE, GROUPS }` named admission | Replaced |

The generated lexing fixture is the executable replacement witness. It checks
exact public token kinds, text, UTF-8 spans, order, lexical diagnostics, named
admission parse behavior, ignored nested regions, operator fences, and
alternative-lexer validation without authored hooks:

```text
cargo test -p recursa-codegen --test generated_lexing --locked
```

Retaining classified block-comment trivia is intentionally outside this
ledger: issue #93 owns that later change. The replacement here preserves the
pinned production behavior, which discards block comments.
