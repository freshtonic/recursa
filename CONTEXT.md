# Recursa

Recursa is an AST-first parser generator: grammar concepts are expressed in the source model, while generated parsing behavior preserves only semantically meaningful data in the AST.

## Language

**Fixed token**:
A grammar atom whose presence shapes syntax but carries no semantic data in the AST.
_Avoid_: Keyword field, punctuation field

**Word-shaped fixed token**:
A fixed token whose spelling is a valid Unicode identifier shape. Its recognition follows the grammar's keyword matching policy.
_Avoid_: Regex token, identifier token

**Keyword matching policy**:
The grammar-wide rule that makes word-shaped fixed tokens either exact or ASCII-case-insensitive throughout recognition and analysis.
_Avoid_: Pretty keyword case, regex flags

**Canonical token spelling**:
The stable semantic spelling of a word-shaped fixed token: its declared spelling under exact matching, or its ASCII-uppercase spelling under ASCII-insensitive matching. It does not replace matched source text.
_Avoid_: Display override, normalized input

**Keyword carrier**:
A regex-backed token definition proven able to recognize a word-shaped fixed token in place of its standalone lexical rule without changing the resulting token kind or span. Carrier status is derived, not declared.
_Avoid_: Identifier marker, keyword token type

**Closed lexical matcher**:
A centrally declared, generator-owned source scanner selected from the fixed vocabulary of same-delimiter regions, nested regions, next-scalar exclusion, operator runs, and physical-line regions. Its configuration is data in the token grammar; authored callbacks do not define its scanning algorithm.
_Avoid_: Custom lexer callback, inline scanner

**Lexical final extent**:
The complete byte range proposed by a lexical matcher after applying its closed scanning rules. Arbitration compares final extent before fixed-token status and explicit priority, independently of declaration order.
_Avoid_: Starter regex length, declaration precedence

**Operator-fence eligibility**:
An operator-shaped fixed candidate crossing a related operator run's interior configured fence is ineligible only when ordinary local arbitration selects diagnostic-free skip trivia covering that fence. A related operator run reaching the candidate's full extent keeps it eligible.
_Avoid_: Global fence veto, comment special case

**Reclaimable keyword**:
A word-shaped fixed token whose concrete token kind is admitted as content by a target at selected grammar positions.
_Avoid_: Soft keyword, context-sensitive keyword

**Keyword category**:
The single exclusive classification assigned to a metadata-bearing keyword and selectable as token admission membership.
_Avoid_: Keyword group, reservation enum

**Keyword flag**:
An orthogonal marker assigned to zero or more metadata-bearing keywords and selectable as token admission membership.
_Avoid_: Secondary category, keyword property callback

**Token admission set**:
A named canonical set of word-shaped fixed-token kinds compiled from category, flag, keyword, keyword-universe, and other admission membership through acyclic union and subtraction.
_Avoid_: Soft-keyword predicate, runtime token filter

**Text equivalence**:
The normalized semantic relation used when comparing exact-text predicates: exact scalar spelling or ASCII-case-insensitive spelling, as selected by the keyword matching policy.
_Avoid_: Regex matching mode, Unicode normalization

**Token type**:
A grammar type whose value retains exact matched input because that text carries semantic data. Parsed values borrow their match from the source; synthesized values supply owned text.
_Avoid_: Fixed token, token wrapper

**Presence node**:
A semantic unit node whose existence records that fixed syntax occurred, without retaining the token text itself.
_Avoid_: Optional keyword token, token flag

**Fixed-token presence**:
An authored boolean field whose value records whether one atomic optional fixed-token sequence occurred. It is a semantic field projection, not a public marker node.
_Avoid_: Presence node, optional keyword token

**Erased optional fixed-token group**:
A non-empty atomic fixed-token sequence accepted by positional syntax and retained only in provenance, not in the authored AST value.
_Avoid_: Optional semantic field, public marker node

**Token kind**:
A grammar-local named classification of a lexical match. Generated APIs expose typed token kinds; numeric runtime identifiers are private implementation details.
_Avoid_: Token ID, public discriminant

**Lexing result**:
The source-borrowing, grammar-bound output of generated lexing. It owns payload-free token records and creates parser input without requiring the caller to supply grammar metadata.
_Avoid_: Token vector, unbound lex result

