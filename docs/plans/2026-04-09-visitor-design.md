# Visitor Pattern Design

Adds AST traversal via the visitor pattern, ported from the `sqltk` crate.

## Core Types

### Break\<E\>

Control flow enum for visitor traversal:

```rust
pub enum Break<E> {
    SkipChildren,  // don't visit child nodes
    Finished,      // traversal complete, stop early
    Err(E),        // error, propagate up
}
```

### NodeKey

Type-erased handle to an AST node, usable as a `HashMap` key:

```rust
#[derive(Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Clone, Copy)]
pub struct NodeKey<'ast> {
    node_addr: usize,
    node_type: TypeId,
    _ast: PhantomData<&'ast ()>,
}
```

Constructed via `NodeKey::new(node)` which captures the memory address and `TypeId`. `get_as::<N>()` retrieves the original node with type verification. Requires `N: 'static`.

### AsNodeKey

Trait for types that can produce a `NodeKey`:

```rust
pub trait AsNodeKey: 'static {
    fn as_node_key(&self) -> NodeKey<'_>;
}
```

## Traits

### Visitor

Defines hooks called during AST traversal:

```rust
pub trait Visitor: Sized {
    type Error;

    fn enter<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<Self::Error>> {
        ControlFlow::Continue(())
    }

    fn exit<N: Visit>(&mut self, node: &N) -> ControlFlow<Break<Self::Error>> {
        ControlFlow::Continue(())
    }
}
```

Both methods have default no-op implementations. A visitor overrides what it cares about, using `downcast_ref` inside the body to check the concrete node type.

### Visit

Marks AST types as traversable (named `Visitable` in sqltk):

```rust
pub trait Visit: 'static + Sized + AsNodeKey {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>>;

    fn downcast_ref<Target: Visit>(&self) -> Option<&Target> {
        (self as &dyn Any).downcast_ref::<Target>()
    }

    fn is<Target: Visit>(&self) -> bool {
        (self as &dyn Any).is::<Target>()
    }
}
```

Requires `'static` — AST types must use owned data (e.g., `String` not `&'input str`) to be visitable. This is the tradeoff for enabling `TypeId`-based downcasting.

## Derived Visit Implementations

### Structs

Visit each field in order. `SkipChildren` from `enter` skips field traversal.

```rust
impl Visit for LetBinding {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) => {
                self.let_kw.visit(visitor)?;
                self.name.visit(visitor)?;
                self.eq.visit(visitor)?;
                self.value.visit(visitor)?;
                self.semi.visit(visitor)?;
            }
            ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}
```

### Enums

Delegate to whichever variant was parsed:

```rust
impl Visit for Statement {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) => {
                match self {
                    Self::Let(inner) => inner.visit(visitor)?,
                    Self::Return(inner) => inner.visit(visitor)?,
                };
            }
            ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}
```

### Scan Types (Leaf Tokens)

No children to visit:

```rust
impl Visit for LetKw {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.enter(self) {
            ControlFlow::Continue(()) => {}
            ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.exit(self)
    }
}
```

## Container Type Impls

Container types are transparent — no `enter`/`exit` on the container itself, visitors see the inner elements directly.

### Box\<T: Visit\>

```rust
impl<T: Visit> Visit for Box<T> {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        (**self).visit(visitor)
    }
}
```

### Option\<T: Visit\>

```rust
impl<T: Visit> Visit for Option<T> {
    fn visit<V: Visitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        if let Some(inner) = self {
            inner.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}
```

### Seq\<T, S, ...\>

```rust
impl<T: Visit, S: Visit, Trailing, Empty> Visit for Seq<T, S, Trailing, Empty> {
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

## Bulk Macros

All three macros (`keywords!`, `punctuation!`, `literals!`) derive `Visit` alongside `Scan`.

### Breaking Change: `literals!` Generates Owned Types

`literals!` now generates `struct Ident(pub String)` instead of `struct Ident<'input>(pub &'input str)`. This satisfies the `'static` requirement for `Visit` at the cost of allocating a `String` per captured token during parsing.

The `derive(Scan)` macro must support both:
- Tuple structs with `String` (owned, no lifetime) — `from_match` calls `matched.to_string()`
- Tuple structs with `&'input str` (borrowed, with lifetime) — existing behaviour

The derive macro distinguishes by checking whether the struct has a lifetime parameter.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Visit separate from Parse | Yes | Single responsibility; not all types need both |
| `'static` requirement | Yes | Enables `TypeId`-based downcasting via `Any` |
| Owned AST types | Yes | Required by `'static`; small allocation cost per token |
| Generic enter/exit with downcast | Yes | Simpler than per-type methods; proven in sqltk |
| Container transparency | Yes | Box/Option/Seq don't fire enter/exit; visitors see elements directly |
| Named `Visit` not `Visitable` | Yes | Shorter, cleaner |

## Deferred

- `Transform` / `Transformable` — AST transformation with ancestor context
- `NodePath` — type-safe ancestor chain tracking
