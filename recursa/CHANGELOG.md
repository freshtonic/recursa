# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/freshtonic/recursa/compare/recursa-v0.1.3...recursa-v0.1.4) - 2026-10-27

### Added

- *(framing)* frame one byte window of a document

## [0.1.3](https://github.com/freshtonic/recursa/compare/recursa-v0.1.2...recursa-v0.1.3) - 2026-10-26

### Added

- refuse one variant, and validate a Node when its reduce builds it

## [0.1.2](https://github.com/freshtonic/recursa/compare/recursa-v0.1.1...recursa-v0.1.2) - 2026-10-22

### Other

- compare the release stamp with the crate version

## [0.1.1](https://github.com/freshtonic/recursa/compare/recursa-v0.1.0...recursa-v0.1.1) - 2026-10-22

### Other

- release v0.1.1

## [0.1.0](https://github.com/freshtonic/recursa/releases/tag/recursa-v0.1.0) - 2026-10-16

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
- *(codegen)* generate named admission sets
- add grammar-erased explorer parsing ([#88](https://github.com/freshtonic/recursa/pull/88))
- generate opaque explorer adapters
- add structured pretty sink
- generate configured AST trait behavior
- generate compound nodes and views ([#64](https://github.com/freshtonic/recursa/pull/64))
- retire legacy parsing facade ([#63](https://github.com/freshtonic/recursa/pull/63))
- generate and load one Node ([#59](https://github.com/freshtonic/recursa/pull/59))
- add Parsed generated parsing facade ([#65](https://github.com/freshtonic/recursa/pull/65))
- produce span-aware nonempty output ([#22](https://github.com/freshtonic/recursa/pull/22))
- compose follow and report parse failures ([#21](https://github.com/freshtonic/recursa/pull/21))
- parse one token through facade ([#20](https://github.com/freshtonic/recursa/pull/20))

### Fixed

- admit a merged kind only where the grammar names it
- make a lookahead token absolute, as bison makes it
- resolve #[greedy(...)] names after lowering and point at the authored site
- *(codegen)* stabilize PostgreSQL grammar integration
- advance generation protocol for lexical output
- preserve closed matcher migration compatibility
- preserve parsed repetition separator provenance

### Other

- Merge branch 'fix/134-absolute-lookahead'
- satisfy the newer cargo manifest and rustdoc lints
- recycle flat store capacity and skip the text pool drop walk
- pre-size the flat store from the remaining token count
- apply the flat AST code review
- Support Arbitrary sampling for arena_ast grammars, with depth-limited termination
- Add optional span capture and LR metrics
- Add arena-backed generated AST support
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
- cover strict-only framing and Explorer, and record ADR 0003
- remove construction-free recovery from the runtime and generator
- complete package metadata and licensing ([#81](https://github.com/freshtonic/recursa/pull/81))
- rewrite workspace READMEs ([#78](https://github.com/freshtonic/recursa/pull/78))
- Generate repetitions and transactional provenance
- bootstrap recursa workspace (port-plan M0a)