**Lex builder**:
The typed construction path for an alternative lexer. It accepts grammar-local token kinds and validated source spans, then runs grammar-owned fixed-word classification and lexical-error normalization.
Its token and classified-trivia records must be source-bounded UTF-8, globally ordered, and non-overlapping; unrecorded source gaps are preserved without rescanning.
_Avoid_: Raw token-record constructor

**Classified trivia**:
An opt-in named whitespace or comment match retained with exact text and a checked span outside the significant-token stream. It is observable in source gaps and never participates in parsing, FIRST, or dispatch.
_Avoid_: Trivia token, ignored token

**Source gap**:
An immutable exact source range before, between, or after non-trivia lexical records, containing zero or more ordered classified-trivia records. Retained documents always expose beginning and end gaps, including empty and trivia-only inputs.
_Avoid_: Reconstructed whitespace, token attachment

**Document framing**:
Generated exact-source partitioning of a repeated island Node, its fixed boundary, atomic raw regions, and an optional committed-variant payload transition. It returns either a wholly strict typed partition or a rejection at the first failing island.
_Avoid_: Statement splitter, parser callback, partially typed document

**Framing island**:
One boundary-terminated occurrence of the Node selected by a document-framing declaration. A complete partition retains its `Parsed<T>`; the first ordinary failure rejects the whole document and no island value survives.
_Avoid_: Best-effort AST, recovered statement

**Framing rejection**:
The ordinary strict failure of one document: the extent of the first failing island candidate, that island's strict parse diagnostics, and the framing cause (missing boundary or missing payload terminator) when the island itself parsed. It is distinct from a fatal framing invariant error.
_Avoid_: Recovered frame, partial document, syntax error list

**Payload transition**:
A generated framing transition entered only after strict parsing commits the configured framing-island variant. Its data and standalone physical-line terminator have distinct exact spans; an incomplete island prefix cannot enter payload mode.
_Avoid_: Token trigger, lexer mode callback

**Lowered grammar**:
The immutable aggregate that keeps a normalized grammar together with its authoring-source context and their correlations.
_Avoid_: Emission model, enriched grammar description

**Source analysis**:
The interpretation of an authored Rust module tree into a source model and lowered grammar, including grammar-semantic resolution and source correlations.
_Avoid_: M3, source parsing

**Semantic analysis**:
The source-independent computation of nullable, FIRST, FOLLOW, dispatch, conflict, precedence, and provenance facts over a normalized grammar.
_Avoid_: Source analysis, Generation

**Source model**:
The grammar-relevant structure and locations of the selected Rust source tree retained alongside the normalized grammar.
_Avoid_: Semantic grammar, emission plan

**Lowering map**:
The explicit correlations between source-model identities and normalized semantic identities within one lowered grammar.
_Avoid_: Persistent ID registry, implicit positional coupling

**Generation**:
One complete, successfully published logical output produced from a lowered grammar under a matching emitter and protocol.
_Avoid_: Generated file, lowered grammar

**Build configuration**:
One name of the grammar's ordered `configurations(...)` list. A later name is a later, more demanding configuration, and it includes every earlier one. The first name is the baseline, which every build admits; each later name selects one Cargo feature.
_Avoid_: Version, feature flag, target release

**Stated configuration**:
The highest build configuration whose feature the build enables. It is what the build reproduces, and generation publishes it as `Configuration::BUILD`.
_Avoid_: Current version, active feature set

**Configuration gate**:
A `#[config(since = NAME)]` or `#[config(before = NAME)]` declaration that removes its element from a build the range excludes, exactly as the `cfg` it replaces, and, for a `since` bound, records that the element needs that configuration.
_Avoid_: Version gate, cfg predicate

**Rejection rule**:
A declaration on an enum that names forbidden pairs of its variants, with the code, message, hint and citation of the source that rejects them; any repetition of that enum rejects a parse whose items hold a named pair.
_Avoid_: Declared validation, conflict rule, validation rule

**Declared rejection**:
The parse failure a rejection rule produces, positioned at the first item that completes a forbidden pair in source order and carrying the rule's message data.
_Avoid_: Validation failure, semantic error

