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
    /// Grammar-local event family covering every reachable Node type.
    type Events: VisitEventFamily;

    /// Traverses this value using generated source-order child dispatch.
    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>;
}

/// Mutable structural traversal implemented by generated AST types.
///
/// Generated implementations operate only on detached semantic values. A
/// [`crate::Parsed`] wrapper deliberately does not implement this trait.
pub trait VisitMut: Sized {
    /// Mutable grammar-local event family covering reachable Node types.
    type EventsMut: VisitEventFamilyMut;

    /// Mutably traverses this detached semantic value in source order.
    fn visit_mut<V>(&mut self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitorMut<Self::EventsMut>;
}

/// Type-specific read-only visitor hooks.
pub trait Visitor<N: ?Sized> {
    /// Caller-defined error returned through [`VisitBreak::Error`].
    type Error;

    /// Runs before generated traversal visits `node` children.
    fn enter(&mut self, _node: &N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }

    /// Runs after generated traversal visits `node` children.
    fn exit(&mut self, _node: &N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }
}

/// Type-specific mutable visitor hooks.
pub trait VisitorMut<N: ?Sized> {
    /// Caller-defined error returned through [`VisitBreak::Error`].
    type Error;

    /// Runs before generated mutable traversal visits `node` children.
    fn enter_mut(&mut self, _node: &mut N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }

    /// Runs after generated mutable traversal visits `node` children.
    fn exit_mut(&mut self, _node: &mut N) -> ControlFlow<VisitBreak<Self::Error>> {
        ControlFlow::Continue(())
    }
}

/// Grammar-local family of lifetime-preserving read-only traversal events.
#[doc(hidden)]
pub trait VisitEventFamily {
    /// Read-only event borrowing one Node for the duration of a callback.
    type Event<'visit>
    where
        Self: 'visit;
}

/// Grammar-local family of lifetime-preserving mutable traversal events.
#[doc(hidden)]
pub trait VisitEventFamilyMut {
    /// Mutable event borrowing one Node for the duration of a callback.
    type EventMut<'visit>
    where
        Self: 'visit;
}

/// Universal read-only visitor dispatch used by generated walks.
pub trait TotalVisitor<F: VisitEventFamily> {
    /// Error shared by every type-specific visitor implementation.
    type Error;

    /// Dispatches the enter hook for one grammar-local event.
    fn total_enter<'visit>(
        &mut self,
        event: F::Event<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;

    /// Dispatches the exit hook for one grammar-local event.
    fn total_exit<'visit>(
        &mut self,
        event: F::Event<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;
}

/// Universal mutable visitor dispatch used by generated walks.
pub trait TotalVisitorMut<F: VisitEventFamilyMut> {
    /// Error shared by every mutable type-specific visitor implementation.
    type Error;

    /// Dispatches the mutable enter hook for one grammar-local event.
    fn total_enter_mut<'visit>(
        &mut self,
        event: F::EventMut<'visit>,
    ) -> ControlFlow<VisitBreak<Self::Error>>
    where
        F: 'visit;

    /// Dispatches the mutable exit hook for one grammar-local event.
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

impl<T: Visit> Visit for bumpalo::boxed::Box<'_, T> {
    type Events = T::Events;

    fn visit<V>(&self, visitor: &mut V) -> ControlFlow<VisitBreak<V::Error>>
    where
        V: TotalVisitor<Self::Events>,
    {
        (**self).visit(visitor)
    }
}

impl<T: VisitMut> VisitMut for bumpalo::boxed::Box<'_, T> {
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

impl<T: Visit> Visit for bumpalo::collections::Vec<'_, T> {
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

impl<T: VisitMut> VisitMut for bumpalo::collections::Vec<'_, T> {
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

impl<T: Visit> Visit for crate::ArenaVec1<'_, T> {
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

impl<T: VisitMut> VisitMut for crate::ArenaVec1<'_, T> {
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

/// Runs one generated read-only traversal step: enter hook, children, exit hook.
///
/// Generated `Visit` implementations expand this in place of the enter/exit
/// plumbing they used to carry, so each Node keeps one event expression.
#[doc(hidden)]
#[macro_export]
macro_rules! __recursa_visit_node {
    ($visitor:ident, $event:expr, { $($children:tt)* }) => {{
        match $visitor.total_enter($event) {
            ::core::ops::ControlFlow::Continue(()) => { $($children)* }
            ::core::ops::ControlFlow::Break($crate::VisitBreak::SkipChildren) => {}
            other => return other,
        }
        $visitor.total_exit($event)
    }};
}

