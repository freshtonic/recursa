//! Grammar-independent traversal contracts for generated semantic ASTs.

use std::ops::ControlFlow;

/// Early-exit control used by read-only and mutable traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisitBreak<E> {
    /// Do not visit this node's children, but continue with its exit hook.
    SkipChildren,
    /// Stop traversal successfully.
    Finished,
    /// Stop traversal with a caller-defined error.
    Error(E),
}

/// Read-only structural traversal implemented by generated AST types.
pub trait Visit: Sized {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>;
}

/// Mutable structural traversal implemented by generated AST types.
///
/// Generated implementations operate only on detached semantic values. A
/// [`crate::Parsed`] wrapper deliberately does not implement this trait.
pub trait VisitMut: Sized {
    fn visit_mut<V: TotalVisitorMut>(
        &mut self,
        visitor: &mut V,
    ) -> ControlFlow<VisitBreak<V::Error>>;
}

/// Type-specific read-only visitor hooks.
pub trait Visitor<N: ?Sized> {
    type Error;

    fn enter(&mut self, _node: &N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }

    fn exit(&mut self, _node: &N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }
}

/// Type-specific mutable visitor hooks.
pub trait VisitorMut<N: ?Sized> {
    type Error;

    fn enter_mut(&mut self, _node: &mut N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }

    fn exit_mut(&mut self, _node: &mut N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }
}

/// Universal read-only visitor dispatch used by generated walks.
pub trait TotalVisitor {
    type Error;

    fn total_enter<N>(&mut self, node: &N) -> ControlFlow<VisitBreak<Self::Error>>;

    fn total_exit<N>(&mut self, node: &N) -> ControlFlow<VisitBreak<Self::Error>>;
}

/// Universal mutable visitor dispatch used by generated walks.
pub trait TotalVisitorMut {
    type Error;

    fn total_enter_mut<N>(&mut self, node: &mut N) -> ControlFlow<VisitBreak<Self::Error>>;

    fn total_exit_mut<N>(&mut self, node: &mut N) -> ControlFlow<VisitBreak<Self::Error>>;
}

impl<T: Visit> Visit for Box<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>> {
        (**self).visit(visitor)
    }
}

impl<T: VisitMut> VisitMut for Box<T> {
    fn visit_mut<V: TotalVisitorMut>(
        &mut self,
        visitor: &mut V,
    ) -> ControlFlow<VisitBreak<V::Error>> {
        (**self).visit_mut(visitor)
    }
}

impl<T: Visit> Visit for Option<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>> {
        if let Some(value) = self {
            value.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: VisitMut> VisitMut for Option<T> {
    fn visit_mut<V: TotalVisitorMut>(
        &mut self,
        visitor: &mut V,
    ) -> ControlFlow<VisitBreak<V::Error>> {
        if let Some(value) = self {
            value.visit_mut(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: Visit> Visit for Vec<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>> {
        for value in self {
            value.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: VisitMut> VisitMut for Vec<T> {
    fn visit_mut<V: TotalVisitorMut>(
        &mut self,
        visitor: &mut V,
    ) -> ControlFlow<VisitBreak<V::Error>> {
        for value in self {
            value.visit_mut(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: Visit> Visit for crate::Vec1<T> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>> {
        for value in self.iter() {
            value.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: VisitMut> VisitMut for crate::Vec1<T> {
    fn visit_mut<V: TotalVisitorMut>(
        &mut self,
        visitor: &mut V,
    ) -> ControlFlow<VisitBreak<V::Error>> {
        for value in self.iter_mut() {
            value.visit_mut(visitor)?;
        }
        ControlFlow::Continue(())
    }
}