**Configuration requirement**:
The lowest build configuration whose grammar accepts one construct, together with the construct's site, its opaque code, message and hint, and the citation of the source that justifies the gate.
_Avoid_: Minimum version, error message

**Requirement ceiling**:
The highest configuration requirement reachable through one Node type, computed by generation as a fixpoint over the type graph. A scan skips a subtree whose ceiling cannot change its answer.
_Avoid_: Maximum version, subtree cache

**Reuse index**:
The per-`OUT_DIR` record, keyed by the discovered grammar source, configuration, and generator, that names the published Generation and its warnings so an unchanged grammar re-selects that Generation without analysis.
_Avoid_: Generation cache, incremental analysis

**Generation protocol**:
The private compatibility contract shared by one Recursa release's generator, structural loaders, generated output, and facade runtime.
_Avoid_: Public ABI, generated-file format

**Grammar element**:
A source-ordered component of a Node or variant after positional token syntax is lowered: one fixed-token use, one erased optional fixed-token group, or one semantic field. Fixed-token presence is a semantic field form.
_Avoid_: Runtime repetition occurrence, separator occurrence

**Token definition**:
The unified grammar declaration of one lexical or fixed token, together with every source site that defines it.
_Avoid_: Token use, token kind

**Token use**:
One located grammar position that refers to a token definition, including a fixed-token element or repetition separator.
_Avoid_: Token definition, runtime token record

**Fixed-token namespace**:
The single grammar-wide symbolic namespace in which bare identifiers in token-bearing attributes resolve to token definitions, independently of Rust modules and `use` items.
_Avoid_: Rust type namespace, token import scope

**Grammar-semantic reference**:
A source name or path whose target changes the normalized grammar and therefore must resolve to a typed source identity before lowering. These references include Node and generated-token field targets, supported container identities, and fixed-token references.
_Avoid_: Every Rust path, opaque emitted path

**Semantic type catalog**:
The closed set of supported container and Recursa type identities outside the selected tree that source analysis may recognize and expose through exact imports, re-exports, and aliases.
_Avoid_: External crate namespace, external glob contents

**Facade mapping**:
The grammar-root configuration that names the normal Recursa dependency path used for supported semantic identities and generated protocol calls.
_Avoid_: Dependency-name inference, hard-coded crate name

**Non-empty sequence**:
A one-or-more semantic repetition whose value cannot be empty. Its concrete storage is a property of the AST representation, not part of the grammar meaning.
_Avoid_: Validated `Vec<T>`, non-empty parser metadata

**Grammar type application**:
A resolved use of a Node declaration with concrete semantic type arguments. Its Node key remains the declaration's logical identity; type arguments distinguish materialized grammar applications, while lifetime arguments are retained for emission but do not distinguish grammar identity.
_Avoid_: Node key, Rust monomorphization artifact

**Container shape**:
The arrangement of cardinality and explicit indirection around one semantic field target. Cardinality determines optionality and repetition; boxing changes indirection without changing the grammar meaning.
_Avoid_: Flat container flag, arbitrary Rust generic type

**Token type binding**:
A synthesized Rust type-namespace binding introduced at a named content-token definition's module and linked to its single grammar-wide semantic token-type identity. Multiple bindings may alias the same generated type.
_Avoid_: Grammar-global implicit import, fixed-token symbol

**Resolution trace**:
The retained path from one grammar-semantic use site through the chosen imports, re-exports, synthesized bindings, and type aliases to its terminal typed identity.
_Avoid_: Diagnostic search log, terminal name string

**Opaque Rust fragment**:
Parsed Rust syntax retained for faithful emission but not target-resolved by source analysis because its target does not change the normalized grammar, including derive and configured trait paths.
_Avoid_: Grammar-semantic reference, unparsed text

**Module inclusion**:
One logical placement of a source file or inline module in the selected grammar tree. The same source file may have multiple module inclusions with distinct logical paths.
_Avoid_: Source file

**Selected-root anchor**:
The logical `crate::...` module path declared as `module = ...` in one generation's root `grammar!` configuration. It anchors root-relative resolution and Node keys, and its structural loader verifies the declared placement during compilation.
_Avoid_: Root filename inference, physical source path

