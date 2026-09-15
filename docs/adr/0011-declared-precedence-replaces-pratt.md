# Declare precedence in bison's terms and remove Pratt

Status: accepted

Design: [`docs/declared-precedence.md`](../declared-precedence.md).

Recursa kept its Pratt authoring surface when it became an LALR parser generator, lowering
binding powers to LR precedence and deriving restricted expression clones from exclusion lists
(ADR 0004). That was a deliberate choice, not a leftover. It is reversed here because the
evidence changed: PostgreSQL 17.9 decides every expression shape with bison's `%left`, `%right`,
`%nonassoc` and `%prec` under `%expect 0`, with no precedence loop in C; pg-sql mirrors that
grammar and its measurements show the Pratt restriction clones are the largest structural cost
left in its automaton (2,301 rules against `gram.y`'s 104); and a prototype showed a plain
left-recursive enum with a `precedence` declaration parses the Pratt example's language
unchanged. Recursa's resolver also read token levels per nonterminal rather than per token,
which is not the model `gram.y` is written against.

Declared precedence becomes the sole precedence model, with bison's exact semantics: one level
per token, a rule takes its last declared terminal's level unless overridden, ties follow
associativity, and an unresolved cell is `RCA0400`. Restricted expressions such as `b_expr` are
written by hand as `gram.y` writes them, through `#[restricts(Base)]`, and are never derived from
an exclusion list. Cardinality-one attachment units inline into their owner's rule so that the
last-terminal rule and overrides apply literally. Pratt attributes, the binding-power scale, the
clone lattice, their diagnostics, fixtures and acceptance gates are removed; the codes are
retired and never reused; `examples/pratt-expressions` becomes `examples/expressions`.

pg-sql's expression grammar rewrite is inside this decision, because pg-sql is the only consumer
and the payoff is measured there. The work lands in three gated steps: Recursa gains the new
lowering while Pratt still works, pg-sql migrates under its zero-conflict 234/234 differential,
then Recursa removes Pratt with a migration diagnostic. This supersedes ADR 0004's "Pratt binding
powers lower to precedence" clause. ADR 0005 is unchanged.
