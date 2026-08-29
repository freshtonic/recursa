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
Its records must be source-bounded UTF-8, ordered, and non-overlapping; unrecorded source gaps are preserved without rescanning. Classified trivia records and observable trivia-gap behavior are introduced by #93.
_Avoid_: Raw token-record constructor

**Lowered grammar**:
The immutable aggregate that keeps a normalized grammar together with its authoring-source context and their correlations.
_Avoid_: Emission model, enriched grammar description

**Source analysis**:
The interpretation of an authored Rust module tree into a source model and lowered grammar, including grammar-semantic resolution and source correlations.
_Avoid_: M3, source parsing

**Semantic analysis**:
The source-independent computation of nullable, FIRST, FOLLOW, dispatch, conflict, Pratt, and provenance facts over a normalized grammar.
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
A one-or-more semantic repetition represented by `recursa::Vec1<T>`, whose public construction and mutation API makes an empty value unrepresentable. Its storage implementation is not part of the grammar meaning.
_Avoid_: Validated `Vec<T>`, non-empty parser metadata

**Grammar type application**:
A resolved use of a Node declaration with concrete semantic type arguments. Its Node key remains the declaration's logical identity; type arguments distinguish materialized grammar applications, while lifetime arguments are retained for emission but do not distinguish grammar identity.
_Avoid_: Node key, Rust monomorphization artifact

**Container shape**:
The recursive composition of supported `Option`, `Box`, `Vec`, and `Vec1` constructors around one resolved Node application or generated content-token type. `Box` may occur anywhere; after ignoring `Box` layers, the accepted normal forms are `T`, `Option<T>`, `Vec<T>`, `Vec1<T>`, and `Option<Vec1<T>>`.
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
The source-aware closed set of documented public bindings a generation introduces while preserving the visibility of their source declarations.
_Avoid_: Generated prelude, hidden loader surface

**Balanced delimiter pair**:
A normalized pair of distinct fixed opening and closing token kinds whose properly nested occurrences may be proven structurally during analysis.
_Avoid_: Arbitrary punctuation pair, runtime delimiter heuristic

**Balanced dispatch**:
A frozen predictive selection strategy for an otherwise unbounded shared opener prefix. Analysis proves the enclosed language balanced for one delimiter pair and freezes the residual post-close trie; generated parsing only counts that pair's nesting and executes the frozen residual decision.
_Avoid_: Bounded FIRST expansion, runtime grammar rediscovery

**Optional viability decision**:
A frozen predictive consume-or-skip strategy for one optional parse site. Analysis compares one occurrence plus the owning sequence tail against that tail alone, retaining exact token-text partitions and an explicit caller-FOLLOW fallback; generated parsing only executes the frozen decision.
_Avoid_: Trial parsing, first-token optional commitment

**Suffix-proving viability**:
The optional viability decision variant for an element whose FIRST overlaps caller FOLLOW and whose bounded prefix language is closed. The trie walks the element's own prefixes; a fully matched or depth-exhausted prefix consumes unconditionally, and a sticky unconditional Skip fallback below the root returns diverging input to the caller. Open element languages keep the greedy legacy selector.
_Avoid_: Backtracking optional, speculative parse

**Parse plan**:
The immutable executable plan for parsing one materialized grammar type, formed by joining source-independent semantic decisions with source construction and capture facts. Generation renders it without rediscovering grammar semantics.
_Avoid_: Generated parse body, analysis facts

**Parse capability**:
The sealed facade contract that gives a generated grammar type its safe grammar-bound input entry while keeping recursive FOLLOW-aware operations private.
_Avoid_: Manual parser trait, parse plan

**Nested AST**:
An authored semantic tree whose child relationships are represented directly by Rust values and owning containers.
_Avoid_: Normal AST, pointer AST

**Flat AST**:
A generated semantic tree whose Nodes live in typed arenas and refer to one another through typed indices rather than owning child values.
_Avoid_: Packed AST, untyped node table, bump-allocated tree

**Construction backend**:
One caller-selectable, grammar-specific materialization of the representation-independent parser, with its own intermediate Node values and final output.
_Avoid_: Second parser, output mode, parser callback

**Selective construction**:
A construction backend that retains or derives values only for chosen Node families while the parser still recognizes the complete grammar.
_Avoid_: Partial parse, incomplete AST

**FOLLOW recipe**:
A per-parse-site runtime composition fixed by analysis: static suffix FIRST, caller FOLLOW, or their union when the suffix is nullable.
_Avoid_: Static type FOLLOW, emitter-derived FOLLOW

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

**Exploration**:
The mutually exclusive Explorer result of inspecting source: either a complete Explorer document or a recovery document produced after strict parsing fails.
_Avoid_: Parse result, partial AST

**Recovery document**:
An owned, source-borrowing Explorer projection of recognized, missing, and skipped grammar structure that cannot contain or become an authored AST value.
_Avoid_: Partial Explorer document, recovered AST

**Recovery node**:
A grammar-shaped node in a recovery document whose variant exists only after unique dispatch and whose parts may describe incomplete source.
_Avoid_: Error Node, invalid AST node

**Recovery part**:
A source-ordered recognized element or separator, zero-width missing marker, or skipped source span owned by a recovery node.
_Avoid_: Synthetic AST field, inserted token

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
The explorer-adapter declaration that recovery or a configured Pretty or Arbitrary operation is available for one generated root.
_Avoid_: Runtime trait reflection, feature flag

**Railroad diagram**:
A Rustdoc-embedded SVG that renders a Node's grammar structure or a content-bearing regex token through compositional railroad geometry.
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

**Semantic projection**:
The canonical source-independent representation used to compare accepted grammar meaning and behavior across implementations while excluding provenance, spans, build-local identities, and physical output layout.
_Avoid_: Structural equality, generated-source snapshot

**Publication barrier**:
The boundary at which one complete successful generation becomes loadable; an attempted generation that fails before this boundary remains unselectable and never authorizes stale-output fallback.
_Avoid_: Output write, atomic rename

**Generated-code debug bundle**:
An optional, non-authoritative rendering of one successful generation's logical generated Rust for human inspection, with no effect on generation identity or compiled payload.
_Avoid_: Generated artifact, source snapshot
