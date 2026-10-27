# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5](https://github.com/freshtonic/recursa/compare/recursa-codegen-v0.1.4...recursa-codegen-v0.1.5) - 2026-10-27

### Added

- classify through a declared projection, and omit a variant from sampling

## [0.1.4](https://github.com/freshtonic/recursa/compare/recursa-codegen-v0.1.3...recursa-codegen-v0.1.4) - 2026-10-27

### Added

- *(framing)* frame one byte window of a document

## [0.1.3](https://github.com/freshtonic/recursa/compare/recursa-codegen-v0.1.2...recursa-codegen-v0.1.3) - 2026-10-26

### Added

- refuse one variant, and validate a Node when its reduce builds it

### Fixed

- *(codegen)* bind a flat handle before indexing so clippy keeps quiet
- make Pretty and Arbitrary agree with restricted sites and the lexer

### Other

- *(codegen)* refuse a variant and validate a Node in every storage
- *(codegen)* round-trip sampled and repaired values in the fixtures

## [0.1.2](https://github.com/freshtonic/recursa/compare/recursa-codegen-v0.1.1...recursa-codegen-v0.1.2) - 2026-10-22

### Other

- set the recursa version in the fixture locks at each release
- *(codegen)* give the source-model Pretty prover to materialization
- *(codegen)* raise the Pretty catalog cache budget to 128 MiB

## [0.1.1](https://github.com/freshtonic/recursa/compare/recursa-codegen-v0.1.0...recursa-codegen-v0.1.1) - 2026-10-22

### Fixed

- *(codegen)* allow four more clippy lints on generated code

### Other

- *(codegen)* group the conflict findings of the LR dump by state once
- *(codegen)* build all goto columns in one pass over the states
- *(codegen)* start the comb-packing search at the first free slot
- *(codegen)* build only anchored lexical DFAs, and build each one once

## [0.1.0](https://github.com/freshtonic/recursa/releases/tag/recursa-codegen-v0.1.0) - 2026-10-16

### Added