**Build inputs**:
The reproducible selected-root, source, module-candidate, configuration, and environment facts that determine a lowered grammar and its Cargo invalidation.
_Avoid_: Grammar description, generated output

**Semantic origin**:
The explicit source identities or typed derivation from which a normalized grammar entity arose.
_Avoid_: Optional source link, implicit positional match

**Generation identity**:
The private identity of canonical lowered content under a particular emitter and generated-output protocol.
_Avoid_: Persistent entity ID, output filename

**Analyzed grammar**:
A successfully lowered grammar paired with source-independent semantic analysis output.
_Avoid_: Emission plan, analysis report

**Emission plan**:
The source-aware generated-behavior plan formed by joining an analyzed grammar's authoring structure, correlations, and semantic facts.
_Avoid_: Grammar description, source model

**Configured trait plan**:
The effective set of supported optional trait implementations selected for a semantic grammar type. Nodes combine grammar defaults with local additions or omissions; generated content-token types use the grammar defaults for optional capabilities in addition to their intrinsic structural value traits.
_Avoid_: Ordinary Rust derives, intrinsic generated behavior

**Configured trait recipe**:
One generator-owned implementation shape selected from the closed catalog by a canonical configured-trait spelling.
_Avoid_: Arbitrary trait path, proc-macro derive

**Intrinsic content-token value traits**:
The fixed `Debug`, `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, and `Hash` implementations emitted for every generated content-token type so ordinary Rust derives on authored AST Nodes compose through token fields. They participate in generated trait-behavior identity and debug metadata but are not optional configured trait recipes.
_Avoid_: Grammar defaults, inferred configured traits

**Generated export plan**:
The source-aware closed set of documented bindings a generation introduces with the effective visibility specified by their declarations.
_Avoid_: Generated prelude, hidden loader surface

**Parse plan**:
The immutable executable plan for parsing one materialized grammar type, formed
by joining LR analysis, source construction, and capture facts. Generation
renders it without rediscovering grammar semantics.
_Avoid_: Generated parse body, analysis facts

**Recursa parser**:
The sole generated parser for one grammar. It recognizes the grammar through
one LALR(1) automaton with a start state per grammar type, so every grammar
type retains its Parse capability as an entry point.
_Avoid_: Parser style, named parser, recursive-descent parser, parser mode

**Reduce action**:
The generated construction step for one LR rule: it takes the rule's semantic
values from the value stack and yields the constructed grammar value. Fixed
tokens carry no value and never reach the value stack.
_Avoid_: Parse body, semantic callback, bison action

**Value stack**:
The Recursa parser's runtime stack of semantic values only, kept apart from the
automaton's state stack and from occurrence provenance.
_Avoid_: Symbol stack, union stack

**Lookahead filter**:
A token-grammar declaration (`lookahead { MERGED = TOKEN before { TRIGGER, ...
} }`) that makes the parser's token feed present one lexed token as a distinct
merged kind whenever the next token is a trigger, so that a grammar needing two
tokens of lookahead at that point stays LALR(1). The substitution is absolute:
where the merged kind applies it is the only terminal looked up.
_Avoid_: Token rewrite, scanner hack, two-token lookahead

**Merged kind**:
The token kind a lookahead filter presents in place of its source token. It is
never lexed and never a trigger, it spells like the token it merges, and a
grammar position that admits the source token where a trigger may follow it
must admit the merged kind as well. A state with no action for a merged kind
reports a syntax error on that token; the source terminal is never retried.
_Avoid_: Lookahead token, _LA alias, filtered token

**Prefix acceptance**:
A Recursa-parser root accepts once the root type is reduced and the lookahead
has no shift action; a shiftable lookahead always shifts.
_Avoid_: End-of-input accept, longest-match search

**Occurrence stack**:
The Recursa parser's runtime stack of occurrence provenance, parallel to the
state stack: it is pushed on every shift with the token's span and folded on
every reduce by the rule's fold. It exists only when the parse captures
provenance or the grammar has a mandatory site, and only when the `spans`
feature permits provenance. Every generated parser exposes
`parse_without_spans` to suppress the stack even for mandatory sites.
_Avoid_: Span stack, location stack

**UI occurrence**:
The Explorer UI's render-layer unit: one Explorer node or Explorer element flattened into a row for display (`occurrenceEntries()` in `recursa-explorer/assets/model.js`). Distinct from the parser-internal Occurrence stack and from OccurrenceSchema; the JS layer keeps the shorter name "occurrence" by established precedent, so context (Rust vs. Explorer UI) disambiguates rather than the name itself.
_Avoid_: Occurrence stack (parser provenance, not this), Explorer node/Explorer element used alone when the row spans both

**Rule fold**:
The static description of what a rule's reduce builds on the occurrence stack,
rendered from the same site forms as its reduce action: a node with one child
per site, a pass-through, a group, a repetition group, or a list step.
_Avoid_: Provenance action, span callback

**Inlined choice**:
An optional grammar element lowered into its parent rule as one alternative with the content and one without, the way gram.y writes optional syntax by hand, so that no epsilon reduce precedes the token that decides it. Fixed-only choices are always inlined; valued choices are inlined when their content can begin with an admitted keyword.
_Avoid_: Expanded optional, rule duplication

**Helper nonterminal**:
A nonterminal that LR lowering invents and shares between use sites: a nullable
optional or presence helper, or a list core and its zero-or-more and
trailing-separator forms. Its rules carry the element type and `Box` layers
their reduce actions need.
_Avoid_: Synthetic node, bison `opt_` rule

**Helper copy**:
A copy of a shared helper nonterminal (`opt(X)@site`) that LR construction
makes for one co-occurrence class of the helper's use sites when the shared
helper's merged LALR(1) lookaheads are a reduce/reduce conflict. It carries the
original's rules, reduce actions and folds, so parsing through it builds the
same value and provenance.
_Avoid_: Cloned helper, split nonterminal, per-site helper

**Attachment unit**:
The private sequence type materialized for one semantic field with attached
fixed tokens (`#[tok(OPEN, this, CLOSE)]`): the fixed tokens around one
occurrence of the field's element. A cardinality-one unit lowers inline into
its owner's rule, so the owner's own reduce takes the field's value from its
right-hand-side position; every other unit keeps a nonterminal whose reduce
passes that value through unchanged.
_Avoid_: Surrounded node, per-element wrapper

