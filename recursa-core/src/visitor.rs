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
    type Events: VisitEventFamily;

    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>;
}

/// Mutable structural traversal implemented by generated AST types.
///
/// Generated implementations operate only on detached semantic values. A
/// [`crate::Parsed`] wrapper deliberately does not implement this trait.
pub trait VisitMut: Sized {
    type EventsMut: VisitEventFamilyMut;

    fn visit_mut<V>(&mut self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitorMut<Self::EventsMut>;
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

/// Grammar-local family of lifetime-preserving read-only traversal events.
#[doc(hidden)]
pub trait VisitEventFamily {
    type Event<'visit>
    where
        Self: 'visit;
}

/// Grammar-local family of lifetime-preserving mutable traversal events.
#[doc(hidden)]
pub trait VisitEventFamilyMut {
    type EventMut<'visit>
    where
        Self: 'visit;
}

/// Universal read-only visitor dispatch used by generated walks.
pub trait TotalVisitor<F: VisitEventFamily> {
    type Error;

    fn total_enter<'visit>(
        &mut self,
        event: F::Event<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;

    fn total_exit<'visit>(
        &mut self,
        event: F::Event<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;
}

/// Universal mutable visitor dispatch used by generated walks.
pub trait TotalVisitorMut<F: VisitEventFamilyMut> {
    type Error;

    fn total_enter_mut<'visit>(
        &mut self,
        event: F::EventMut<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;

    fn total_exit_mut<'visit>(
        &mut self,
        event: F::EventMut<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;
}

/// Type-erased marker carried for traversal-only implementation nodes.
#[doc(hidden)]
pub trait VisitNode {}

impl<T: ?Sized> VisitNode for T {}

impl<T: Visit> Visit for Box<T> {
    type Events = T::Events;

    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>,
    {
        (**self).visit(visitor)
    }
}

impl<T: VisitMut> VisitMut for Box<T> {
    type EventsMut = T::EventsMut;

    fn visit_mut<V>(&mut self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitorMut<Self::EventsMut>,
    {
        (**self).visit_mut(visitor)
    }
}

impl<T: Visit> Visit for Option<T> {
    type Events = T::Events;

    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>,
    {
        if let Some(value) = self {
            value.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: VisitMut> VisitMut for Option<T> {
    type EventsMut = T::EventsMut;

    fn visit_mut<V>(&mut self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitorMut<Self::EventsMut>,
    {
        if let Some(value) = self {
            value.visit_mut(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: Visit> Visit for Vec<T> {
    type Events = T::Events;

    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>,
    {
        for value in self {
            value.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: VisitMut> VisitMut for Vec<T> {
    type EventsMut = T::EventsMut;

    fn visit_mut<V>(&mut self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitorMut<Self::EventsMut>,
    {
        for value in self {
            value.visit_mut(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: Visit> Visit for crate::Vec1<T> {
    type Events = T::Events;

    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>,
    {
        for value in self.iter() {
            value.visit(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

impl<T: VisitMut> VisitMut for crate::Vec1<T> {
    type EventsMut = T::EventsMut;

    fn visit_mut<V>(&mut self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitorMut<Self::EventsMut>,
    {
        for value in self.iter_mut() {
            value.visit_mut(visitor)?;
        }
        ControlFlow::Continue(())
    }
}