/// Runs one generated mutable traversal step: enter hook, children, exit hook.
#[doc(hidden)]
#[macro_export]
macro_rules! __recursa_visit_node_mut {
    ($visitor:ident, $event:expr, { $($children:tt)* }) => {{
        match $visitor.total_enter_mut($event) {
            ::core::ops::ControlFlow::Continue(()) => { $($children)* }
            ::core::ops::ControlFlow::Break($crate::VisitBreak::SkipChildren) => {}
            other => return other,
        }
        $visitor.total_exit_mut($event)
    }};
}

/// Visits a compact list of generated children in authored source order.
///
/// `ref`/`mut` items are fields of `$self`; `bind_ref`/`bind_mut` items are
/// pattern bindings that already borrow the child. A bracketed item
/// (`[field]`) traverses through the grammar-local helper trait given after
/// the receiver instead of the public `Visit`/`VisitMut` trait.
#[doc(hidden)]
#[macro_export]
macro_rules! __recursa_visit_children {
    (ref $self:ident $visitor:ident ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@ref $self $visitor () ; $($items)*);
    };
    (ref $self:ident $visitor:ident $helper:path ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@ref $self $visitor ($helper) ; $($items)*);
    };
    (mut $self:ident $visitor:ident ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@mut $self $visitor () ; $($items)*);
    };
    (mut $self:ident $visitor:ident $helper:path ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@mut $self $visitor ($helper) ; $($items)*);
    };
    (bind_ref $visitor:ident ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@bind_ref $visitor () ; $($items)*);
    };
    (bind_ref $visitor:ident $helper:path ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@bind_ref $visitor ($helper) ; $($items)*);
    };
    (bind_mut $visitor:ident ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@bind_mut $visitor () ; $($items)*);
    };
    (bind_mut $visitor:ident $helper:path ; $($items:tt)*) => {
        $crate::__recursa_visit_children!(@bind_mut $visitor ($helper) ; $($items)*);
    };

    (@ref $self:ident $visitor:ident ($($helper:tt)*) ;) => {};
    (@ref $self:ident $visitor:ident ($($helper:tt)*) ; [$field:tt] $(, $($rest:tt)*)?) => {
        <_ as $($helper)*>::__recursa_visit_with(&$self.$field, $visitor)?;
        $crate::__recursa_visit_children!(@ref $self $visitor ($($helper)*) ; $($($rest)*)?);
    };
    (@ref $self:ident $visitor:ident ($($helper:tt)*) ; $field:tt $(, $($rest:tt)*)?) => {
        $crate::Visit::visit(&$self.$field, $visitor)?;
        $crate::__recursa_visit_children!(@ref $self $visitor ($($helper)*) ; $($($rest)*)?);
    };

    (@mut $self:ident $visitor:ident ($($helper:tt)*) ;) => {};
    (@mut $self:ident $visitor:ident ($($helper:tt)*) ; [$field:tt] $(, $($rest:tt)*)?) => {
        <_ as $($helper)*>::__recursa_visit_mut_with(&mut $self.$field, $visitor)?;
        $crate::__recursa_visit_children!(@mut $self $visitor ($($helper)*) ; $($($rest)*)?);
    };
    (@mut $self:ident $visitor:ident ($($helper:tt)*) ; $field:tt $(, $($rest:tt)*)?) => {
        $crate::VisitMut::visit_mut(&mut $self.$field, $visitor)?;
        $crate::__recursa_visit_children!(@mut $self $visitor ($($helper)*) ; $($($rest)*)?);
    };

    (@bind_ref $visitor:ident ($($helper:tt)*) ;) => {};
    (@bind_ref $visitor:ident ($($helper:tt)*) ; [$binding:ident] $(, $($rest:tt)*)?) => {
        <_ as $($helper)*>::__recursa_visit_with($binding, $visitor)?;
        $crate::__recursa_visit_children!(@bind_ref $visitor ($($helper)*) ; $($($rest)*)?);
    };
    (@bind_ref $visitor:ident ($($helper:tt)*) ; $binding:ident $(, $($rest:tt)*)?) => {
        $crate::Visit::visit($binding, $visitor)?;
        $crate::__recursa_visit_children!(@bind_ref $visitor ($($helper)*) ; $($($rest)*)?);
    };

    (@bind_mut $visitor:ident ($($helper:tt)*) ;) => {};
    (@bind_mut $visitor:ident ($($helper:tt)*) ; [$binding:ident] $(, $($rest:tt)*)?) => {
        <_ as $($helper)*>::__recursa_visit_mut_with($binding, $visitor)?;
        $crate::__recursa_visit_children!(@bind_mut $visitor ($($helper)*) ; $($($rest)*)?);
    };
    (@bind_mut $visitor:ident ($($helper:tt)*) ; $binding:ident $(, $($rest:tt)*)?) => {
        $crate::VisitMut::visit_mut($binding, $visitor)?;
        $crate::__recursa_visit_children!(@bind_mut $visitor ($($helper)*) ; $($($rest)*)?);
    };
}