**Presence envelope**:
The private sequence type materialized for a fixed-token presence with required
fixed tokens before or after its optional sequence (`#[tok(IF, this, EXISTS)]
#[presence(NOT)]`). It is a nonterminal with one alternative per presence value
whose reduce yields the `bool`.
_Avoid_: Presence wrapper, optional keyword node

**Declared precedence**:
A token-grammar declaration of ordered precedence levels, each left, right or
non-associative over token names and precedence-only names, and the sole source
of rule and token precedence for LR conflict resolution. A rule takes the level
of its last declared terminal unless its Node or variant names a level as an
override; a shift/reduce conflict compares the reducing rule's level with the
lookahead's level, and a tie follows the rule's associativity. A rule or
lookahead with no level leaves the conflict unresolved.
_Avoid_: Binding power, Pratt hierarchy, precedence climbing, %left line

**Precedence-only name**:
A member of a declared precedence level that names no token and exists only so
that a Node or variant override can take that level.
_Avoid_: Pseudo-token, UMINUS token

**Restricted expression**:
An authored Node that accepts a subset of another expression Node's productions
so that an interior operand cannot consume a token that also closes the enclosing
syntax. It is written by hand, as PostgreSQL writes b_expr, and is not derived
from an exclusion list.
_Avoid_: Exclusion set, Pratt clone, minimum binding power

**Parse capability**:
The sealed facade contract that gives a generated grammar type its safe
grammar-bound input entry.
_Avoid_: Manual parser trait, parse plan

**Node declaration**:
The representation-independent definition of a semantic Node's shape, child relationships, cardinalities, and grammar annotations. Node declarations are the canonical source from which concrete AST representations and parsing behavior are derived.
_Avoid_: Primary AST, concrete Rust type used as the schema

**AST style**:
The choice of concrete AST representation and ownership model used to realize Node declarations. It determines how declared child relationships and cardinalities are stored.
_Avoid_: Parser style, grammar variant

