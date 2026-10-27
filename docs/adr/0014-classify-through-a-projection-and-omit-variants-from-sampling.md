# Classify through a declared projection, and omit a variant from sampling

Status: accepted

Design: [`docs/declared-rejections.md`](../declared-rejections.md), "Classification through a
projection" and "Sampling". Raised by pgtk-sql, which must read `U&"x" UESCAPE '!'` in every
identifier position.

`parser.c` (`base_yylex`) makes one `IDENT` of a `UIDENT` and its `UESCAPE` clause. pgtk-sql lexes
`U&"..."` as a token of its own, as scan.l lexes `UIDENT`, and it models the form as a struct of
the name and an optional clause, and adds a variant that holds that struct to each of its
identifier enums. Two things then fail. A
classification reads one token, and a token-wrapper enum with a two-token variant is no shape it
reads, so every declared rule that classifies an identifier (the `xmltable_column_el` options,
the role names, the partitioning strategy) can no longer be written. And `Arbitrary` samples the
new variant as often as the token variant, but few sampled escape strings name a correct escape
character, so a hook refuses most statements: the round-trip yield of pgtk-sql fell from 897 to
303 per mille.

We choose two declarations. A struct declares a projection, `#[classify(via = field)]` or
`#[classify(via(path, ...), spell = path)]`, and a classification that reaches the struct, as a
field type or as the payload of a token-wrapper variant, reads the text of the projected field,
or the text that the consumer's spelling function makes of several; the last field of a path may
be an `Option`, and an absent value gives the empty text. The spelling function gives a
spelling and not a folded text, so each classification keeps its own fold, and the struct states
its projection once for every enum that holds it. We rejected a projection on each variant,
because one struct stands in many enums; and a classification by a consumer function over the
whole Node, because the flat key would then need the lent stores of a validation hook in every
list step. Like a fold function, a spelling function is consumer code that runs in the parse
(ADR 0013).

An enum variant takes `#[omit(Arbitrary)]`, and the sampler never chooses it, as it never chooses
a refused variant. We rejected a weight, which only lowers the loss, and a sampler hook on the
struct, because the arena form samples a seed mirror whose types a consumer cannot name. The
omission reuses the filter of a refused variant, which the seed mirror shares by construction.
A grammar that uses neither declaration plans and generates what it did before: the plan of a
wrapper whose every variant holds a token is unchanged, and the plan identity takes the omissions
only when there are some. Only the generation identity changes, as it does for every change of the
generator, because it holds the generator's source fingerprint.
