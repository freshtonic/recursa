// Visitor pattern types for AST traversal.

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
    /// Create a `NodeKey` from a reference to a node.
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

/// Trait for types that can produce a `NodeKey`.
pub trait AsNodeKey {
    fn as_node_key(&self) -> NodeKey<'_>
    where
        Self: Sized + 'static,
    {
        NodeKey::new(self)
    }
}

/// Marks AST types as traversable via the visitor pattern.
///
/// The `visit` method drives traversal by calling `visitor.total_enter(self)`,
/// visiting children, then `visitor.total_exit(self)`.
pub trait Visit: Sized + AsNodeKey {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>>;

    fn downcast_ref<Target: Visit + 'static>(&self) -> Option<&Target>
    where
        Self: 'static,
    {
        (self as &dyn Any).downcast_ref::<Target>()
    }

    fn is<Target: Visit + 'static>(&self) -> bool
    where
        Self: 'static,
    {
        (self as &dyn Any).is::<Target>()
    }
}

/// Type-safe visitor hooks for a specific node type.
///
/// Implement this for each AST type you want to handle in your visitor.
/// The `#[derive(TotalVisitor)]` macro generates the dispatch from
/// `TotalVisitor` to your `Visitor<N>` impls.
pub trait Visitor<N>: Sized {
    type Error;

    fn enter(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
        ControlFlow::Continue(())
    }

    fn exit(&mut self, _node: &N) -> ControlFlow<Break<Self::Error>> {
        ControlFlow::Continue(())
    }
}

/// Universal visitor dispatch trait.
///
/// Called by `Visit::visit` for every AST node. Implementations dispatch
/// to type-specific `Visitor<N>` impls based on `TypeId`.
///
/// Use `#[derive(TotalVisitor)]` to generate this automatically:
///
/// ```text
/// #[derive(TotalVisitor)]
/// #[total_visitor(dispatch = [Statement, Expr], error = MyError)]
/// struct MyVisitor { ... }
/// ```
pub trait TotalVisitor: Sized {
    type Error;

    fn total_enter<N>(&mut self, node: &N) -> ControlFlow<Break<Self::Error>>;

    fn total_exit<N>(&mut self, node: &N) -> ControlFlow<Break<Self::Error>>;
}

// -- Blanket Visit impls for container types --

impl<T: Visit> AsNodeKey for Box<T> {}
impl<T: Visit> Visit for Box<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        (**self).visit(visitor)
    }
}

impl<T: Visit> AsNodeKey for Option<T> {}
impl<T: Visit> Visit for Option<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        if let Some(inner) = self {
            inner.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: Visit> AsNodeKey for Vec<T> {}
impl<T: Visit> Visit for Vec<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        for item in self {
            item.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

// -- Unit type is transparent for Visit (no-op separator in Seq<T, ()>) --

impl AsNodeKey for () {}
impl Visit for () {
    fn visit<V: TotalVisitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
}

// -- PhantomData is transparent for Visit (nothing to visit) --

impl<T: 'static> AsNodeKey for std::marker::PhantomData<T> {}
impl<T: 'static> Visit for std::marker::PhantomData<T> {
    fn visit<V: TotalVisitor>(&self, _visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        ControlFlow::Continue(())
    }
}

// -- Leaf Visit impls for string leaves --
// No-op traversal: string leaves don't participate in visitor dispatch.
// `Cow<'a, str>` is the canonical leaf for borrowed AST text; `String`
// is kept for legacy / 'static AST fixtures.
impl<'a> AsNodeKey for ::std::borrow::Cow<'a, str> {}
impl<'a> Visit for ::std::borrow::Cow<'a, str> {
    fn visit<V: TotalVisitor>(&self, _visitor: &mut V) -> ::std::ops::ControlFlow<Break<V::Error>> {
        ::std::ops::ControlFlow::Continue(())
    }
}

impl AsNodeKey for String {}
impl Visit for String {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        match visitor.total_enter(self) {
            ControlFlow::Continue(()) | ControlFlow::Break(Break::SkipChildren) => {}
            other => return other,
        }
        visitor.total_exit(self)
    }
}