**Boxed child**:
An explicitly indirect child relationship whose cardinality is unchanged. Its concrete form is determined by the AST style; in the flat style it adds nothing, because every Node already has a handle.
_Avoid_: Optional child, implicitly boxed recursion

**Primary AST**:
The historical name for the authored nested AST when it served as the canonical grammar representation. In the declaration model, use Node declaration for the canonical definition and Nested AST for the owning representation.
_Avoid_: Preferred AST representation

**Nested AST**:
A semantic tree whose child relationships are represented directly by values and owning containers.
_Avoid_: Primary AST, normal AST, pointer AST

**Flat AST**:
A semantic AST representation whose Nodes live in pools and refer to one another through typed handles rather than owning child values. It retains semantic values, and one span per Node only when span capture is requested; it never retains comments or occurrence provenance.
_Avoid_: Secondary AST, packed AST, untyped node table, bump-allocated tree

**Typed pool**:
A per-result store holding every Node of one pooled Node type contiguously, addressed by Node handle. Nodes of a fresh parse appear in it in post-order: each child precedes its parent.
_Avoid_: Vec per type, hot table, arena

**Pooled Node**:
A Node declaration that opts into a typed pool. Every other Node lives in the shared pool.
_Avoid_: Hot node, tier-1 type

**Shared pool**:
The per-result store that holds every Node not in a typed pool, each occupying whole words at a typed word offset. It is not the bump arena selected by `arena_ast`.
_Avoid_: Arena, cold store, untyped node table

**Node handle**:
The four-byte typed handle of one Node in a Flat AST store. Its type parameter names the Node, and the Node's declaration decides whether it resolves into a typed pool or the shared pool; no storage tag is stored and field types do not change when a Node moves between pools.
_Avoid_: Pool index, shared reference, node pointer, raw index, u32 slot

**List handle**:
The eight-byte handle of one contiguous run of handles in a result's list store. An empty run and an absent list are distinct values; a one-or-more field's list handle cannot name an empty run.
_Avoid_: Child vector, index slice

**Text handle**:
The four-byte typed handle of one content token's text in a result's text pool. The text still borrows the input or owns its replacement; the pool, not the Node, owns it.
_Avoid_: Interned symbol, inline token, string ID

**Side table**:
A consumer-owned dense array with one value per Node of one typed pool, addressed by Node handle. It is not part of the Flat AST store.
_Avoid_: Node annotation field, type map

**Flat AST store**:
The per-result owner of one Flat AST's typed pools, shared pool, list store, and text pool, addressed by that result's handles. Its stores are append-only and its handles are stable. The structure reachable from its root has tree semantics: each non-root Node has one parent and there are no cycles.
_Avoid_: Shared global arena, untyped node table, Ast struct with public fields

**Construction backend**:
One caller-selectable, grammar-specific materialization of the representation-independent parser, with its own intermediate Node values and final output.
_Avoid_: Second parser, output mode, parser callback

**Selective construction**:
A construction backend that retains or derives values only for chosen Node families while the parser still recognizes the complete grammar.
_Avoid_: Partial parse, incomplete AST

**Node view**:
The concrete, visibility-matched occurrence view generated for a Node, pairing a semantic reference with its private provenance cursor while implementing Recursa's common view protocol.
_Avoid_: AST wrapper, span block

**Explorer adapter**:
The optional grammar-erased capability for strictly parsing, exploring, or sampling one source-visible Node family and exposing its result for explorer consumers.
_Avoid_: Dynamic tree adapter, explorer UI

**Explorer UI**:
The interactive local application that consumes explorer adapters and exploration results to inspect grammars, parsed trees, spans, diagnostics, samples, and rendered output.
_Avoid_: Explorer adapter, explorer backend

**Explorer document**:
A read-only explorer representation of one complete parsed or generated root and its occurrence provenance, bounded by the lifetime of the input from which it was produced.
_Avoid_: Partial Explorer document, owned AST snapshot, parsed root

**Explorer node**:
A grammar-erased read-only handle to one Node or content-token occurrence in an explorer document, without exposing its semantic Rust type or occurrence identity.
_Avoid_: Dynamic Node, Node key, AST address

**Explorer element**:
A read-only occurrence of one source-order grammar element belonging to the active Node shape or enum variant, carrying its schema, optional span, and child occurrences.
_Avoid_: Element ID, span-block slot

