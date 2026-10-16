# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/freshtonic/recursa/releases/tag/recursa-explorer-v0.1.0) - 2026-10-16

### Added

- declare AST nodes independently of storage
- build interactive explorer UI ([#87](https://github.com/freshtonic/recursa/pull/87))
- add local explorer loopback server ([#83](https://github.com/freshtonic/recursa/pull/83))

### Fixed

- bound Explorer Pretty requests

### Other

- satisfy the newer cargo manifest and rustdoc lints
- link private items as code spans in public docs
- Hide enum-field labels in Explorer and add workspace design references
- Let Shuffle generate deeper, larger statements without stack overflow
- Isolate Arbitrary sampling in a subprocess and populate the source view from it
- Make Explorer resizable panels reach half the workspace, widen the source column's default
- Redesign the Explorer AST workspace: 3-column layout, five render modes, railroad diagrams
- Replace dual parser generation with sole LR pipeline
- complete package metadata and licensing ([#81](https://github.com/freshtonic/recursa/pull/81))
- rewrite workspace READMEs ([#78](https://github.com/freshtonic/recursa/pull/78))
- document runtime and macro contracts ([#80](https://github.com/freshtonic/recursa/pull/80))
- bootstrap recursa workspace (port-plan M0a)
