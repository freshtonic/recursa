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
pub trait AsNodeKey: 'static {
    fn as_node_key(&self) -> NodeKey<'_>
    where
        Self: Sized,
    {
        NodeKey::new(self)
    }
}

/// Marks AST types as traversable via the visitor pattern.
///
/// The `visit` method drives traversal by calling `visitor.enter(self)`,
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
