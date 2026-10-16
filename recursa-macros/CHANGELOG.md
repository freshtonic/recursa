# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/freshtonic/recursa/releases/tag/recursa-macros-v0.1.0) - 2026-10-16

### Added

- declared text rules on a struct field
- refuse a set or one item by its folded identifier text
- reject a set of items that a gram.y action rejects
- answer the requirement queries from the flat form
- make a build-configuration gate a declaration that records its requirement
- *(codegen)* cfg on lookahead entries and an optional exclusion
- *(codegen)* cfg on tokens! matchers, literals and lexer_tokens entries
- *(codegen)* cfg on names in the tokens! precedence block
- *(codegen)* cfg on tokens! keyword entries
- traverse and render the Flat AST
- parse directly into the Flat AST
- add flat storage primitives and generated flat types
- declare restricted expressions with #[restricts(Base)]
- declare AST nodes independently of storage
- make a #[greedy(...)] acceptance extend a Pratt expression (protocol 23)
- accept declared greedy overlaps with #[greedy(...)]
- render railroad diagrams only under cargo doc
- partition exact source into strict islands and recovery regions
- extend construction-free recovery through Pratt expressions
- generate construction-free recovery for compound and container roots
- retain classified trivia in immutable source gaps
- *(codegen)* bound compound generation ([#101](https://github.com/freshtonic/recursa/pull/101))
- add fixed-token presence syntax
- remove authored lexer hooks
- add closed lexical matchers
- *(codegen)* generate named admission sets
- add grammar-erased explorer parsing ([#88](https://github.com/freshtonic/recursa/pull/88))
- generate opaque explorer adapters
- add grammar-local Pretty layout hints
- enforce atomic generation publication
- generate configured AST trait behavior
- generate and load one Node ([#59](https://github.com/freshtonic/recursa/pull/59))

### Fixed

- admit a merged kind only where the grammar names it
- make a lookahead token absolute, as bison makes it
- resolve #[greedy(...)] names after lowering and point at the authored site
- *(codegen)* stabilize PostgreSQL grammar integration
- advance generation protocol for lexical output
- preserve closed matcher migration compatibility
- *(codegen)* canonicalize admission aliases
- close trait facade audit gaps
- harden generated trait contracts

### Other

- satisfy the newer cargo manifest and rustdoc lints
- recycle flat store capacity and skip the text pool drop walk
- pre-size the flat store from the remaining token count
- apply the flat AST code review
- describe the one precedence model everywhere
- rename the Pratt example to examples/expressions
- Add optional span capture and LR metrics
- Replace dual parser generation with sole LR pipeline
- lower provenance folds and tail decisions for the table-driven style ([#118](https://github.com/freshtonic/recursa/pull/118))
- emit per-terminal expected atoms and check error parity on the twin fixture ([#117](https://github.com/freshtonic/recursa/pull/117))
- render the table-driven parser style end to end ([#113](https://github.com/freshtonic/recursa/pull/113))
- decode frozen dispatch tables once and index their edges by kind
- replace the runtime lexer with one generated logos state machine
- bump the generation protocol to 19 for interned FOLLOW composition
- bump the generation protocol to 17 for #[greedy(...)]
- hash parse-plan identities structurally instead of through Debug text
- bump the generation protocol to 15 for the size-reduction batch
- remove construction-free recovery from the runtime and generator
- complete package metadata and licensing ([#81](https://github.com/freshtonic/recursa/pull/81))
- rewrite workspace READMEs ([#78](https://github.com/freshtonic/recursa/pull/78))
- document runtime and macro contracts ([#80](https://github.com/freshtonic/recursa/pull/80))
- Generate repetitions and transactional provenance
