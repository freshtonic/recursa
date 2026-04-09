# Visitor Pattern Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add AST traversal via the visitor pattern: `Visit`, `Visitor`, `NodeKey`, `Break`, `AsNodeKey` traits/types, plus `#[derive(Visit)]` macro and updates to bulk macros.

**Architecture:** Core types (`Break`, `NodeKey`, `AsNodeKey`, `Visit`, `Visitor`) in `recursa-core`. Derive macro (`#[derive(Visit)]`) in `recursa-derive`. Blanket impls for `Box`, `Option`, `Seq`. Bulk macros updated to derive `Visit`. `literals!` macro changed to generate owned `String` types.

**Tech Stack:** Rust, `syn`/`quote`/`proc-macro2`

**Design doc:** `docs/plans/2026-04-09-visitor-design.md`

---

## Task 1: Break Enum and NodeKey

Add the `Break<E>` enum and `NodeKey<'ast>` struct to `recursa-core`.

**Files:**
- Create: `recursa-core/src/visitor.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the failing tests**

Add to `recursa-core/src/lib.rs` test module:

```rust
use std::any::TypeId;
use crate::visitor::{Break, NodeKey};

#[test]
fn node_key_equality() {
    let x = 42u32;
    let k1 = NodeKey::new(&x);
    let k2 = NodeKey::new(&x);
    assert_eq!(k1, k2);
}

#[test]
fn node_key_different_nodes() {
    let x = 42u32;
    let y = 42u32;
    let k1 = NodeKey::new(&x);
    let k2 = NodeKey::new(&y);
    assert_ne!(k1, k2); // different addresses
}

#[test]
fn node_key_get_as() {
    let x = 42u32;
    let k = NodeKey::new(&x);
    assert_eq!(k.get_as::<u32>(), Some(&42));
    assert_eq!(k.get_as::<i32>(), None); // wrong type
}

#[test]
fn node_key_hashable() {
    use std::collections::HashMap;
    let x = 42u32;
    let k = NodeKey::new(&x);
    let mut map = HashMap::new();
    map.insert(k, "found");
    assert_eq!(map.get(&k), Some(&"found"));
}

