# Documentation gates

Recursa treats authored Rust documentation as an architectural contract. Public APIs explain the
user-facing promise; private items explain ownership, invariants, and phase boundaries needed to
change the implementation safely. Documentation-hidden items are still a matching-version ABI for
generated code and must be documented even though normal Rustdoc output omits them.

CI applies one stable, three-layer gate to the seven authored library packages. The example packages
are consumers rather than library surfaces, so they remain outside this gate. Do not replace these
commands with a workspace-wide invocation that accidentally includes examples.

## 1. Authored library lint

Clippy checks public and private authored items for every library target. The explicit package list
is deliberate: it covers all Recursa libraries while excluding examples and binary-only targets.

```bash
cargo clippy \
  -p recursa \
  -p recursa-core \
  -p recursa-codegen \
  -p recursa-diagrams \
  -p recursa-macros \
  -p recursa-test \
  -p recursa-explorer \
  --lib --all-features --locked -- \
  -D warnings \
  -D missing-docs \
  -D clippy::missing-docs-in-private-items
```

Write documentation that states the item's role or contract. For plans and protocol values, record
ordering, identity, compatibility, and validation invariants. For long phase functions, explain why
the work remains together and identify its major phases. A comment that only repeats the identifier
does not satisfy the project contract even if it satisfies the compiler lint.

## 2. Documentation-hidden ABI policy

Stable Clippy intentionally exempts `#[doc(hidden)]` API, so `recursa-test` supplies a syntax-aware
policy test:

```bash
cargo test -p recursa-test --test documentation_policy --locked
```

The test parses the actual authored library module graph with `syn`. It checks feature-gated hidden
items without assuming a particular feature selection and requires documentation on public
descendants of a hidden module, type, or trait. It excludes modules that are provably test-only and
does not interpret macro token bodies, because those are test support or generated tokens rather
than authored library items. Public re-exports inherit the target item's documentation.

Keep this policy syntax-aware. Do not replace it with regular-expression source scanning, and do not
use unstable `--document-hidden-items` or a nightly toolchain.

## 3. Private-item Rustdoc

Rustdoc renders private authored items and rejects broken intra-doc links across the same explicit
library package set:

```bash
RUSTDOCFLAGS="--document-private-items -D rustdoc::broken_intra_doc_links" \
  cargo doc \
  -p recursa \
  -p recursa-core \
  -p recursa-codegen \
  -p recursa-diagrams \
  -p recursa-macros \
  -p recursa-test \
  -p recursa-explorer \
  --all-features --no-deps --locked
```

All three layers run on stable Rust and must remain compatible with the workspace MSRV, Rust 1.88.