**Explorer schema**:
The feature-selected static names, roles, and token classifications that describe a grammar to dynamic consumers without exposing Rust types or numeric identities.
_Avoid_: Public registry, UI model

**Explorer capability**:
The explorer-adapter declaration that a configured Pretty or Arbitrary operation is available for one generated root.
_Avoid_: Runtime trait reflection, feature flag

**Explorer statistics**:
The optional elapsed parse time, elapsed lex time, and parse-arena memory usage returned alongside an Explorer parse, gated behind the `statistics` feature (which `explorer` enables transitively). Memory usage is reported only when the document is backed by a parse arena; a document without that storage reports no memory figure.
_Avoid_: Parse statistics, timing metadata

**Railroad diagram**:
A Rustdoc-embedded SVG that renders a Node's grammar structure or a content-bearing regex token through compositional railroad geometry. The generator publishes only the semantic diagram plan; the facade renders SVG and writes standalone files while `cargo doc` expands generated attributes.
_Avoid_: Syntax box, generated production string

**Diagram label**:
An optional author-facing name used as the terminal text for a content-bearing regex token's railroad diagram. Without one, the declared regex is used.
_Avoid_: Regex expansion, display override

**Pretty context**:
The facade-owned rendering context that combines grammar configuration with optional parsed provenance for structured Pretty emission.
_Avoid_: Formatter state, AST metadata

**Recursa diagnostic**:
A stable-coded, structured finding produced by grammar generation or its structural loaders.
_Avoid_: Panic message, opaque error string

**Build report**:
The deterministic collection of Recursa diagnostics produced before a generation is published.
_Avoid_: Loader output, compiler error stream

**Loader diagnostic**:
A post-publication Recursa diagnostic emitted by a structural root or Node loader at its compiler location.
_Avoid_: Build-report diagnostic, analyzer diagnostic

**Diagnostic location**:
A manifest-relative source path with an optional half-open UTF-8 byte range identifying where a Recursa diagnostic arose.
_Avoid_: Rendered line and column, fabricated span

**Diagnostic stage**:
The generation or loading phase that owns a Recursa diagnostic and determines which later phases depend on it.
_Avoid_: Stack frame, display category

**Canonical diagnostic rendering**:
The deterministic multiline plain-text representation of a structured Recursa diagnostic used for snapshots and generation failures.
_Avoid_: Cargo warning, loader output, debug dump

**Build diagnostic code registry**:
The append-only assignment of stable generation and loader diagnostic codes to failure classes, stages, severities, locations, evidence, and remedies. Runtime lexical and parse codes use separate matching-version contracts because they arise during runtime input recognition or matching-version boundary validation rather than generation or loading.
_Avoid_: Message catalogue, enum ordering

**Internal diagnostic**:
A Recursa diagnostic reporting a detected generator, publication, protocol, or loader invariant rather than invalid grammar source.
_Avoid_: Panic, user-source diagnostic

**Generation failure**:
The typed unsuccessful result of generation, containing every diagnostic collected before the publication barrier and rendering them once through Rust's standard error path.
_Avoid_: Pre-logged error, partial generation

**Loader cascade**:
A set of derivative loader failures caused by one root-wide generation or protocol failure. The root diagnostic represents the failure and suppresses those dependent reports.
_Avoid_: Independent Node-body failure, repeated root error

**Acceptance contract**:
The normative set of observable gates that must pass before a capability is complete, with each gate traced to its fixture, evidence procedure, expected result, and originating decision.
_Avoid_: Exit prose, implementation plan

**Legacy-case ledger**:
The 167-entry traceability inventory that maps every Recursa spike test case to executable replacement evidence or an explicit superseding contract.
_Avoid_: New test count, copied test suite

**Publication barrier**:
The boundary at which one complete successful generation becomes loadable; an attempted generation that fails before this boundary remains unselectable and never authorizes stale-output fallback.
_Avoid_: Output write, atomic rename

**Generated-code debug bundle**:
An optional, non-authoritative rendering of one successful generation's logical generated Rust for human inspection, with no effect on generation identity or compiled payload.
_Avoid_: Generated artifact, source snapshot