#[test]
fn break_variants() {
    let _skip: Break<String> = Break::SkipChildren;
    let _fin: Break<String> = Break::Finished;
    let _err: Break<String> = Break::Err("oops".to_string());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `visitor` module doesn't exist.

**Step 3: Implement**

Create `recursa-core/src/visitor.rs`:

```rust
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::ops::ControlFlow;

/// Control flow for visitor traversal.
#[derive(Debug)]
pub enum Break<E> {
    /// Skip visiting child nodes of the current node.
    SkipChildren,
    /// Traversal is complete, stop early.
    Finished,
    /// An error occurred during traversal.
    Err(E),
}

/// Type-erased handle to an AST node, usable as a HashMap key.
#[derive(Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Clone, Copy)]
pub struct NodeKey<'ast> {
    node_addr: usize,
    node_type: TypeId,
    _ast: PhantomData<&'ast ()>,
}

impl<'ast> NodeKey<'ast> {
    /// Create a NodeKey from a reference to a node.
    pub fn new<N: 'static>(node: &'ast N) -> Self {
        Self {
            node_addr: node as *const N as usize,
            node_type: TypeId::of::<N>(),
            _ast: PhantomData,
        }
    }

    /// Retrieve the original node reference if the type matches.
    pub fn get_as<N: 'static>(&self) -> Option<&'ast N> {
        if self.node_type == TypeId::of::<N>() {
            unsafe { (self.node_addr as *const N).as_ref() }
        } else {
            None
        }
    }
}

/// Trait for types that can produce a NodeKey.
pub trait AsNodeKey: 'static {
    fn as_node_key(&self) -> NodeKey<'_> {
        NodeKey::new(self)
    }
}
```

Note: `AsNodeKey` has a default implementation since `NodeKey::new` works for any `'static` type. Individual types just need to impl the trait with no body.

Update `recursa-core/src/lib.rs`:

```rust
pub mod visitor;

pub use visitor::{Break, NodeKey, AsNodeKey};
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/visitor.rs recursa-core/src/lib.rs
git commit -m "Add Break enum, NodeKey, and AsNodeKey trait"
```

---

## Task 2: Visit and Visitor Traits

Add the `Visit` and `Visitor` traits to `recursa-core`.

**Files:**
- Modify: `recursa-core/src/visitor.rs`
- Modify: `recursa-core/src/lib.rs`

**Step 1: Write the failing tests**

Add to `recursa-core/src/lib.rs` test module:

```rust
use std::ops::ControlFlow;
use crate::visitor::{Visit, Visitor};

// A simple manual Visit impl for testing
struct Leaf(i32);

impl AsNodeKey for Leaf {}

impl Visit for Leaf {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}

struct Counter {
    enter_count: usize,
    exit_count: usize,
}

impl Visitor for Counter {
    type Error = ();

    fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
        self.enter_count += 1;
        ControlFlow::Continue(())
    }

    fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
        self.exit_count += 1;
        ControlFlow::Continue(())
    }
}

#[test]
fn visitor_enter_exit_called() {
    let leaf = Leaf(42);
    let mut counter = Counter { enter_count: 0, exit_count: 0 };
    leaf.visit(&mut counter);
    assert_eq!(counter.enter_count, 1);
    assert_eq!(counter.exit_count, 1);
}

#[test]
fn visitor_downcast_in_enter() {
    struct TypeChecker { found_leaf: bool }
    impl Visitor for TypeChecker {
        type Error = ();
        fn enter<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<Self::Error>> {
            if let Some(leaf) = node.downcast_ref::<Leaf>() {
                self.found_leaf = true;
                assert_eq!(leaf.0, 42);
            }
            ControlFlow::Continue(())
        }
    }

    let leaf = Leaf(42);
    let mut checker = TypeChecker { found_leaf: false };
    leaf.visit(&mut checker);
    assert!(checker.found_leaf);
}

#[test]
fn visitor_skip_children() {
    // SkipChildren should not propagate up as an error
    struct Skipper;
    impl Visitor for Skipper {
        type Error = ();
        fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
            ControlFlow::Break(Break::SkipChildren)
        }
        fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
            ControlFlow::Continue(())
        }
    }

    let leaf = Leaf(42);
    let mut skipper = Skipper;
    let result = leaf.visit(&mut skipper);
    assert!(matches!(result, ControlFlow::Continue(())));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — `Visit` and `Visitor` not defined.

**Step 3: Implement**

Add to `recursa-core/src/visitor.rs`:

```rust
/// Marks AST types as traversable via the visitor pattern.
///
/// The `accept` method drives traversal by calling `visitor.enter(self)`,
/// visiting children, then `visitor.exit(self)`.
pub trait Visit: 'static + Sized + AsNodeKey {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>>;

    fn downcast_ref<Target: Visit>(&self) -> Option<&Target> {
        (self as &dyn Any).downcast_ref::<Target>()
    }

    fn is<Target: Visit>(&self) -> bool {
        (self as &dyn Any).is::<Target>()
    }
}

/// Defines hooks called during AST traversal.
///
/// Override `enter` and/or `exit` to inspect nodes. Use `downcast_ref`
/// inside the body to check for specific node types.
pub trait Visitor: Sized {
    type Error;

    fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
        ControlFlow::Continue(())
    }

    fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
        ControlFlow::Continue(())
    }
}
```

Update `recursa-core/src/lib.rs` re-exports:

```rust
pub use visitor::{Break, NodeKey, AsNodeKey, Visit, Visitor};
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/visitor.rs recursa-core/src/lib.rs
git commit -m "Add Visit and Visitor traits"
```

---

## Task 3: Blanket Visit Impls for Box, Option, Seq

Add transparent `Visit` impls for container types.

**Files:**
- Modify: `recursa-core/src/visitor.rs`
- Modify: `recursa-core/src/seq.rs` (may need `'static` bounds)
- Modify: `recursa-core/src/lib.rs` (tests)

**Step 1: Write the failing tests**

Add to `recursa-core/src/lib.rs` test module:

```rust
#[test]
fn visit_box_delegates() {
    let boxed = Box::new(Leaf(99));
    let mut counter = Counter { enter_count: 0, exit_count: 0 };
    boxed.visit(&mut counter);
    assert_eq!(counter.enter_count, 1); // Leaf's enter, not Box's
    assert_eq!(counter.exit_count, 1);
}

#[test]
fn visit_option_some() {
    let opt = Some(Leaf(1));
    let mut counter = Counter { enter_count: 0, exit_count: 0 };
    opt.visit(&mut counter);
    assert_eq!(counter.enter_count, 1);
}

#[test]
fn visit_option_none() {
    let opt: Option<Leaf> = None;
    let mut counter = Counter { enter_count: 0, exit_count: 0 };
    opt.visit(&mut counter);
    assert_eq!(counter.enter_count, 0);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-core`