- declared text rules on a struct field
- refuse a set or one item by its folded identifier text
- reject a set of items that a gram.y action rejects
- answer the requirement queries from the flat form
- give an arena-parsed value the same requirement spans as a value parse
- make a build-configuration gate a declaration that records its requirement
- *(codegen)* cfg on lookahead entries and an optional exclusion
- *(codegen)* cfg on tokens! matchers, literals and lexer_tokens entries
- *(codegen)* cfg on names in the tokens! precedence block
- *(codegen)* cfg on tokens! keyword entries
- restrict only the leading operand with #[restricts(Base, leading)]
- traverse and render the Flat AST
- parse directly into the Flat AST
- add flat storage primitives and generated flat types
- declare restricted expressions with #[restricts(Base)]
- derive Pretty parenthesization from declared precedence
- declare AST nodes independently of storage
- make a #[greedy(...)] acceptance extend a Pratt expression (protocol 23)
- accept declared greedy overlaps with #[greedy(...)]
- render railroad diagrams only under cargo doc
- partition exact source into strict islands and recovery regions
- extend construction-free recovery through Pratt expressions
- retain classified trivia in immutable source gaps
- retire nullable-FOLLOW warnings that frozen decisions prove resolved
- prove optional viability by element suffix under caller-FOLLOW overlap
- *(codegen)* bound compound generation ([#101](https://github.com/freshtonic/recursa/pull/101))
- add fixed-token presence syntax
- remove authored lexer hooks
- add closed lexical matchers
- *(codegen)* generate named admission sets
- add grammar-erased explorer parsing ([#88](https://github.com/freshtonic/recursa/pull/88))
- generate semantic railroad diagrams ([#86](https://github.com/freshtonic/recursa/pull/86))
- emit generated-code debug bundles
- generate opaque explorer adapters
- add grammar-local Pretty layout hints
- validate grammar-safe token spacing
- generate frozen pretty layouts
- preserve positional token attachments
- validate generated plan families
- enforce atomic generation publication
- publish generated grammar diagrams
- generate configured AST trait behavior
- render frozen Pratt parse plans
- render frozen predictive parse plans
- preserve matched token kinds and spelling
- validate post-lex grammar repairs
- generate typed lexer callbacks
- finalize alternative lexer records
- expose typed lexical error views
- generate compound nodes and views ([#64](https://github.com/freshtonic/recursa/pull/64))
- retire legacy parsing facade ([#63](https://github.com/freshtonic/recursa/pull/63))
- generate and load one Node ([#59](https://github.com/freshtonic/recursa/pull/59))
- complete M3 diagnostics and semantic projections ([#55](https://github.com/freshtonic/recursa/pull/55))
- plan lexical keyword semantics ([#54](https://github.com/freshtonic/recursa/pull/54))
- materialize generic grammar types ([#51](https://github.com/freshtonic/recursa/pull/51))
- lower tokens and trait plans ([#56](https://github.com/freshtonic/recursa/pull/56))
- resolve imports and token bindings ([#52](https://github.com/freshtonic/recursa/pull/52))
- resolve anchored semantic paths ([#58](https://github.com/freshtonic/recursa/pull/58))
- discover configured module trees ([#57](https://github.com/freshtonic/recursa/pull/57))
- lower single-file grammar sources ([#53](https://github.com/freshtonic/recursa/pull/53))
- generalize normalized grammar semantics ([#50](https://github.com/freshtonic/recursa/pull/50))
- disambiguate shared prefixes with FIRST-k ([#28](https://github.com/freshtonic/recursa/pull/28))
- warn about worst-case FOLLOW conflicts ([#29](https://github.com/freshtonic/recursa/pull/29))
- analyze Pratt conflicts ([#27](https://github.com/freshtonic/recursa/pull/27))
- explain overlaps with provenance ([#26](https://github.com/freshtonic/recursa/pull/26))
- collect left-recursion diagnostics ([#25](https://github.com/freshtonic/recursa/pull/25))
- analyze normalized grammar semantics ([#23](https://github.com/freshtonic/recursa/pull/23))

### Fixed

- make a lookahead token absolute, as bison makes it
- write two negated is_some_and tests as is_none_or
- keep Arbitrary seed types clippy-clean in a downstream crate
- allow match_like_matches_macro in generated flat traversal
- fold a level-less variant's Pretty parenthesization test to false
- keep an inlined attachment unit's occurrence unchanged
- identify unsupported grammar settings before parsing values
- resolve #[greedy(...)] names after lowering and point at the authored site
- never emit rerun-if-changed for a missing candidate module path
- complete a strict final island that ends at EOF without its boundary
- resolve explorer content-token schemas at the kind level
- keep witness-dead branches from poisoning the Pratt grouping proof
- resolve diagram links from every page and inline breadth-first
- cache railroad measurements and gate inline bodies by size
- bound per-diagram inline expansion
- *(codegen)* deduplicate Cargo warning directives
- *(codegen)* stabilize PostgreSQL grammar integration
- *(codegen)* preserve Pratt terminal fallbacks
- *(codegen)* scope downstream lint allowances
- *(analysis)* isolate caller follow evidence
- *(codegen)* stabilize generation plan identity
- *(codegen)* validate optional plan elision
- *(codegen)* sparsify optional viability planning
- *(codegen)* freeze optional viability symbolically
- *(codegen)* compose optional and Pratt decision plans
- *(codegen)* cut off unique Pratt-bearing enum paths
- *(codegen)* complete Pratt dispatch semantics ([#102](https://github.com/freshtonic/recursa/pull/102))
- bound FIRST-k residual analysis
- advance generation protocol for lexical output
- hide matcher invariants from generated errors
- narrow lexical error contract
- preserve closed matcher migration compatibility
- *(codegen)* canonicalize admission aliases
- align explorer and diagram acceptance (#86, #88)
- name generated explorer sample callback type
- satisfy final strict workspace lints ([#88](https://github.com/freshtonic/recursa/pull/88))
- integrate debug bundles with explorer output
- regenerate edited debug bundles
- validate emitted explorer symbol collisions
- settle lazy typed explorer generation
- gate explorer generation and retain Visit ownership
- integrate explorer with closed applications
- freeze explorer schema decisions
- harden explorer generation contracts
- preserve complete Pratt grouping syntax
- prove complete Pretty boundary languages
- freeze typed Pratt Pretty emission plan
- locate unsafe closed Pretty boundaries
- prove Pretty whitespace is ignored trivia
- prove crossing Pretty lexer prefixes
- validate closed generic Pretty boundaries
- preserve parsed repetition separator provenance
- freeze Pretty rendering decisions
- validate runtime-tight Pretty boundaries
- preserve mapped Pretty attachment semantics
- avoid cyclic concrete Pretty bounds
- close Pretty audit gaps
- close Pretty lexer and structure gaps
- derive Pratt and top-level pretty structure
- freeze lexer-safe pretty spacing
- preserve automatic punctuation boundaries
- canonicalize word repetition separators
- derive serde bounds for generic fields
- make generic declarations application independent
- preserve authored generic node declarations
- emit generic node applications
- integrate attachment pretty and diagrams
- type private attachment correlations
- harden generation publication contracts
- clean failed selector writes
- derive diagrams from frozen container plans
- close trait facade audit gaps
- harden generated trait contracts
- enforce lexical finalization invariants
- diagnose nullable Pratt shared prefixes (#27 #28)
- apply FIRST-k to Pratt conflicts (#27 #28)
- retain canonical extender provenance (#26 #27)
- collect independent atom cycles ([#25](https://github.com/freshtonic/recursa/pull/25))

### Other

- Merge branch 'perf/generated-item-count'
- embed the diagram plan once, not once per root Node
- one flat parse entry per crate, and no scan body below the baseline
- Merge branch 'fix/134-absolute-lookahead'
- apply rustfmt to four files it rewrites
- cover cfg_attr on scoped LR conflict resolutions
- break generated sources into lines so rustdoc stays linear
- satisfy the newer cargo manifest and rustdoc lints
- link private items as code spans in public docs
- recycle flat store capacity and skip the text pool drop walk
- pre-size the flat store from the remaining token count
- apply the flat AST code review
- apply the standards review to the flat AST branch
- retire the binding-power vocabulary from the retained surface
- describe the one precedence model everywhere
- retire the Pratt acceptance gates and prove the tail predicate
- restate every Pratt test against declared precedence
- remove the Pratt authoring surface and its consumers
- prove declared precedence through an expressions generation fixture
- rewrite the precedence fixture's Expr without #[pratt]
- rename the Pratt example to examples/expressions
- describe attachment-unit inlining where the lowering assumed a nonterminal
- Merge branch 'precedence/pretty' into precedence/declared-only
- Fix two Arbitrary-codegen bugs found by a plain (no-feature-flags) build
- Never budget-limit a variant's own first-level railroad diagram detail
- Fix invalid syntax in the zero-leaf-variant depth-guard branch
- Support per-declaration #[node(...)] trait overrides on central token entries
- Support Arbitrary sampling for arena_ast grammars, with depth-limited termination
- Redesign the Explorer AST workspace: 3-column layout, five render modes, railroad diagrams
- Add optional span capture and LR metrics
- Move boxed reductions directly into arena
- Optimize generated word classification
- Avoid cloning generated lexical records
- Add arena-backed generated AST support
- Arena-allocate parser provenance
- Optimize LR reduction dispatch
- Clean up LR-only generated code warnings
- Replace dual parser generation with sole LR pipeline
- support scoped LALR conflict resolutions (closes #132)
- prove the cyclic grouping shape renders end to end
- keep a grouping witness a cyclic sibling cannot hide (closes #130)
- keep an authored-input code out of the internal-invariant schema
- name a lookahead filter's merged kind in the block (closes #128)
- Merge branch 'table-driven/122-pratt-shapes' (closes #122, closes #123, closes #124)
- admit an atom or prefix variant in a Pratt operand exclusion (closes #122)
- admit a content-token operator on a prefix Pratt variant (closes #124)
- admit an attached optional Pratt operand (closes #123)
- prove the named-parser surface on a two-parser fixture ([#119](https://github.com/freshtonic/recursa/pull/119))
- generate every twin fixture once with both parsers ([#119](https://github.com/freshtonic/recursa/pull/119))
- render every declared parser in one generation ([#119](https://github.com/freshtonic/recursa/pull/119))
- compare provenance, Pretty and root acceptance between the parser styles ([#118](https://github.com/freshtonic/recursa/pull/118))
- lower provenance folds and tail decisions for the table-driven style ([#118](https://github.com/freshtonic/recursa/pull/118))
- Merge branch 'table-driven/114-containers' (closes #114)
- Merge branch 'table-driven/115-pratt' (closes #115)
- test the equal-level tie of prefix and postfix rules; share the token builder ([#115](https://github.com/freshtonic/recursa/pull/115))
- render Pratt reduce arms in the table-driven style ([#115](https://github.com/freshtonic/recursa/pull/115))
- lower operand minimum binding powers to restriction clones ([#115](https://github.com/freshtonic/recursa/pull/115))
- clone a Pratt nonterminal once per distinct exclusion set ([#115](https://github.com/freshtonic/recursa/pull/115))
- resolve Pratt shift/reduce conflicts by binding-power precedence ([#115](https://github.com/freshtonic/recursa/pull/115))
- make node rendering fallible and record the section 8 deviations ([#113](https://github.com/freshtonic/recursa/pull/113))
- render the table-driven parser style end to end ([#113](https://github.com/freshtonic/recursa/pull/113))
- Merge branch 'table-driven/112-lalr' (closes #112)
- expose LrAnalysis::analyze_source and run the pg-sql construction as an integration test
- lower the normalized grammar to LALR(1) tables with conflict diagnostics ([#112](https://github.com/freshtonic/recursa/pull/112))
- pin that a payload interior cannot change how later text lexes
- decode frozen dispatch tables once and index their edges by kind
- Merge branch 'feat/greedy-tooltips'
- move greedy acceptances from inline text to hover tooltips
- Merge branch 'perf/follow-site-cache'
- answer FOLLOW composition from per-site caches and static Pratt guards
- bump the generation protocol to 19 for interned FOLLOW composition
- intern composed FOLLOW sets so parsing allocates none
- drop the unused BTreeSet import left by greedy name resolution
- state the greedy default and suggest the acceptance in RCA0300 and RCA0301
- bump the generation protocol to 17 for #[greedy(...)]
- hash parse-plan identities structurally instead of through Debug text
- split generated items at the token level for lint scoping
- intern decision states by precomputed hash and use a deterministic fast hasher in analysis
- reuse a published Generation when the grammar identity is unchanged
- bump the generation protocol to 15 for the size-reduction batch
- replace generated view structs with one generic View<'view, T>
- drive explorer child access from element metadata
- build explorer root adapters from generic runtime functions
- interpret frozen dispatch through one runtime table walker
- parse direct children and wrapping variants through runtime helpers
- replace per-node forwarding impls with facade blankets
- emit one visit walk per node and compact visit events
- cover strict-only framing and Explorer, and record ADR 0003
- remove construction-free recovery from the runtime and generator
- share the typed-kind table across content-token parsers
- Merge branch 'claude/lint-scope-boundary' into claude/generated-size-reduction
- follow the diagram plan directory and protocol 13 in one-node fixtures
- Merge branch 'capability/issue-94-compound-recovery'
- Merge branch 'capability/issue-93-retained-trivia'
- keep recursa-codegen clippy-clean across all targets
- harden the suffix-proving viability seam after review
- Merge branch 'diagrams/pg-proto-port' into capability/optional-suffix-viability
- derive the generation identity from semantic plans, not source text
- Optimize emission decision lookups
- *(codegen)* clarify transient proof controls
- *(codegen)* retain transient generation proofs
- Preserve mixed caller follow diagnostics
- Resolve predictive optional path viability
- Fix Pratt operand boundaries and generated Pretty separators ([#103](https://github.com/freshtonic/recursa/pull/103))
- bound PostgreSQL conflict analysis
- bound static conflict products
- share bounded FIRST-k analysis support
- canonicalize bounded FIRST-k languages
- tighten obsolete hook diagnostics
- *(codegen)* reuse compound fixture dependencies
- *(codegen)* move collision checks to planner seam
- keep performance evidence opt-in
- stabilize nested Cargo fixtures
- normalize nested Cargo diagnostics
- finish review contract cleanup
- make compound fixture locks CI-clean
- strengthen source analysis contracts ([#77](https://github.com/freshtonic/recursa/pull/77))
- replace codegen boilerplate contracts
- enforce authored documentation gates ([#84](https://github.com/freshtonic/recursa/pull/84))
- document generation and diagnostics ([#82](https://github.com/freshtonic/recursa/pull/82))
- decompose generation capabilities ([#79](https://github.com/freshtonic/recursa/pull/79))
- complete package metadata and licensing ([#81](https://github.com/freshtonic/recursa/pull/81))
- rewrite workspace READMEs ([#78](https://github.com/freshtonic/recursa/pull/78))
- cover source and semantic analysis internals ([#77](https://github.com/freshtonic/recursa/pull/77))
- replace milestone-coded names with capabilities ([#76](https://github.com/freshtonic/recursa/pull/76))
- execute direct legacy ledger successors
- close the M4 functional evidence matrix
- activate final explorer acceptance evidence
- restore exact legacy runtime evidence
- activate complete legacy acceptance evidence
- port legacy grammar runtime cases
- inventory frozen legacy acceptance cases
- freeze typed explorer schema plans
- keep Pretty helper within lint limits
- freeze Pretty hint family authority
- exercise representative pretty grammars
- format generic attachment integration
- close attachment coverage gaps
- preserve native diagnostics for corrupt node output
- prove warnings cross publication barrier
- render frozen Pratt diagram plans
- Generate repetitions and transactional provenance
- exercise real Pratt recursion ([#24](https://github.com/freshtonic/recursa/pull/24))
- reach arithmetic and JSON parity ([#24](https://github.com/freshtonic/recursa/pull/24))
- bootstrap recursa workspace (port-plan M0a)
