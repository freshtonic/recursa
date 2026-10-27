# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/freshtonic/recursa/compare/recursa-core-v0.1.3...recursa-core-v0.1.4) - 2026-10-27

### Added

- *(framing)* frame one byte window of a document

## [0.1.3](https://github.com/freshtonic/recursa/compare/recursa-core-v0.1.2...recursa-core-v0.1.3) - 2026-10-26

### Added

- refuse one variant, and validate a Node when its reduce builds it

### Fixed

- make Pretty and Arbitrary agree with restricted sites and the lexer

## [0.1.2](https://github.com/freshtonic/recursa/compare/recursa-core-v0.1.1...recursa-core-v0.1.2) - 2026-10-22

### Other

- compare the release stamp with the crate version

## [0.1.1](https://github.com/freshtonic/recursa/compare/recursa-core-v0.1.0...recursa-core-v0.1.1) - 2026-10-22

### Other

- release v0.1.1

## [0.1.0](https://github.com/freshtonic/recursa/releases/tag/recursa-core-v0.1.0) - 2026-10-16

### Added

- declared text rules on a struct field
- refuse a set or one item by its folded identifier text
- reject a set of items that a gram.y action rejects
- answer the requirement queries from the flat form
- give an arena-parsed value the same requirement spans as a value parse
- make a build-configuration gate a declaration that records its requirement
- traverse and render the Flat AST
- parse directly into the Flat AST
- add flat storage primitives and generated flat types
- declare AST nodes independently of storage
- make a #[greedy(...)] acceptance extend a Pratt expression (protocol 23)
- render railroad diagrams only under cargo doc
- partition exact source into strict islands and recovery regions
- extend construction-free recovery through Pratt expressions
- generate construction-free recovery for compound and container roots
- retain classified trivia in immutable source gaps
- *(codegen)* bound compound generation ([#101](https://github.com/freshtonic/recursa/pull/101))
- add fixed-token presence syntax
- remove authored lexer hooks
- add closed lexical matchers
- add grammar-erased explorer parsing ([#88](https://github.com/freshtonic/recursa/pull/88))
- generate opaque explorer adapters
- add grammar-local Pretty layout hints
- validate grammar-safe token spacing
- generate frozen pretty layouts
- add structured pretty sink
- preserve positional token attachments
- generate configured AST trait behavior
- render frozen Pratt parse plans
- render frozen predictive parse plans
- generate typed lexer callbacks
- generate compound nodes and views ([#64](https://github.com/freshtonic/recursa/pull/64))
- retire legacy parsing facade ([#63](https://github.com/freshtonic/recursa/pull/63))
- generate and load one Node ([#59](https://github.com/freshtonic/recursa/pull/59))
- add Parsed generated parsing facade ([#65](https://github.com/freshtonic/recursa/pull/65))
- produce span-aware nonempty output ([#22](https://github.com/freshtonic/recursa/pull/22))
- compose follow and report parse failures ([#21](https://github.com/freshtonic/recursa/pull/21))
- parse one token through facade ([#20](https://github.com/freshtonic/recursa/pull/20))

### Fixed

- settle an end-marker reduce/reduce between unit rules by declaration order
- admit a merged kind only where the grammar names it
- make a lookahead token absolute, as bison makes it
- write two negated is_some_and tests as is_none_or
- keep an inlined attachment unit's occurrence unchanged
- resolve #[greedy(...)] names after lowering and point at the authored site
- complete a strict final island that ends at EOF without its boundary
- *(codegen)* stabilize PostgreSQL grammar integration
- advance generation protocol for lexical output
- narrow lexical error contract
- settle lazy typed explorer generation
- make explorer documents lazy and typed
- integrate explorer with closed applications
- harden explorer generation contracts
- freeze typed Pratt Pretty emission plan
- preserve parsed repetition separator provenance
- preserve mapped Pretty attachment semantics
- close Pretty audit gaps
- close Pretty lexer and structure gaps
- derive Pratt and top-level pretty structure
- freeze lexer-safe pretty spacing
- preserve automatic punctuation boundaries
- make generic declarations application independent
- harden generated trait contracts

### Other

- Merge branch 'fix/134-absolute-lookahead'
- satisfy the newer cargo manifest and rustdoc lints
- link private items as code spans in public docs
- recycle flat store capacity and skip the text pool drop walk
- pre-size the flat store from the remaining token count
- apply the flat AST code review
- apply the standards review to the flat AST branch
- retire the Pratt acceptance gates and prove the tail predicate
- Let Shuffle generate deeper, larger statements without stack overflow
- Support Arbitrary sampling for arena_ast grammars, with depth-limited termination
- Add optional span capture and LR metrics
- Move boxed reductions directly into arena
- Co-locate LR rule metadata
- Avoid cloning generated lexical records
- Optimize provenance folding reductions
- Reduce LR state stack overhead
- Compact provenance folding state
- Add arena-backed generated AST support
- Arena-allocate parser provenance
- Optimize LR reduction dispatch
- Clean up LR-only generated code warnings
- Replace dual parser generation with sole LR pipeline
- support scoped LALR conflict resolutions (closes #132)
- make CaptureRequest crate-visible to match the collector's signature
- record the implemented named-parser shape ([#119](https://github.com/freshtonic/recursa/pull/119))
- lower provenance folds and tail decisions for the table-driven style ([#118](https://github.com/freshtonic/recursa/pull/118))
- fold an occurrence stack on reduce in the table-driven runtime ([#118](https://github.com/freshtonic/recursa/pull/118))
- Merge branch 'table-driven/114-containers' (closes #114)
- Merge branch 'table-driven/116-lookahead' (closes #116)
- prove the lookahead filter with a twin fixture ([#116](https://github.com/freshtonic/recursa/pull/116))
- core, codegen: apply lookahead filters in the table-driven token feed ([#116](https://github.com/freshtonic/recursa/pull/116))
- render the table-driven parser style end to end ([#113](https://github.com/freshtonic/recursa/pull/113))
- add the table-driven runtime: LR tables decoder, value stack and automaton loop ([#113](https://github.com/freshtonic/recursa/pull/113))
- state that framing and the explorer drive only the default parser
- add the parser style configuration and the renderer selection point
- add the parser-style marker to the hidden generated-parse protocol
- partition a framed document in one lex instead of one lex per island
- decode frozen dispatch tables once and index their edges by kind
- Merge branch 'perf/logos-lexer'
- replace the runtime lexer with one generated logos state machine
- bump the generation protocol to 19 for interned FOLLOW composition
- intern composed FOLLOW sets so parsing allocates none
- bump the generation protocol to 17 for #[greedy(...)]
- hash parse-plan identities structurally instead of through Debug text
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
- share the compiled lexer-matcher table across lex calls
- Fix Pratt operand boundaries and generated Pretty separators ([#103](https://github.com/freshtonic/recursa/pull/103))
- enforce authored documentation gates ([#84](https://github.com/freshtonic/recursa/pull/84))
- complete package metadata and licensing ([#81](https://github.com/freshtonic/recursa/pull/81))
- rewrite workspace READMEs ([#78](https://github.com/freshtonic/recursa/pull/78))
- document runtime and macro contracts ([#80](https://github.com/freshtonic/recursa/pull/80))
- replace milestone-coded names with capabilities ([#76](https://github.com/freshtonic/recursa/pull/76))
- exercise representative pretty grammars
- Generate repetitions and transactional provenance
- bootstrap recursa workspace (port-plan M0a)