Expected: FAIL — no `Visit` impl for `Box<Leaf>` or `Option<Leaf>`.

**Step 3: Implement**

Add to `recursa-core/src/visitor.rs`:

```rust
impl<T: Visit> AsNodeKey for Box<T> {}
impl<T: Visit> Visit for Box<T> {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        (**self).visit(visitor)
    }
}

impl<T: Visit> AsNodeKey for Option<T> {}
impl<T: Visit> Visit for Option<T> {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        if let Some(inner) = self {
            inner.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}
```

For `Seq`, add to `recursa-core/src/visitor.rs` (or `seq.rs`):

```rust
impl<T: Visit, S: Visit, Trailing: 'static, Empty: 'static> AsNodeKey for Seq<T, S, Trailing, Empty>
where
    T: Clone,
    S: Clone,
{}

impl<T: Visit, S: Visit, Trailing: 'static, Empty: 'static> Visit for Seq<T, S, Trailing, Empty>
where
    T: Clone,
    S: Clone,
{
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        for (element, sep) in self.pairs() {
            element.visit(visitor)?;
            if let Some(sep) = sep {
                sep.visit(visitor)?;
            }
        }
        ControlFlow::Continue(())
    }
}
```

Note: `Seq`'s `Visit` impl needs `Trailing: 'static` and `Empty: 'static` to satisfy `AsNodeKey`'s `'static` bound. The marker types (`NoTrailing`, etc.) are already `'static`.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-core`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-core/src/visitor.rs recursa-core/src/seq.rs recursa-core/src/lib.rs
git commit -m "Add Visit impls for Box, Option, and Seq"
```

---

## Task 4: derive(Visit) Macro

Add the `#[derive(Visit)]` proc macro to `recursa-derive`.

**Files:**
- Create: `recursa-derive/src/visit_derive.rs`
- Modify: `recursa-derive/src/lib.rs`
- Create: `recursa-derive/tests/visit.rs`

**Step 1: Write the failing tests**

Create `recursa-derive/tests/visit.rs`:

```rust
use std::ops::ControlFlow;
use recursa_core::{AsNodeKey, Break, Visit, Visitor};
use recursa_derive::Visit;

// -- Leaf types (manual Visit impl for testing) --

struct Token;
impl AsNodeKey for Token {}
impl Visit for Token {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(())) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}

// -- Derived Visit --

#[derive(Visit)]
struct TwoTokens {
    a: Token,
    b: Token,
}

#[derive(Visit)]
enum Choice {
    First(Token),
    Second(TwoTokens),
}

// -- Counter visitor --

struct Counter { enters: usize, exits: usize }
impl Visitor for Counter {
    type Error = ();
    fn enter<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
        self.enters += 1;
        ControlFlow::Continue(())
    }
    fn exit<N: Visit>(&mut self, _node: &N) -> ControlFlow<Break<()>> {
        self.exits += 1;
        ControlFlow::Continue(())
    }
}

#[test]
fn visit_struct_visits_fields() {
    let node = TwoTokens { a: Token, b: Token };
    let mut c = Counter { enters: 0, exits: 0 };
    node.visit(&mut c);
    // TwoTokens enter + Token a enter + Token a exit + Token b enter + Token b exit + TwoTokens exit
    assert_eq!(c.enters, 3);
    assert_eq!(c.exits, 3);
}

#[test]
fn visit_enum_visits_variant() {
    let node = Choice::First(Token);
    let mut c = Counter { enters: 0, exits: 0 };
    node.visit(&mut c);
    // Choice enter + Token enter + Token exit + Choice exit
    assert_eq!(c.enters, 2);
    assert_eq!(c.exits, 2);
}

#[test]
fn visit_skip_children() {
    struct SkipTwoTokens;
    impl Visitor for SkipTwoTokens {
        type Error = ();
        fn enter<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<()>> {
            if node.is::<TwoTokens>() {
                ControlFlow::Break(Break::SkipChildren)
            } else {
                ControlFlow::Continue(())
            }
        }
    }

    let node = TwoTokens { a: Token, b: Token };
    let mut s = SkipTwoTokens;
    let result = node.visit(&mut s);
    // Should complete successfully (SkipChildren is not an error)
    assert!(matches!(result, ControlFlow::Continue(())));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p recursa-derive -- visit`
Expected: FAIL — `Visit` derive macro not defined.

**Step 3: Implement**

Create `recursa-derive/src/visit_derive.rs`:

```rust
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn derive_visit(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let accept_body = match &input.data {
        Data::Struct(data) => derive_visit_struct(data)?,
        Data::Enum(data) => derive_visit_enum(data)?,
        _ => return Err(syn::Error::new_spanned(name, "Visit can only be derived for structs and enums")),
    };

    Ok(quote! {
        impl #impl_generics ::recursa_core::AsNodeKey for #name #ty_generics #where_clause {}

        impl #impl_generics ::recursa_core::Visit for #name #ty_generics #where_clause {
            fn visit<V: ::recursa_core::Visitor>(
                &self,
                visitor: &mut V,
            ) -> ::std::ops::ControlFlow<::recursa_core::Break<V::Error>> {
                match ::recursa_core::Visitor::enter(visitor, self) {
                    ::std::ops::ControlFlow::Continue(()) => {
                        #accept_body
                    }
                    ::std::ops::ControlFlow::Break(::recursa_core::Break::SkipChildren) => {}
                    other => return other,
                }
                ::recursa_core::Visitor::exit(visitor, self)
            }
        }
    })
}

fn derive_visit_struct(data: &syn::DataStruct) -> syn::Result<TokenStream> {
    let field_visits: Vec<_> = match &data.fields {
        Fields::Named(fields) => fields.named.iter().map(|f| {
            let name = &f.ident;
            quote! { ::recursa_core::Visit::visit(&self.#name, visitor)?; }
        }).collect(),
        Fields::Unnamed(fields) => fields.unnamed.iter().enumerate().map(|(i, _)| {
            let idx = syn::Index::from(i);
            quote! { ::recursa_core::Visit::visit(&self.#idx, visitor)?; }
        }).collect(),
        Fields::Unit => vec![],
    };

    Ok(quote! { #(#field_visits)* })
}

fn derive_visit_enum(data: &syn::DataEnum) -> syn::Result<TokenStream> {
    let match_arms: Vec<_> = data.variants.iter().map(|variant| {
        let vname = &variant.ident;
        match &variant.fields {
            Fields::Unnamed(fields) => {
                let bindings: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("__f{i}"), proc_macro2::Span::call_site()))
                    .collect();
                let visits: Vec<_> = bindings.iter().map(|b| {
                    quote! { ::recursa_core::Visit::visit(#b, visitor)?; }
                }).collect();
                quote! {
                    Self::#vname(#(#bindings),*) => { #(#visits)* }
                }
            }
            Fields::Named(fields) => {
                let names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
                let visits: Vec<_> = names.iter().map(|n| {
                    quote! { ::recursa_core::Visit::visit(#n, visitor)?; }
                }).collect();
                quote! {
                    Self::#vname { #(#names),* } => { #(#visits)* }
                }
            }
            Fields::Unit => {
                quote! { Self::#vname => {} }
            }
        }
    }).collect();

    Ok(quote! {
        match self {
            #(#match_arms)*
        }
    })
}
```

Update `recursa-derive/src/lib.rs`:

```rust
mod visit_derive;

#[proc_macro_derive(Visit)]
pub fn derive_visit(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match visit_derive::derive_visit(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p recursa-derive -- visit`
Expected: PASS

**Step 5: Commit**

```bash
git add recursa-derive/src/visit_derive.rs recursa-derive/src/lib.rs recursa-derive/tests/visit.rs
git commit -m "Add derive(Visit) macro for structs and enums"
```

---

## Task 5: Update Bulk Macros and literals! to Owned Types

Update all three bulk macros to derive `Visit`. Change `literals!` to generate owned `String` types. Update `derive(Scan)` for tuple structs to support `String` fields (no lifetime).

**Files:**
- Modify: `recursa-core/src/macros.rs`
- Modify: `recursa-derive/src/scan_derive.rs`
- Modify: `recursa-derive/tests/scan_tuple_struct.rs`
- Modify: `tests/bulk_macros.rs`

**Step 1: Update `literals!` macro**

Change from `pub struct $name<'input>(pub &'input str)` to `pub struct $name(pub String)`. Add `Visit` derive to all three macros.

```rust
#[macro_export]
macro_rules! keywords {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[scan(pattern = $pattern)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Keyword {
            $($name($name)),*
        }
    };
}

// Same for punctuation!

#[macro_export]
macro_rules! literals {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[scan(pattern = $pattern)]
            pub struct $name(pub String);
        )*

        #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Literal {
            $($name($name)),*
        }
    };
}
```

**Step 2: Update `derive(Scan)` for owned tuple structs**

In `recursa-derive/src/scan_derive.rs`, the `derive_scan_tuple_struct` function currently requires a lifetime parameter. It needs to also support tuple structs with `String` (no lifetime):

- If the struct has a lifetime parameter: existing behaviour (`&'input str` capture)
- If the struct has no lifetime parameter: assume `String`, `from_match` calls `matched.to_string()`

**Step 3: Update tests**

`tests/bulk_macros.rs` tests for `literals!` need to expect `String` instead of `&str`:

```rust
let lit = IntLit::parse(&mut input, &NoRules).unwrap();
assert_eq!(lit.0, "42"); // String implements PartialEq<&str>
```

`recursa-derive/tests/scan_tuple_struct.rs` should add a test for owned `String` tuple struct.

**Step 4: Update all other test files that use literal types**

This is a cascading change. Any test that uses types like `Ident<'input>(&'input str)` or `IntLit<'input>(&'input str)` declared outside of `literals!` is NOT affected — only `literals!`-generated types change. Test files that define their own Scan tuple structs with lifetimes continue to work.

However, `tests/mini_language.rs`, `tests/container_types.rs`, and other tests may use `literals!`. Check and update as needed.

**Step 5: Build and test**

Run: `cargo test --workspace`
Expected: All tests pass.

**Step 6: Commit**

```bash
git add recursa-core/src/macros.rs recursa-derive/src/scan_derive.rs recursa-derive/tests/ tests/
git commit -m "Update bulk macros to derive Visit; literals! generates owned String types"
```

---

## Task 6: Re-exports, Integration Test, Cleanup

Wire up re-exports and write an integration test using the visitor pattern.

**Files:**
- Modify: `src/lib.rs` (facade re-exports)
- Create: `tests/visitor.rs`

**Step 1: Ensure re-exports**

`pub use recursa_core::*` should pick up `Visit`, `Visitor`, `Break`, `NodeKey`, `AsNodeKey`. `pub use recursa_derive::*` should pick up `derive(Visit)`. Verify.

**Step 2: Write integration test**

Create `tests/visitor.rs`:

```rust
use std::ops::ControlFlow;
use recursa::{AsNodeKey, Break, Visit, Visitor, Parse, ParseRules, Input, Scan};

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = "let")]
struct LetKw;

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = "=")]
struct EqSign;

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = ";")]
struct Semi;

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
struct Ident(String);

#[derive(Scan, Visit, Debug, Clone)]
#[scan(pattern = r"[0-9]+")]
struct IntLit(String);

struct Lang;
impl ParseRules for Lang {
    const IGNORE: &'static str = r"\s+";
    fn ignore_cache() -> &'static std::sync::OnceLock<regex::Regex> {
        static CACHE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        &CACHE
    }
}

#[derive(Parse, Visit, Debug)]
#[parse(rules = Lang)]
struct LetStmt {
    let_kw: LetKw,
    name: Ident,
    eq: EqSign,
    value: IntLit,
    semi: Semi,
}

struct IdentCollector { idents: Vec<String> }
impl Visitor for IdentCollector {
    type Error = ();
    fn enter<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<()>> {
        if let Some(ident) = node.downcast_ref::<Ident>() {
            self.idents.push(ident.0.clone());
        }
        ControlFlow::Continue(())
    }
}

#[test]
fn visitor_collects_idents() {
    let mut input = Input::new("let x = 42;");
    let stmt = LetStmt::parse(&mut input, &Lang).unwrap();
    let mut collector = IdentCollector { idents: vec![] };
    stmt.visit(&mut collector);
    assert_eq!(collector.idents, vec!["x"]);
}
```

**Step 3: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass, zero warnings.

**Step 4: Commit**

```bash
git add src/lib.rs tests/visitor.rs
git commit -m "Add visitor integration test and verify re-exports"
```

---

## Summary

| Task | What it delivers |
|------|-----------------|
| 1 | `Break<E>`, `NodeKey`, `AsNodeKey` |
| 2 | `Visit` and `Visitor` traits |
| 3 | `Visit` impls for `Box`, `Option`, `Seq` |
| 4 | `#[derive(Visit)]` proc macro |
| 5 | Bulk macros derive `Visit`; `literals!` generates owned types |
| 6 | Re-exports and integration test |
