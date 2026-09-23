use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ptr::NonNull;

use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;

use crate::input::{Input, RetainedDocument, SourceGaps, TokenRecord, TriviaRecord};
use crate::{FollowSet, GrammarInfo, HasSpan, ParseError, Span};

#[doc(hidden)]
/// Seal implemented only by generated grammar-local input types.
pub mod parse_seal {
    /// Prevents external implementations of the safe [`super::Parse`] facade.
    pub trait Sealed<I: ?Sized> {}
}

/// Safe parsing capability implemented only by matching Recursa generations.
///
/// `I` is a generated grammar-local input type. Recursive parsing and FOLLOW
/// transport are deliberately absent from this public contract.
pub trait Parse<I: ?Sized>: parse_seal::Sealed<I> {
    /// Parsed wrapper or value returned by the generated root entry point.
    type Output;

    /// Parses one root from grammar-bound input transactionally.
    fn parse(input: &mut I) -> Result<Self::Output, ParseError>;
}

/// Safe arena-backed parsing capability implemented by matching Recursa generations.
pub trait ArenaParse<I: ?Sized>: arena_parse_seal::Sealed<I> {
    /// Lifetime-parametric family owned by the returned parse wrapper.
    type Family: ArenaAstFamily;

    /// Self-contained parsed wrapper returned for this generated input lifetime.
    type Output;

    /// Parses one root transactionally into a self-contained arena owner.
    fn parse_arena(input: &mut I) -> Result<Self::Output, ParseError>;
}

/// Sealed implementation detail for [`ArenaParse`].
#[doc(hidden)]
pub mod arena_parse_seal {
    /// Prevents external implementations of the safe arena parsing facade.
    pub trait Sealed<I: ?Sized> {}
}

/// A parsed semantic value together with its immutable occurrence provenance.
///
/// Mutating access is deliberately unavailable: consume the wrapper with
/// [`Parsed::into_ast`] before changing the semantic value.
pub struct Parsed<'input, T> {
    /// Authored semantic value.
    ast: T,
    /// Parsed, uncaptured, or synthetic occurrence provenance.
    origin: Origin<'input>,
}

/// Type constructor for one generated arena-backed AST application.
///
/// # Safety
///
/// Implementations are generated alongside [`ArenaGeneratedParse`] and must use one
/// lifetime-parametric AST type whose layout does not depend on the chosen lifetime.
#[doc(hidden)]
pub unsafe trait ArenaAstFamily {
    /// The authored AST application borrowing one parse-owned arena.
    type Ast<'arena>;
}

/// A parsed arena-backed AST whose allocator lifetime is confined to this wrapper.
///
/// Callers borrow the authored tree with [`ArenaParsed::ast`]. Moving the AST out is deliberately
/// unsupported because its boxes and vectors borrow the arena owned by this value.
pub struct ArenaParsed<'input, F: ArenaAstFamily> {
    /// Parse-owned arena, erased semantic root, and occurrence sidecar.
    cell: ArenaParseCell<'input>,
    /// Complete input source while a public capture or retained document needs it.
    source: Option<&'input str>,
    /// Absolute source window of this parse.
    bounds: Option<Span>,
    /// Opted-in lexical records used for document gap views.
    document: Option<RetainedDocument>,
    /// Selects the generated AST family without storing a value of it outside the cell.
    family: PhantomData<F>,
}

impl<F: ArenaAstFamily> ArenaParsed<'_, F> {
    /// Borrows the authored AST for at most as long as this wrapper is borrowed.
    pub fn ast(&self) -> &F::Ast<'_> {
        // SAFETY: `parse_arena_with_request_and_follow` constructs the erased pointer from exactly
        // `F::Ast<'arena>`. The returned lifetime is shortened to the borrow of this cell, so no
        // arena-backed value can outlive its owner.
        unsafe { self.cell.borrow_dependent().ast.cast::<F>() }
    }

    /// Projects the captured provenance retained beside the arena-backed root.
    fn view_cursor(&self) -> ViewCursor<'_> {
        ViewCursor {
            occurrence: self.cell.borrow_dependent().root,
            source: self.source,
            bounds: self.bounds,
            records: self
                .document
                .as_ref()
                .map(|document| document.records.as_ref()),
            trivia: self
                .document
                .as_ref()
                .map(|document| document.trivia.as_ref()),
        }
    }

    /// Returns the complete source when this parse retained provenance.
    pub fn source(&self) -> Option<&str> {
        self.source?.get(self.bounds?.range())
    }

    /// Returns the absolute source-coordinate window retained by this parse.
    pub fn source_bounds(&self) -> Option<Span> {
        self.source?;
        self.bounds
    }

    /// Translates an absolute captured span into the bounded source slice.
    pub fn local_source_range(&self, span: Span) -> Option<std::ops::Range<usize>> {
        let source = self.source?;
        let bounds = self.bounds?;
        if span.start() < bounds.start() || span.end() > bounds.end() {
            return None;
        }
        source.get(span.range())?;
        Some(
            usize::try_from(span.start() - bounds.start()).ok()?
                ..usize::try_from(span.end() - bounds.start()).ok()?,
        )
    }
}

#[cfg(feature = "explorer")]
impl<F: ArenaAstFamily> ArenaParsed<'_, F> {
    /// Exposes the typed root and occurrence cursor to the erased Explorer ABI.
    pub(crate) fn explorer_parts(&self) -> (&F::Ast<'_>, ViewCursor<'_>) {
        (self.ast(), self.view_cursor())
    }
}

#[cfg(feature = "statistics")]
impl<F: ArenaAstFamily> ArenaParsed<'_, F> {
    /// Reports the total bytes this parse's arena has allocated.
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.cell.borrow_owner().arena.allocated_bytes()
    }
}

impl<F> fmt::Debug for ArenaParsed<'_, F>
where
    F: ArenaAstFamily,
    for<'arena> F::Ast<'arena>: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.ast().fmt(formatter)
    }
}

impl<F> crate::Pretty for ArenaParsed<'_, F>
where
    F: ArenaAstFamily,
    for<'arena> F::Ast<'arena>: crate::Pretty,
{
    fn pretty(&self, context: &mut crate::PrettyContext) -> fmt::Result {
        context.with_cursor(self.view_cursor(), |context| self.ast().pretty(context))
    }
}

impl<T: fmt::Debug> fmt::Debug for Parsed<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.ast.fmt(formatter)
    }
}

impl<T: PartialEq> PartialEq for Parsed<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.ast == other.ast
    }
}

impl<T: Eq> Eq for Parsed<'_, T> {}

impl<T: PartialOrd> PartialOrd for Parsed<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.ast.partial_cmp(&other.ast)
    }
}

impl<T: Ord> Ord for Parsed<'_, T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ast.cmp(&other.ast)
    }
}

impl<T: Hash> Hash for Parsed<'_, T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ast.hash(state);
    }
}

impl<T: crate::Visit> crate::Visit for Parsed<'_, T> {
    type Events = T::Events;

    fn visit<V>(&self, visitor: &mut V) -> std::ops::ControlFlow<crate::VisitBreak<V::Error>>
    where
        V: crate::TotalVisitor<Self::Events>,
    {
        self.ast.visit(visitor)
    }
}

impl<T: crate::Pretty> crate::Pretty for Parsed<'_, T> {
    fn pretty(&self, context: &mut crate::PrettyContext) -> fmt::Result {
        context.with_cursor(self.origin.view_cursor(), |context| {
            self.ast.pretty(context)
        })
    }
}

#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for Parsed<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.ast.serialize(serializer)
    }
}

/// Provenance state retained alongside one semantic root.
enum Origin<'input> {
    /// Programmatically constructed without source.
    Synthetic,
    /// Parsed successfully while capture was disabled.
    Uncaptured,
    /// Parsed with source-borrowing occurrence provenance.
    Captured {
        /// Complete source used by occurrence spans.
        source: &'input str,
        /// Exact absolute input window retained by this parse.
        bounds: Span,
        /// Root of the immutable occurrence sidecar.
        provenance: ProvenanceCell,
        /// Opted-in immutable lexical records used for document gap views.
        document: Option<RetainedDocument>,
    },
}

impl Origin<'_> {
    /// Projects one provenance state into the common generated-view cursor.
    fn view_cursor(&self) -> ViewCursor<'_> {
        match self {
            Self::Synthetic | Self::Uncaptured => ViewCursor::absent(),
            Self::Captured {
                source,
                bounds,
                provenance,
                document,
            } => ViewCursor {
                occurrence: provenance.borrow_dependent().root,
                source: Some(*source),
                bounds: Some(*bounds),
                records: document.as_ref().map(|document| document.records.as_ref()),
                trivia: document.as_ref().map(|document| document.trivia.as_ref()),
            },
        }
    }
}

/// Arena-borrowing occurrence provenance for one parsed root.
struct Provenance<'arena> {
    /// Root occurrence whose descendants follow generated child ordinals.
    root: Option<&'arena OccurrenceNode<'arena>>,
}

self_cell::self_cell! {
    /// Move-owned arena and the provenance tree allocated inside it.
    struct ProvenanceCell {
        owner: Bump,

        #[covariant]
        dependent: Provenance,
    }
}

/// Type-erased root stored in the same self cell as its allocator.
struct ErasedArenaAst<'arena> {
    /// Root allocation inside the parse arena.
    pointer: NonNull<()>,
    /// Drops the root and its recursively owned values before the arena releases its chunks.
    drop: unsafe fn(NonNull<()>),
    /// Binds the erased allocation to the arena borrow.
    marker: PhantomData<&'arena ()>,
}

impl<'arena> ErasedArenaAst<'arena> {
    /// Allocates and erases one generated AST root.
    fn new<T>(value: T, arena: &'arena Bump) -> Self {
        let pointer = NonNull::from(arena.alloc(value)).cast();
        unsafe fn drop_value<T>(pointer: NonNull<()>) {
            // SAFETY: `pointer` was produced by `Bump::alloc::<T>` in `new` and is dropped once,
            // before the owning bump arena.
            unsafe { pointer.cast::<T>().as_ptr().drop_in_place() };
        }
        Self {
            pointer,
            drop: drop_value::<T>,
            marker: PhantomData,
        }
    }

    /// Borrows the exact generated family used to construct this erased root.
    unsafe fn cast<F: ArenaAstFamily>(&self) -> &F::Ast<'_> {
        // SAFETY: upheld by the generated `ArenaGeneratedParse::Family` association and the
        // caller's construction path; the borrow cannot outlive this erased owner.
        unsafe { &*self.pointer.cast::<F::Ast<'_>>().as_ptr() }
    }
}

impl Drop for ErasedArenaAst<'_> {
    fn drop(&mut self) {
        // SAFETY: `new` installs the matching monomorphized destructor and the self cell drops its
        // dependent before its arena owner.
        unsafe { (self.drop)(self.pointer) };
    }
}

#[cfg(feature = "arbitrary")]
self_cell::self_cell! {
    /// Move-owned arena and one synthesized (sample-generated, sourceless) root allocated inside
    /// it — the arena-backed counterpart of [`Parsed::synthetic`], with no provenance tree since a
    /// sampled document was never lexed or parsed.
    struct ArenaSyntheticCell {
        owner: Bump,

        #[covariant]
        dependent: ErasedArenaAst,
    }
}

/// A synthesized (Arbitrary-sampled), arena-backed root with no source or provenance.
#[cfg(feature = "arbitrary")]
pub struct ArenaSynthetic<F: ArenaAstFamily> {
    /// Owns the arena and its erased root, dropping the root before releasing arena storage.
    cell: ArenaSyntheticCell,
    /// Retains the generated family identity needed to recover the erased root's concrete type.
    family: PhantomData<F>,
}

#[cfg(feature = "arbitrary")]
impl<F: ArenaAstFamily> ArenaSynthetic<F> {
    /// Allocates a fresh arena and materializes one root inside it via `build`.
    pub fn new(build: impl for<'arena> FnOnce(&'arena Bump) -> F::Ast<'arena>) -> Self {
        let cell = ArenaSyntheticCell::try_new(Bump::new(), |arena| {
            Ok::<_, std::convert::Infallible>(ErasedArenaAst::new(build(arena), arena))
        })
        .unwrap_or_else(|error: std::convert::Infallible| match error {});
        Self {
            cell,
            family: PhantomData,
        }
    }

    /// Borrows the authored AST for at most as long as this wrapper is borrowed.
    pub fn ast(&self) -> &F::Ast<'_> {
        // SAFETY: `new` constructs the erased pointer from exactly `F::Ast<'arena>`, matching
        // `ArenaParsed::ast`'s identical invariant.
        unsafe { self.cell.borrow_dependent().cast::<F>() }
    }
}

#[cfg(all(feature = "arbitrary", feature = "explorer"))]
impl<F: ArenaAstFamily> ArenaSynthetic<F> {
    /// Exposes the typed root and an empty (no source, no provenance) cursor to the Explorer ABI.
    pub(crate) fn explorer_parts(&self) -> (&F::Ast<'_>, ViewCursor<'_>) {
        (
            self.ast(),
            ViewCursor {
                occurrence: None,
                source: None,
                bounds: None,
                records: None,
                trivia: None,
            },
        )
    }
}

/// Arena-borrowing semantic root and occurrence provenance for one successful parse.
struct ArenaParseStorage<'arena> {
    /// Erased authored AST root.
    ast: ErasedArenaAst<'arena>,
    /// Root occurrence whose descendants follow generated child ordinals.
    root: Option<&'arena OccurrenceNode<'arena>>,
}

/// Arena owner retains the input lifetime that every borrowed token text is shortened from.
struct ArenaOwner<'input> {
    /// Allocator shared by semantic AST containers and provenance.
    arena: Bump,
    /// Establishes that source borrowed into the dependent outlives the cell.
    source: &'input str,
}

self_cell::self_cell! {
    /// Move-owned AST/provenance arena and both trees allocated inside it.
    struct ArenaParseCell<'input> {
        owner: ArenaOwner<'input>,

        #[covariant]
        dependent: ArenaParseStorage,
    }
}

/// One private occurrence-sidecar node.
struct OccurrenceNode<'arena> {
    /// Captured extent when requested or required.
    span: Option<Span>,
    /// Semantic and fixed-element children in generated ordinal order.
    children: BumpVec<'arena, Occurrence<'arena>>,
    /// Fixed separators retained independently from semantic children.
    repetition_separators: BumpVec<'arena, RepetitionSeparatorOccurrence>,
}

/// Fixed separator provenance belongs to a repetition occurrence, not to its
/// semantic child sequence. Keeping the two shapes distinct prevents runtime
/// separator count from changing generated child ordinals.
struct RepetitionSeparatorNode {
    /// Exact matched separator range.
    span: Span,
}

/// Authored role of one generated grammar element.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ElementRole {
    /// A fixed keyword or punctuation occurrence with no semantic AST field.
    FixedToken,
    /// A semantic field stored in the authored AST shape.
    SemanticField,
}

/// Static metadata shared by every occurrence of one generated element.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ElementMetadata {
    /// Authored field name or generated fixed-token name.
    pub(crate) name: Option<&'static str>,
    /// Whether the element contributes semantic data.
    pub(crate) role: ElementRole,
}

impl ElementMetadata {
    /// The authored field name or canonical fixed spelling, when one exists.
    pub const fn name(self) -> Option<&'static str> {
        self.name
    }

    /// Whether this element is fixed syntax or a semantic field.
    pub const fn role(self) -> ElementRole {
        self.role
    }
}

/// One source-ordered element occurrence of a concrete Node view.
#[derive(Clone, Copy)]
pub struct ElementView<'view> {
    /// Static descriptor shared by all occurrences.
    metadata: &'static ElementMetadata,
    /// Cursor locating this occurrence in parsed provenance.
    cursor: ViewCursor<'view>,
    /// Source-order ordinal in the active Node shape.
    index: usize,
}

impl ElementView<'_> {
    /// Static metadata for this grammar element.
    pub const fn metadata(self) -> &'static ElementMetadata {
        self.metadata
    }

    /// Zero-based position in the active authored element sequence.
    pub const fn index(self) -> usize {
        self.index
    }

    /// Authored field name or generated fixed-element name.
    pub const fn name(self) -> Option<&'static str> {
        self.metadata.name()
    }

    /// Whether this occurrence represents fixed syntax or a semantic field.
    pub const fn role(self) -> ElementRole {
        self.metadata.role()
    }
}

impl crate::output::span_seal::Sealed for ElementView<'_> {}

impl HasSpan for ElementView<'_> {
    fn span(&self) -> Option<Span> {
        self.cursor.span()
    }
}

/// Exact-size iterator over the active source-ordered elements of a Node view.
pub struct Elements<'view> {
    /// Active shape's static element descriptors.
    metadata: &'static [ElementMetadata],
    /// Parent occurrence cursor.
    cursor: ViewCursor<'view>,
    /// Next source-order element ordinal.
    index: usize,
}

/// A semantic repetition paired with its aggregate occurrence provenance.
pub struct Repeated<'view, T: Node, U> {
    /// Authored semantic sequence.
    values: &'view [U],
    /// Aggregate repetition occurrence cursor.
    cursor: ViewCursor<'view>,
    /// Projection from stored field value to semantic Node.
    project: fn(&U) -> &T,
    /// Child ordinal locating a semantic value inside fixed attachments.
    attached_semantic_child: Option<usize>,
    /// Binds the projected Node lifetime without storing a reference.
    marker: PhantomData<&'view T>,
}

impl<'view, T: Node, U> Repeated<'view, T, U> {
    #[doc(hidden)]
    /// Constructs a repetition view for directly stored Node values.
    pub fn new(values: &'view [U], cursor: ViewCursor<'view>, project: fn(&U) -> &T) -> Self {
        Self {
            values,
            cursor,
            project,
            attached_semantic_child: None,
            marker: PhantomData,
        }
    }

    #[doc(hidden)]
    /// Constructs a repetition view whose values have fixed attachments.
    pub fn new_attached(
        values: &'view [U],
        cursor: ViewCursor<'view>,
        project: fn(&U) -> &T,
        semantic_child: usize,
    ) -> Self {
        Self {
            values,
            cursor,
            project,
            attached_semantic_child: Some(semantic_child),
            marker: PhantomData,
        }
    }

    /// Returns the number of semantic occurrences.
    pub const fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether this repetition has no semantic occurrences.
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns one concrete child view by repetition ordinal.
    pub fn get(&self, index: usize) -> Option<T::View<'view>>
    where
        T: 'view,
    {
        self.values.get(index).map(|value| {
            let cursor = self.cursor.child(index);
            let cursor = self
                .attached_semantic_child
                .map_or(cursor, |child| cursor.child(child));
            (self.project)(value).__view(cursor)
        })
    }

    /// Iterates concrete child views in semantic order.
    pub fn iter(&self) -> RepeatedIter<'view, T, U>
    where
        T: 'view,
    {
        RepeatedIter {
            values: self.values.iter(),
            cursor: self.cursor,
            project: self.project,
            attached_semantic_child: self.attached_semantic_child,
            index: 0,
            marker: PhantomData,
        }
    }
}

impl<T: Node, U> crate::output::span_seal::Sealed for Repeated<'_, T, U> {}

impl<T: Node, U> HasSpan for Repeated<'_, T, U> {
    fn span(&self) -> Option<Span> {
        self.cursor.span()
    }
}

/// Exact-size iterator over the child views of one repetition occurrence.
pub struct RepeatedIter<'view, T: Node, U> {
    /// Stored field values not yet projected.
    values: std::slice::Iter<'view, U>,
    /// Aggregate repetition cursor.
    cursor: ViewCursor<'view>,
    /// Projection from stored field value to semantic Node.
    project: fn(&U) -> &T,
    /// Child ordinal locating a semantic value inside fixed attachments.
    attached_semantic_child: Option<usize>,
    /// Next repetition ordinal.
    index: usize,
    /// Binds the projected Node lifetime without storing a reference.
    marker: PhantomData<&'view T>,
}

impl<'view, T: Node + 'view, U> Iterator for RepeatedIter<'view, T, U> {
    type Item = T::View<'view>;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.values.next()?;
        let index = self.index;
        self.index += 1;
        let cursor = self.cursor.child(index);
        let cursor = self
            .attached_semantic_child
            .map_or(cursor, |child| cursor.child(child));
        Some((self.project)(value).__view(cursor))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl<T: Node, U> ExactSizeIterator for RepeatedIter<'_, T, U> {}
impl<T: Node, U> std::iter::FusedIterator for RepeatedIter<'_, T, U> {}

impl<'view> Elements<'view> {
    /// Constructs the empty iterator used by default Node views.
    fn empty() -> Self {
        Self {
            metadata: &[],
            cursor: ViewCursor::absent(),
            index: 0,
        }
    }

    /// Number of active grammar elements remaining in this iterator.
    pub const fn len(&self) -> usize {
        self.metadata.len().saturating_sub(self.index)
    }

    /// Whether this iterator has no active grammar elements remaining.
    pub const fn is_empty(&self) -> bool {
        self.index >= self.metadata.len()
    }

    /// Returns one active element by authored position.
    pub fn get(&self, index: usize) -> Option<ElementView<'view>> {
        let metadata = self.metadata.get(index)?;
        Some(ElementView {
            metadata,
            cursor: self.cursor.child(index),
            index,
        })
    }
}

impl<'view> Iterator for Elements<'view> {
    type Item = ElementView<'view>;

    fn next(&mut self) -> Option<Self::Item> {
        let metadata = self.metadata.get(self.index)?;
        let cursor = self.cursor.child(self.index);
        self.index += 1;
        Some(ElementView {
            metadata,
            cursor,
            index: self.index - 1,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.metadata.len().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Elements<'_> {}
impl std::iter::FusedIterator for Elements<'_> {}

impl OccurrenceNode<'_> {
    /// Writes this node as `(span: children ; separators)`; see [`Parsed::debug_provenance`].
    fn render(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("(")?;
        match self.span {
            Some(span) => write!(formatter, "{}..{}", span.start(), span.end())?,
            None => formatter.write_str("-")?,
        }
        if !self.children.is_empty() {
            formatter.write_str(":")?;
            for child in &self.children {
                formatter.write_str(" ")?;
                match child.node {
                    Some(node) => node.render(formatter)?,
                    None => formatter.write_str("_")?,
                }
            }
        }
        if !self.repetition_separators.is_empty() {
            formatter.write_str(" ;")?;
            for separator in &self.repetition_separators {
                write!(
                    formatter,
                    " {}..{}",
                    separator.node.span.start(),
                    separator.node.span.end()
                )?;
            }
        }
        formatter.write_str(")")
    }

    /// Reports whether this node or any private descendant retained provenance.
    fn is_captured(&self) -> bool {
        self.span.is_some()
            || self.children.iter().any(Occurrence::is_captured)
            || !self.repetition_separators.is_empty()
    }

    /// Reports whether public source access is justified by a captured span.
    fn has_public_capture(&self) -> bool {
        self.span.is_some()
            || self
                .children
                .iter()
                .filter_map(|child| child.node)
                .any(Self::has_public_capture)
    }
}

impl<'input, T> Parsed<'input, T> {
    /// Wraps a programmatically constructed AST without inventing source data.
    pub fn synthetic(ast: T) -> Self {
        Self {
            ast,
            origin: Origin::Synthetic,
        }
    }

    /// Returns the captured parse-window text, if this parse captured provenance.
    ///
    /// Framing bounds this slice to one island, while occurrence spans retain absolute offsets in
    /// the complete document. Use [`Parsed::local_source_range`] before indexing this slice with an
    /// occurrence span; [`Parsed::source_bounds`] exposes the absolute window and its base offset.
    pub fn source(&self) -> Option<&'input str> {
        let (source, bounds) = self.captured_source_window()?;
        source.get(bounds.range())
    }

    /// Returns the absolute source-coordinate window retained by this parse.
    ///
    /// The window starts at zero for ordinary whole-input parsing and at the island's document
    /// offset for framing. Every captured occurrence span uses these absolute coordinates.
    pub fn source_bounds(&self) -> Option<Span> {
        self.captured_source_window().map(|(_, bounds)| bounds)
    }

    /// Translates one absolute captured span into a range suitable for [`Parsed::source`].
    ///
    /// Returns `None` when source was not captured, the span escapes this parse's window, or either
    /// endpoint is not a UTF-8 boundary. The returned range is local to the bounded source slice.
    pub fn local_source_range(&self, span: Span) -> Option<std::ops::Range<usize>> {
        let (source, bounds) = self.captured_source_window()?;
        if span.start() < bounds.start() || span.end() > bounds.end() {
            return None;
        }
        source.get(span.range())?;
        let start = usize::try_from(span.start() - bounds.start()).ok()?;
        let end = usize::try_from(span.end() - bounds.start()).ok()?;
        Some(start..end)
    }

    /// Returns captured full source and absolute bounds under the public provenance policy.
    fn captured_source_window(&self) -> Option<(&'input str, Span)> {
        match &self.origin {
            Origin::Captured {
                source,
                bounds,
                provenance,
                document,
            } if document.is_some()
                || provenance
                    .borrow_dependent()
                    .root
                    .is_some_and(OccurrenceNode::has_public_capture) =>
            {
                source.get(bounds.range())?;
                Some((*source, *bounds))
            }
            Origin::Captured { .. } => None,
            Origin::Synthetic | Origin::Uncaptured => None,
        }
    }

    /// Consumes the wrapper and discards all occurrence provenance.
    pub fn into_ast(self) -> T {
        self.ast
    }

    /// Renders the occurrence sidecar as text so that two parses can be compared exhaustively.
    ///
    /// The rendering is the whole retained tree, which the public views only expose piecemeal:
    /// `synthetic` or `uncaptured` for a parse without provenance, otherwise the captured window
    /// followed by the root occurrence. An occurrence is `(span: children ; separators)`, with
    /// `-` for an occurrence whose span was not retained, `_` for an absent child, and the
    /// separator list present only for repetitions that retained one. Two parses agree on
    /// provenance exactly when their renderings are equal.
    pub fn debug_provenance(&self) -> DebugProvenance<'_> {
        DebugProvenance {
            origin: &self.origin,
        }
    }
}

/// The textual rendering of one parse's occurrence sidecar; see [`Parsed::debug_provenance`].
pub struct DebugProvenance<'parsed> {
    /// The provenance being rendered.
    origin: &'parsed Origin<'parsed>,
}

impl fmt::Display for DebugProvenance<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.origin {
            Origin::Synthetic => formatter.write_str("synthetic"),
            Origin::Uncaptured => formatter.write_str("uncaptured"),
            Origin::Captured {
                bounds, provenance, ..
            } => {
                write!(formatter, "captured {}..{} ", bounds.start(), bounds.end())?;
                match &provenance.borrow_dependent().root {
                    None => formatter.write_str("none"),
                    Some(root) => root.render(formatter),
                }
            }
        }
    }
}

impl fmt::Debug for DebugProvenance<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl<T: Node> Parsed<'_, T> {
    /// Returns the generated concrete view of the root occurrence.
    pub fn root(&self) -> T::View<'_> {
        self.ast.__view(self.origin.view_cursor())
    }
}

impl<T: crate::requirement::Requires> Parsed<'_, T> {
    /// Returns the lowest configuration whose grammar accepts this value, with the span of the
    /// first construct, in source order, that sets it.
    ///
    /// `None` means the grammar's baseline configuration accepts the whole value.
    pub fn minimum_requirement(&self) -> Option<crate::requirement::Requirement> {
        crate::requirement::scan_from_cursor(
            &self.ast,
            self.origin.view_cursor(),
            crate::requirement::minimum_scan(),
        )
    }

    /// Returns the first construct, in source order, whose requirement is above `floor`.
    pub fn requirement_above(
        &self,
        floor: crate::requirement::Configuration,
    ) -> Option<crate::requirement::Requirement> {
        crate::requirement::scan_from_cursor(
            &self.ast,
            self.origin.view_cursor(),
            crate::requirement::above_scan(floor),
        )
    }
}

#[cfg(feature = "explorer")]
impl<T> Parsed<'_, T> {
    /// Exposes the typed root and occurrence cursor to the erased Explorer ABI.
    pub(crate) fn explorer_parts(&self) -> (&T, ViewCursor<'_>) {
        (&self.ast, self.origin.view_cursor())
    }

    /// Reports whether this document was constructed without parsed source.
    pub(crate) fn explorer_is_synthetic(&self) -> bool {
        matches!(self.origin, Origin::Synthetic)
    }
}

/// Common protocol implemented by generated concrete Node occurrence views.
pub trait NodeView: HasSpan {
    /// Authored semantic Node type referenced by this occurrence.
    type Value: ?Sized;

    /// The semantic value at this occurrence.
    fn value(&self) -> &Self::Value;

    /// Active grammar elements in authored source order.
    fn span_elements(&self) -> Elements<'_> {
        Elements::empty()
    }

    #[doc(hidden)]
    /// Returns the matching occurrence cursor behind this view.
    ///
    /// Generated views supply the cursor once; [`HasSpan`] and the view
    /// `Pretty` forwarding are implemented on top of it for every view.
    fn __recursa_cursor(&self) -> ViewCursor<'_>;
}

#[doc(hidden)]
/// Selects the provenance-element table for a value's active shape.
///
/// Generated Node bodies implement this once per Node; the grammar-local
/// generic `View` forwards [`NodeView::span_elements`] to it. Content tokens
/// keep the empty default.
pub trait NodeElements {
    /// Returns the active grammar elements of this value at `cursor`.
    fn __recursa_span_elements<'view>(&'view self, cursor: ViewCursor<'view>) -> Elements<'view> {
        let _ = cursor;
        Elements::empty()
    }
}

impl<V: NodeView> crate::output::span_seal::Sealed for V {}

impl<V: NodeView> HasSpan for V {
    fn span(&self) -> Option<Span> {
        self.__recursa_cursor().span()
    }
}

/// Renders one occurrence view by positioning the context at its cursor.
#[doc(hidden)]
pub fn pretty_view<V>(view: &V, context: &mut crate::PrettyContext) -> fmt::Result
where
    V: NodeView,
    V::Value: crate::Pretty,
{
    context.with_cursor(view.__recursa_cursor(), |context| {
        crate::Pretty::pretty(view.value(), context)
    })
}

/// Private seal implemented through the generated Node view factory.
mod node_seal {
    /// Prevents grammar authors from implementing [`super::Node`] manually.
    pub trait Sealed {}
}

/// Associates a semantic Node with its generated concrete occurrence view.
///
/// Implementations are supplied by matching Recursa generated code through
/// the doc-hidden ABI; grammar authors do not implement this trait directly.
pub trait Node: node_seal::Sealed {
    /// Concrete occurrence-view type borrowing the semantic value and sidecar.
    type View<'view>: NodeView<Value = Self>
    where
        Self: 'view;

    #[doc(hidden)]
    /// Constructs a concrete occurrence view from matching generated provenance.
    fn __view<'view>(&'view self, cursor: ViewCursor<'view>) -> Self::View<'view>;
}

impl<T: NodeViewFactory> node_seal::Sealed for T {}

impl<T: NodeViewFactory> Node for T {
    type View<'view>
        = T::View<'view>
    where
        Self: 'view;

    fn __view<'view>(&'view self, cursor: ViewCursor<'view>) -> Self::View<'view> {
        T::view(self, cursor)
    }
}

/// Matching-version factory implemented only by generated Node bodies.
#[doc(hidden)]
pub trait NodeViewFactory: Sized {
    /// Generated concrete occurrence-view type.
    type View<'view>: NodeView<Value = Self>
    where
        Self: 'view;

    /// Constructs the view for one matching occurrence cursor.
    fn view<'view>(&'view self, cursor: ViewCursor<'view>) -> Self::View<'view>;
}

/// Opaque occurrence cursor passed to generated concrete views.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ViewCursor<'view> {
    /// Matching sidecar occurrence, absent for synthetic or uncaptured values.
    occurrence: Option<&'view OccurrenceNode<'view>>,
    /// Complete parsed source used by occurrence spans.
    source: Option<&'view str>,
    /// Exact source window bounding retained document gaps.
    bounds: Option<Span>,
    /// Significant and rejected lexical records retained for opted-in gap views.
    records: Option<&'view [TokenRecord]>,
    /// Classified trivia retained for opted-in gap views.
    trivia: Option<&'view [TriviaRecord]>,
}

/// Preserves one absent generated child ordinal in copied Pretty provenance.
fn empty_pretty_provenance() -> crate::pretty::PrettyProvenance {
    crate::pretty::PrettyProvenance::new(None, None, Box::new([]), Box::new([]))
}

impl<'view> ViewCursor<'view> {
    /// Constructs a cursor for a value that carries no parse provenance.
    ///
    /// Every span taken through such a cursor is `None`, so a scan over a detached value still
    /// reports its requirements and reports no extents.
    pub fn detached() -> Self {
        Self::absent()
    }

    /// Constructs a cursor with no source provenance.
    fn absent() -> Self {
        Self {
            occurrence: None,
            source: None,
            bounds: None,
            records: None,
            trivia: None,
        }
    }

    /// Returns the extent for the generated view without exposing sidecar IDs.
    pub fn span(self) -> Option<Span> {
        self.occurrence.and_then(|occurrence| occurrence.span)
    }

    /// Selects a generated child occurrence without exposing its identity.
    pub fn child(self, index: usize) -> Self {
        Self {
            occurrence: self
                .occurrence
                .and_then(|occurrence| occurrence.children.get(index))
                .and_then(|occurrence| occurrence.node),
            source: self.source,
            bounds: self.bounds,
            records: self.records,
            trivia: self.trivia,
        }
    }

    /// Exact captured text for this occurrence, when available.
    pub fn text(self) -> Option<&'view str> {
        let span = self.span()?;
        self.source?.get(span.range())
    }

    /// Materializes the subset of provenance consumed by Pretty rendering.
    pub(crate) fn pretty_provenance(self) -> Option<crate::pretty::PrettyProvenance> {
        let occurrence = self.occurrence?;
        let text = occurrence
            .span
            .and_then(|span| self.source?.get(span.range()))
            .map(str::to_owned);
        let children = occurrence
            .children
            .iter()
            .map(|child| {
                ViewCursor {
                    occurrence: child.node,
                    source: self.source,
                    bounds: self.bounds,
                    records: self.records,
                    trivia: self.trivia,
                }
                .pretty_provenance()
                .unwrap_or_else(empty_pretty_provenance)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let repetition_separators = occurrence
            .repetition_separators
            .iter()
            .map(|separator| {
                let text = self
                    .source
                    .and_then(|source| source.get(separator.node.span.range()))
                    .expect("captured separator spans were validated against their source")
                    .to_owned();
                crate::pretty::RepetitionSeparatorProvenance {
                    text,
                    span: separator.node.span,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Some(crate::pretty::PrettyProvenance::new(
            text,
            occurrence.span,
            children,
            repetition_separators,
        ))
    }

    /// Binds this occurrence cursor to generated static element metadata.
    pub fn elements(self, metadata: &'static [ElementMetadata]) -> Elements<'view> {
        Elements {
            metadata,
            cursor: self,
            index: 0,
        }
    }

    /// Projects immutable beginning, inter-record, and end gaps for an opted-in document.
    pub fn source_gaps(self) -> Option<SourceGaps<'view>> {
        SourceGaps::new(self.source?, self.records?, self.trivia?, self.bounds?)
    }
}

/// Borrowed matched text or owned synthesized text for generated token types.
#[doc(hidden)]
#[derive(Clone)]
pub struct ContentText<'input>(
    /// Borrowed match or owned synthesized token text.
    ContentTextStorage<'input>,
);

/// Borrowed-or-owned storage behind a generated content token.
#[derive(Clone)]
enum ContentTextStorage<'input> {
    /// Exact text borrowed zero-copy from matched input.
    Matched(&'input str),
    /// Text owned by a synthesized token value.
    Owned(Box<str>),
}

impl<'input> ContentText<'input> {
    /// Stores synthesized content independently of any caller-owned buffer.
    pub fn owned(text: Box<str>) -> Self {
        Self(ContentTextStorage::Owned(text))
    }

    /// Borrows matched text only when `span` is a valid UTF-8 source range.
    pub(crate) fn matched(source: &'input str, span: Span) -> Option<Self> {
        source
            .get(span.range())
            .map(|text| Self(ContentTextStorage::Matched(text)))
    }

    /// Reports whether the text owns its bytes instead of borrowing the input.
    ///
    /// The Flat AST text pool counts owned texts so that it can release a pool of borrowed texts
    /// without walking every entry for drop glue ([`crate::flat::TextPool`]).
    pub(crate) fn is_owned(&self) -> bool {
        matches!(self.0, ContentTextStorage::Owned(_))
    }

    /// Borrows the token text independently of its storage mode.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            ContentTextStorage::Matched(text) => text,
            ContentTextStorage::Owned(text) => text,
        }
    }
}

impl fmt::Debug for ContentText<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ContentText")
            .field(&self.as_str())
            .finish()
    }
}

impl PartialEq for ContentText<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ContentText<'_> {}

impl PartialOrd for ContentText<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ContentText<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for ContentText<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ContentText<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de, 'input> serde::Deserialize<'de> for ContentText<'input> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Ok(Self::owned(text.into_boxed_str()))
    }
}

/// Generated Node capture policy fixed by the grammar plan.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePolicy {
    /// Never retain occurrence provenance.
    Off,
    /// Retain provenance only for explicit span-capturing entry points.
    Optional,
    /// Always retain provenance because generated behavior requires it.
    Mandatory,
}

/// Capture mode selected by the public root entry point.
#[derive(Clone, Copy)]
pub(crate) enum CaptureRequest {
    /// Request optional captures for public occurrence views.
    Spanned,
    /// Suppress every optional and mandatory provenance capture.
    WithoutSpans,
    /// Request one span per flat Node: the LR loop tracks token extents and never collects
    /// occurrence provenance, which is what `docs/adr/0008-keep-flat-asts-semantic-only.md`
    /// requires of a flat parse.
    FlatSpans,
}

impl CaptureRequest {
    /// Reports whether this build and entry point permit span retention of any kind.
    const fn permits_spans(self) -> bool {
        cfg!(feature = "spans") && !matches!(self, Self::WithoutSpans)
    }

    /// Reports whether the LR loop keeps an extent stack for this request.
    ///
    /// Only flat construction asks for extents, and it never asks for provenance, so the two are
    /// mutually exclusive by construction.
    const fn tracks_extents(self) -> bool {
        matches!(self, Self::FlatSpans) && cfg!(feature = "spans")
    }
}

impl CapturePolicy {
    /// Resolves grammar policy and caller request to an effective capture flag.
    fn captures(self, request: CaptureRequest) -> bool {
        if !request.permits_spans() {
            return false;
        }
        match (self, request) {
            // A flat parse retains spans on its own Nodes and never an occurrence tree, so no
            // grammar policy makes it capture provenance.
            (Self::Off, _) | (_, CaptureRequest::FlatSpans) => false,
            (Self::Optional, CaptureRequest::Spanned) | (Self::Mandatory, _) => true,
            (Self::Optional, CaptureRequest::WithoutSpans) => false,
        }
    }
}

/// Opaque move-owned occurrence produced alongside a recursive value.
#[doc(hidden)]
pub struct Occurrence<'arena> {
    /// Retained sidecar node, absent when this subtree captured nothing.
    node: Option<&'arena OccurrenceNode<'arena>>,
}

impl<'arena> Occurrence<'arena> {
    /// Represents an authored element that has no active occurrence.
    #[inline(always)]
    pub fn absent() -> Self {
        Self { node: None }
    }

    /// Renders the retained node as [`Parsed::debug_provenance`] renders it, or `_`.
    #[cfg(all(test, feature = "spans"))]
    pub(crate) fn render(&self) -> String {
        struct Render<'occurrence, 'arena>(&'occurrence Occurrence<'arena>);
        impl fmt::Display for Render<'_, '_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.0.node {
                    Some(node) => node.render(formatter),
                    None => formatter.write_str("_"),
                }
            }
        }
        Render(self).to_string()
    }

    /// Whether this occurrence retains any captured extent or descendant.
    #[inline(always)]
    pub fn is_captured(&self) -> bool {
        self.node.is_some()
    }
}

/// Opaque fixed-token provenance produced only for repetition separators.
///
/// This matching-version type intentionally cannot be converted into an
/// [`Occurrence`], so generated separator capture cannot alter semantic child
/// ordinals or any public Node view.
#[doc(hidden)]
pub struct RepetitionSeparatorOccurrence {
    /// Separator-specific sidecar that cannot become a semantic child.
    node: RepetitionSeparatorNode,
}

impl RepetitionSeparatorOccurrence {
    /// Wraps the span of a separator the table-driven parser shifted; the span was validated
    /// against the source when it was shifted.
    pub(crate) fn from_span(span: Span) -> Self {
        Self {
            node: RepetitionSeparatorNode { span },
        }
    }
}

/// Lazy ordinal accumulator for runtime-sized child occurrence sequences.
#[doc(hidden)]
pub struct Occurrences<'arena> {
    /// Uncaptured leading ordinals deferred until a later capture requires them.
    leading_absent: usize,
    /// Materialized occurrence ordinals beginning at the first capture.
    retained: BumpVec<'arena, Occurrence<'arena>>,
}

impl<'arena> Occurrences<'arena> {
    /// Sentinel selecting eager retention of every child ordinal.
    const RETAIN_ALL: usize = usize::MAX;

    /// Creates an empty lazy occurrence accumulator.
    pub fn new_in(arena: &'arena Bump) -> Self {
        Self {
            leading_absent: 0,
            retained: BumpVec::new_in(arena),
        }
    }

    /// Creates an accumulator for a fixed-size generated child sequence.
    ///
    /// When the enclosing occurrence captures its own span, every child ordinal must be
    /// retained even if every child is absent. Otherwise leading absent ordinals remain lazy
    /// until a captured descendant makes them observable.
    #[inline(always)]
    pub(crate) fn with_capacity_in(
        arena: &'arena Bump,
        capacity: usize,
        retain_absent: bool,
    ) -> Self {
        Self {
            leading_absent: if retain_absent { Self::RETAIN_ALL } else { 0 },
            retained: if retain_absent {
                BumpVec::with_capacity_in(capacity, arena)
            } else {
                BumpVec::new_in(arena)
            },
        }
    }

    /// Appends one ordinal while delaying leading absent allocation.
    #[inline(always)]
    pub fn push(&mut self, occurrence: Occurrence<'arena>) {
        if self.leading_absent == Self::RETAIN_ALL {
            self.retained.push(occurrence);
            return;
        }
        if occurrence.is_captured() {
            if self.retained.is_empty() {
                self.retained
                    .extend((0..self.leading_absent).map(|_| Occurrence::absent()));
            }
            self.retained.push(occurrence);
        } else if self.retained.is_empty() {
            self.leading_absent += 1;
        } else {
            self.retained.push(occurrence);
        }
    }

    /// Consumes the accumulator into its materialized occurrence ordinals.
    #[inline(always)]
    pub(crate) fn into_retained(self) -> BumpVec<'arena, Occurrence<'arena>> {
        self.retained
    }
}

/// Move-owned semantic value and occurrence produced by generated table parsing.
#[doc(hidden)]
pub struct ParsedNode<'arena, T> {
    /// Semantic value constructed by reduce actions.
    value: T,
    /// Opaque occurrence sidecar for parent assembly.
    occurrence: Occurrence<'arena>,
}

impl<'arena, T> ParsedNode<'arena, T> {
    /// Pairs a value the table-driven automaton built with the occurrence its fold produced.
    pub(crate) fn from_parts(value: T, occurrence: Occurrence<'arena>) -> Self {
        Self { value, occurrence }
    }

    /// Separates a value from its opaque occurrence for runtime tests.
    #[cfg(test)]
    pub(crate) fn into_parts(self) -> (T, Occurrence<'arena>) {
        (self.value, self.occurrence)
    }
}

/// Stateless builder that compacts occurrence provenance.
pub(crate) struct ProvenanceCollector<'arena> {
    /// Arena that owns every occurrence node and child sequence for this parse.
    arena: &'arena Bump,
}

impl<'arena> ProvenanceCollector<'arena> {
    /// Binds one collector to the parse-owned arena.
    fn new(arena: &'arena Bump) -> Self {
        Self { arena }
    }

    /// Finishes provenance while retaining separators outside child ordinals.
    ///
    /// Every concrete span reaching this method belongs to a token that the LR occurrence stack
    /// validated on shift, or is an extent assembled from the boundaries of those validated
    /// tokens. Construction therefore stays infallible and does not repeat UTF-8 range checks for
    /// every enclosing occurrence.
    #[inline(always)]
    pub(crate) fn finish_repeated(
        &self,
        policy: CapturePolicy,
        request: CaptureRequest,
        span: Option<Span>,
        children: BumpVec<'arena, Occurrence<'arena>>,
        repetition_separators: BumpVec<'arena, RepetitionSeparatorOccurrence>,
    ) -> Occurrence<'arena> {
        let span = policy.captures(request).then_some(span).flatten();
        // Default-policy callers keep leading absent ordinals lazy, so their common uncaptured
        // path stops at `is_empty`. Spanned empty productions at EOF can still carry eagerly
        // retained absent ordinals without a concrete span and need the fallback scan.
        if span.is_none()
            && (children.is_empty() || !children.iter().any(Occurrence::is_captured))
            && repetition_separators.is_empty()
        {
            return Occurrence::absent();
        }
        Occurrence {
            node: Some(self.arena.alloc(OccurrenceNode {
                span,
                children,
                repetition_separators,
            })),
        }
    }
}

/// Generated table-parser context.
///
/// The input, root FOLLOW, capture policy/request, and provenance collector remain within the
/// matching-version ABI. Generated reduce actions interact through [`crate::lr::Reduction`]; this
/// context owns the grammar-erased state shared by the LR loop and its diagnostics.
#[doc(hidden)]
pub struct ParseContext<'context, 'input, 'arena, A> {
    /// Grammar-bound cursor and caller construction state.
    input: &'context mut Input<'input, A>,
    /// FOLLOW supplied for this root invocation.
    follow: &'context FollowSet,
    /// Grammar-selected capture policy.
    policy: CapturePolicy,
    /// Root-call capture request.
    request: CaptureRequest,
    /// Builder for validated compact sidecars.
    provenance: ProvenanceCollector<'arena>,
}

impl<'context, 'input, 'arena, A> ParseContext<'context, 'input, 'arena, A> {
    /// Binds one table parse invocation to input, root FOLLOW, and capture state.
    fn new(
        input: &'context mut Input<'input, A>,
        follow: &'context FollowSet,
        policy: CapturePolicy,
        request: CaptureRequest,
        arena: &'arena Bump,
    ) -> Self {
        Self {
            input,
            follow,
            policy,
            request,
            provenance: ProvenanceCollector::new(arena),
        }
    }

    /// Returns the complete parse source.
    pub(crate) fn source(&self) -> &'input str {
        self.input.source()
    }

    /// Returns matching static grammar metadata.
    pub(crate) fn grammar(&self) -> &'static GrammarInfo {
        self.input.grammar()
    }

    /// Returns mutable caller construction state.
    pub(crate) fn builder(&mut self) -> &mut A {
        self.input.builder()
    }

    /// Returns one token record at lookahead distance without consuming it.
    pub(crate) fn peek(&self, distance: usize) -> Option<TokenRecord> {
        self.input.peek(distance)
    }

    /// Consumes and returns the next token record.
    pub(crate) fn advance(&mut self) -> Option<TokenRecord> {
        self.input.advance()
    }

    /// Returns the current absolute token cursor.
    pub(crate) fn cursor(&self) -> usize {
        self.input.cursor()
    }

    /// Restores a cursor saved by [`ParseContext::cursor`] after a failed transactional parse.
    pub(crate) fn restore(&mut self, cursor: usize) {
        self.input.restore(cursor);
    }

    /// Returns a record by absolute token index without moving the cursor.
    pub(crate) fn record(&self, index: usize) -> Option<TokenRecord> {
        self.input.record(index)
    }

    /// Reports whether this context's own policy retains spans under the root request.
    pub(crate) fn captures(&self) -> bool {
        self.policy.captures(self.request)
    }

    /// Reports whether this invocation needs an occurrence stack, including mandatory sites.
    ///
    /// A flat parse never needs one, not even for a mandatory site: flat construction has no
    /// occurrence sites to fill, so the request's extent tracking replaces provenance entirely.
    pub(crate) fn captures_provenance(&self, has_mandatory_sites: bool) -> bool {
        !self.request.tracks_extents()
            && self.request.permits_spans()
            && (self.captures() || has_mandatory_sites)
    }

    /// Reports whether the LR loop tracks one source extent per state-stack frame.
    ///
    /// Generated flat construction reads it to decide whether its stores keep a span per Node.
    pub fn tracks_extents(&self) -> bool {
        self.request.tracks_extents()
    }

    /// Returns the number of token records from the cursor to the end of the lexed input.
    ///
    /// Generated flat construction reads it once, before the automaton runs, to pre-size every
    /// store of the Flat AST from measured per-token multipliers. It is an upper bound on the
    /// tokens this parse can consume, because a parse never reads past the end of its window, and
    /// it is zero at EOF. It is a sizing hint only: a store that runs past its reservation grows.
    pub fn remaining_tokens(&self) -> usize {
        self.input.remaining_tokens()
    }

    /// Reports whether one resolved fold policy retains its own span for this parse.
    #[inline(always)]
    pub(crate) fn captures_policy(&self, policy: CapturePolicy) -> bool {
        policy.captures(self.request)
    }

    /// Returns the arena backing provenance retained by this parse.
    pub(crate) const fn provenance_arena(&self) -> &'arena Bump {
        self.provenance.arena
    }

    /// Returns the arena shared by generated AST containers and provenance.
    pub fn arena(&self) -> &'arena Bump {
        self.provenance.arena
    }

    /// Reports a shifted token span that leaves the source.
    pub(crate) fn invalid_token_span(&self, span: Span) -> ParseError {
        ParseError::invalid_token_span(self.input, span, None)
    }

    /// Runs the collector for a table-driven fold whose token spans were validated on shift.
    pub(crate) fn collect_occurrence(
        &self,
        policy: CapturePolicy,
        span: Option<Span>,
        children: BumpVec<'arena, Occurrence<'arena>>,
        separators: BumpVec<'arena, RepetitionSeparatorOccurrence>,
    ) -> Occurrence<'arena> {
        self.provenance
            .finish_repeated(policy, self.request, span, children, separators)
    }

    /// Returns the FOLLOW supplied for this root invocation.
    pub(crate) fn follow(&self) -> &FollowSet {
        self.follow
    }

    /// Returns the grammar-selected provenance capture policy.
    pub(crate) const fn capture_policy(&self) -> CapturePolicy {
        self.policy
    }

    /// Constructs an unexpected-input diagnostic at the current cursor.
    pub(crate) fn unexpected(&self, expected: &FollowSet) -> ParseError {
        ParseError::unexpected(self.input, expected)
    }

    /// Converts a matched record into borrowed content-token text.
    pub(crate) fn matched_text(
        &self,
        record: TokenRecord,
    ) -> Result<ContentText<'input>, ParseError> {
        ContentText::matched(self.input.source(), record.span).ok_or_else(|| {
            ParseError::invalid_token_span(self.input, record.span, Some(record.kind))
        })
    }

    /// Converts a matched record into content text whose borrow is shortened to the AST arena.
    pub fn matched_text_in_arena(
        &self,
        record: TokenRecord,
    ) -> Result<ContentText<'arena>, ParseError>
    where
        'input: 'arena,
    {
        let source: &'arena str = self.input.source();
        ContentText::matched(source, record.span).ok_or_else(|| {
            ParseError::invalid_token_span(self.input, record.span, Some(record.kind))
        })
    }
}
/// Matching-version table parsing protocol implemented by generated code.
#[doc(hidden)]
pub trait GeneratedParse<'input, A = ()>: Node + Sized {
    /// Grammar-fixed provenance capture policy for this Node.
    const CAPTURE_POLICY: CapturePolicy;

    /// Returns the exact grammar metadata that must own parser input.
    fn grammar() -> &'static GrammarInfo;

    /// Returns the caller FOLLOW used by this Node as a root.
    fn root_follow() -> &'static FollowSet;

    /// Runs the generated LR parser under a matching context.
    fn parse<'arena>(
        context: &mut ParseContext<'_, 'input, 'arena, A>,
    ) -> Result<ParsedNode<'arena, Self>, ParseError>;
}

/// Matching-version parsing protocol for one arena-backed AST family.
#[doc(hidden)]
pub trait ArenaGeneratedParse<'input, A = ()>: Sized {
    /// Lifetime-parametric authored AST produced by this root application.
    type Family: ArenaAstFamily;

    /// Grammar-fixed provenance capture policy for this root.
    const CAPTURE_POLICY: CapturePolicy;

    /// Returns the exact grammar metadata that must own parser input.
    fn grammar() -> &'static GrammarInfo;

    /// Returns the caller FOLLOW used by this root.
    fn root_follow() -> &'static FollowSet;

    /// Runs the generated LR parser with semantic containers borrowing the parse arena.
    fn parse_arena<'arena>(
        context: &mut ParseContext<'_, 'input, 'arena, A>,
    ) -> Result<ParsedNode<'arena, <Self::Family as ArenaAstFamily>::Ast<'arena>>, ParseError>
    where
        'input: 'arena;
}

/// Grammar-local input wrapper generated for one grammar.
///
/// The generated `Input` type implements this once, which lets one blanket
/// implementation supply the safe [`Parse`] facade for every generated Node
/// and content token instead of one forwarding impl per type.
#[doc(hidden)]
pub trait GeneratedInput {
    /// The runtime input wrapped by the generated grammar-local type.
    type Inner: ?Sized;

    /// Exposes the wrapped runtime input.
    fn __recursa_inner(&mut self) -> &mut Self::Inner;
}

impl<'input, A, I, T> parse_seal::Sealed<I> for T
where
    I: GeneratedInput<Inner = Input<'input, A>> + ?Sized,
    T: GeneratedParse<'input, A>,
{
}

impl<'input, A, I, T> Parse<I> for T
where
    I: GeneratedInput<Inner = Input<'input, A>> + ?Sized,
    T: GeneratedParse<'input, A>,
{
    type Output = Parsed<'input, T>;

    fn parse(input: &mut I) -> Result<Self::Output, ParseError> {
        parse::<T, A>(input.__recursa_inner())
    }
}

impl<'input, A, I, T> arena_parse_seal::Sealed<I> for T
where
    I: GeneratedInput<Inner = Input<'input, A>> + ?Sized,
    T: ArenaGeneratedParse<'input, A>,
{
}

impl<'input, A, I, T> ArenaParse<I> for T
where
    I: GeneratedInput<Inner = Input<'input, A>> + ?Sized,
    T: ArenaGeneratedParse<'input, A>,
{
    type Family = T::Family;
    type Output = ArenaParsed<'input, T::Family>;

    fn parse_arena(input: &mut I) -> Result<Self::Output, ParseError> {
        parse_arena::<T, A>(input.__recursa_inner())
    }
}

/// Runs the generated root parser with provenance enabled exactly when the `spans` feature is.
///
#[doc(hidden)]
pub fn parse<'input, T, A>(input: &mut Input<'input, A>) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request::<T, A>(input, ordinary_capture_request())
}

/// Runs the generated root parser with the explicit span-capture request.
///
#[doc(hidden)]
pub fn parse_spanned<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request::<T, A>(input, CaptureRequest::Spanned)
}

/// Runs the generated root parser while suppressing all provenance capture.
///
#[doc(hidden)]
pub fn parse_without_spans<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request::<T, A>(input, CaptureRequest::WithoutSpans)
}

/// Runs one generated arena-backed root parser with provenance selected by the `spans` feature.
#[doc(hidden)]
pub fn parse_arena<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<ArenaParsed<'input, T::Family>, ParseError>
where
    T: ArenaGeneratedParse<'input, A>,
{
    parse_arena_with_request_and_follow::<T, A>(input, ordinary_capture_request(), T::root_follow())
}

/// Selects the ordinary public parse behavior for this runtime feature set.
const fn ordinary_capture_request() -> CaptureRequest {
    if cfg!(feature = "spans") {
        CaptureRequest::Spanned
    } else {
        CaptureRequest::WithoutSpans
    }
}

/// Runs one generated arena-backed root parser with explicit span capture.
#[doc(hidden)]
pub fn parse_arena_spanned<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<ArenaParsed<'input, T::Family>, ParseError>
where
    T: ArenaGeneratedParse<'input, A>,
{
    parse_arena_with_request_and_follow::<T, A>(input, CaptureRequest::Spanned, T::root_follow())
}

/// Runs one generated arena-backed root parser while suppressing all provenance capture.
#[doc(hidden)]
pub fn parse_arena_without_spans<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<ArenaParsed<'input, T::Family>, ParseError>
where
    T: ArenaGeneratedParse<'input, A>,
{
    parse_arena_with_request_and_follow::<T, A>(
        input,
        CaptureRequest::WithoutSpans,
        T::root_follow(),
    )
}

/// Owns an arena-backed semantic root and provenance under one transactional parse.
fn parse_arena_with_request_and_follow<'input, T, A>(
    input: &mut Input<'input, A>,
    request: CaptureRequest,
    follow: &FollowSet,
) -> Result<ArenaParsed<'input, T::Family>, ParseError>
where
    T: ArenaGeneratedParse<'input, A>,
{
    if !std::ptr::eq(input.grammar(), T::grammar()) {
        return Err(ParseError::grammar_mismatch(input));
    }
    let checkpoint = input.cursor();
    let source = input.source();
    let bounds = input.bounds();
    let cell = match ArenaParseCell::try_new(
        ArenaOwner {
            arena: Bump::new(),
            source,
        },
        |owner| {
            let arena = &owner.arena;
            debug_assert!(std::ptr::eq(owner.source, source));
            let mut context = ParseContext::new(input, follow, T::CAPTURE_POLICY, request, arena);
            let ParsedNode { value, occurrence } = T::parse_arena(&mut context)?;
            Ok::<_, ParseError>(ArenaParseStorage {
                ast: ErasedArenaAst::new(value, arena),
                root: occurrence.node,
            })
        },
    ) {
        Ok(cell) => cell,
        Err(error) => {
            input.restore(checkpoint);
            return Err(error);
        }
    };
    let document = input.retained_document();
    let public_capture = cell
        .borrow_dependent()
        .root
        .is_some_and(OccurrenceNode::has_public_capture);
    let retains_source = public_capture || document.is_some();
    Ok(ArenaParsed {
        cell,
        source: retains_source.then_some(source),
        bounds: retains_source.then_some(bounds),
        document,
        family: PhantomData,
    })
}

/// Runs the generated root parser with one generated caller FOLLOW.
///
/// Document framing uses this seam to keep its separator outside the authored
/// root while preserving ordinary transactional parse behavior.
#[cfg(feature = "explorer")]
#[cfg(all(feature = "explorer", feature = "spans"))]
pub(crate) fn parse_spanned_with_root_follow<'input, T, A>(
    input: &mut Input<'input, A>,
    follow: &FollowSet,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request_and_follow::<T, A>(input, CaptureRequest::Spanned, follow)
}

/// Runs an arena-backed generated root parser with one framing caller FOLLOW.
#[cfg(feature = "explorer")]
#[cfg(all(feature = "explorer", feature = "spans"))]
pub(crate) fn parse_arena_spanned_with_root_follow<'input, T, A>(
    input: &mut Input<'input, A>,
    follow: &FollowSet,
) -> Result<ArenaParsed<'input, T::Family>, ParseError>
where
    T: ArenaGeneratedParse<'input, A>,
{
    parse_arena_with_request_and_follow::<T, A>(input, CaptureRequest::Spanned, follow)
}

/// Runs one generated flat construction transactionally over the same tables nested parsing uses.
///
/// This is the entry-point half of `Type::parse_flat`: generated code owns the automaton call and
/// its Flat AST stores, and this function owns everything that does not depend on the grammar's
/// own types. It checks that the input belongs to `grammar`, checkpoints the cursor, binds a
/// context whose request tracks token extents when `spans` asks for them and this build has the
/// `spans` feature, and restores the cursor when `drive` fails.
///
/// The context never collects occurrence provenance, whatever the grammar's capture policy or
/// mandatory sites say ([`ParseContext::captures_provenance`]), because a Flat AST has no
/// occurrence tree to fill. `drive` learns whether spans are retained from
/// [`ParseContext::tracks_extents`], so a build without the `spans` feature costs nothing.
///
/// The FOLLOW is the empty table-driven root set, exactly as [`GeneratedParse::root_follow`]
/// returns for the nested path, so prefix acceptance is identical.
#[doc(hidden)]
pub fn parse_flat_with<'input, A, R>(
    input: &mut Input<'input, A>,
    grammar: &'static GrammarInfo,
    spans: bool,
    drive: impl FnOnce(&mut ParseContext<'_, 'input, '_, A>) -> Result<R, ParseError>,
) -> Result<R, ParseError> {
    if !std::ptr::eq(input.grammar(), grammar) {
        return Err(ParseError::grammar_mismatch(input));
    }
    let request = if spans {
        CaptureRequest::FlatSpans
    } else {
        CaptureRequest::WithoutSpans
    };
    let checkpoint = input.cursor();
    // A flat parse retains no provenance, so this arena stays empty; the context still needs one
    // because every table parse shares the same context type.
    let arena = Bump::new();
    let mut context = ParseContext::new(
        input,
        crate::__private::empty_follow(),
        CapturePolicy::Off,
        request,
        &arena,
    );
    match drive(&mut context) {
        Ok(result) => Ok(result),
        Err(error) => {
            input.restore(checkpoint);
            Err(error)
        }
    }
}

/// Runs a root parser transactionally under one capture request.
fn parse_with_request<'input, T, A>(
    input: &mut Input<'input, A>,
    request: CaptureRequest,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request_and_follow::<T, A>(input, request, T::root_follow())
}

/// Runs a root parser transactionally under an explicit caller FOLLOW.
fn parse_with_request_and_follow<'input, T, A>(
    input: &mut Input<'input, A>,
    request: CaptureRequest,
    follow: &FollowSet,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    if !std::ptr::eq(input.grammar(), T::grammar()) {
        return Err(ParseError::grammar_mismatch(input));
    }
    let checkpoint = input.cursor();
    let source = input.source();
    let bounds = input.bounds();
    let mut value = None;
    let provenance = match ProvenanceCell::try_new(Bump::new(), |arena| {
        let mut context = ParseContext::new(input, follow, T::CAPTURE_POLICY, request, arena);
        let ParsedNode {
            value: parsed_value,
            occurrence,
        } = T::parse(&mut context)?;
        value = Some(parsed_value);
        Ok::<_, ParseError>(Provenance {
            root: occurrence.node,
        })
    }) {
        Ok(provenance) => provenance,
        Err(error) => {
            input.restore(checkpoint);
            return Err(error);
        }
    };
    let value = value.expect("a successful parse always constructs its semantic root");
    let document = input.retained_document();
    let root = provenance.borrow_dependent().root;
    let origin = if root.is_some() || document.is_some() {
        debug_assert!(root.is_none_or(OccurrenceNode::is_captured));
        Origin::Captured {
            source,
            bounds,
            provenance,
            document,
        }
    } else {
        Origin::Uncaptured
    };
    Ok(Parsed { ast: value, origin })
}

/// Binds a root-style [`ParseContext`] over an input for the runtime's own unit tests.
#[cfg(test)]
pub(crate) fn test_context<'input, A, R>(
    input: &mut Input<'input, A>,
    operation: impl FnOnce(&mut ParseContext<'_, 'input, '_, A>) -> R,
) -> R {
    test_context_with_request(input, CaptureRequest::WithoutSpans, operation)
}

/// Binds a root-style [`ParseContext`] with the span-capture request, as `parse_spanned` does.
#[cfg(all(test, feature = "spans"))]
pub(crate) fn test_context_spanned<'input, A, R>(
    input: &mut Input<'input, A>,
    operation: impl FnOnce(&mut ParseContext<'_, 'input, '_, A>) -> R,
) -> R {
    test_context_with_request(input, CaptureRequest::Spanned, operation)
}

/// Binds a root-style [`ParseContext`] that tracks extents, as `parse_flat` does.
#[cfg(all(test, feature = "spans"))]
pub(crate) fn test_context_flat<'input, A, R>(
    input: &mut Input<'input, A>,
    operation: impl FnOnce(&mut ParseContext<'_, 'input, '_, A>) -> R,
) -> R {
    test_context_with_request(input, CaptureRequest::FlatSpans, operation)
}

/// Binds a root-style [`ParseContext`] under an optional policy and the given request.
#[cfg(test)]
fn test_context_with_request<'input, A, R>(
    input: &mut Input<'input, A>,
    request: CaptureRequest,
    operation: impl FnOnce(&mut ParseContext<'_, 'input, '_, A>) -> R,
) -> R {
    static EMPTY_FOLLOW: FollowSet = FollowSet::from_static(0, &[], &[]);
    let arena = Bump::new();
    let mut context = ParseContext::new(
        input,
        &EMPTY_FOLLOW,
        CapturePolicy::Optional,
        request,
        &arena,
    );
    operation(&mut context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::LexResult;
    use crate::lex::{LexDiagnostic, LexDiagnosticCode};
    use crate::{ExpectedAtomId, ParseErrorKind};

    const WORD: u16 = 0;
    const LEXICAL_ERROR: u16 = 1;
    static GRAMMAR: GrammarInfo = GrammarInfo {
        name: "parsed-tests",
        kind_names: &["word", "lexical error"],
        atom_names: &["word"],
        lexical_error_kind: LEXICAL_ERROR,
    };
    static OTHER_GRAMMAR: GrammarInfo = GrammarInfo {
        name: "other-parsed-tests",
        kind_names: &["word", "lexical error"],
        atom_names: &["word"],
        lexical_error_kind: LEXICAL_ERROR,
    };
    static WORD_START: FollowSet = FollowSet::from_static(2, &[1 << WORD], &[ExpectedAtomId(0)]);
    static WORD_TABLES: crate::lr::LrTables = crate::lr::LrTables::new(
        1, b"BBAA", b"BD", b"BB", b"BACB", b"BA", b"BC", b"BAA", b"BBB", b"BAA", b"BAB", b"BB",
        b"B", b"BAAAAAA", b"BAA", false,
    );

    #[derive(Clone, Copy)]
    struct TokenView<'view, T> {
        value: &'view T,
        cursor: ViewCursor<'view>,
    }

    impl<T> NodeView for TokenView<'_, T> {
        fn __recursa_cursor(&self) -> ViewCursor<'_> {
            self.cursor
        }

        type Value = T;

        fn value(&self) -> &Self::Value {
            self.value
        }
    }

    trait WordValue<'input> {
        fn from_text(text: ContentText<'input>) -> Self;
    }

    fn reduce_word<'input, T: WordValue<'input>>(
        rule: u32,
        reduction: &mut crate::lr::Reduction<'_, '_, 'input, '_, ()>,
    ) -> Result<(), ParseError> {
        match rule {
            1 => {
                let record = reduction.token();
                let text = reduction.matched_text(record)?;
                reduction.push(T::from_text(text));
            }
            other => panic!("rule {other} is not reduced by the test table"),
        }
        Ok(())
    }

    macro_rules! test_token {
        ($name:ident, $policy:ident) => {
            #[derive(Debug)]
            struct $name<'input> {
                text: ContentText<'input>,
            }

            impl<'input> NodeViewFactory for $name<'input> {
                type View<'view>
                    = TokenView<'view, Self>
                where
                    Self: 'view;

                fn view<'view>(&'view self, cursor: ViewCursor<'view>) -> Self::View<'view> {
                    TokenView {
                        value: self,
                        cursor,
                    }
                }
            }

            impl<'input> GeneratedParse<'input> for $name<'input> {
                const CAPTURE_POLICY: CapturePolicy = CapturePolicy::$policy;

                fn grammar() -> &'static GrammarInfo {
                    &GRAMMAR
                }

                fn root_follow() -> &'static FollowSet {
                    &WORD_START
                }

                fn parse<'arena>(
                    context: &mut ParseContext<'_, 'input, 'arena, ()>,
                ) -> Result<ParsedNode<'arena, Self>, ParseError> {
                    crate::lr::run(context, &WORD_TABLES, 0, 0, reduce_word::<Self>)
                }
            }

            impl<'input> WordValue<'input> for $name<'input> {
                fn from_text(text: ContentText<'input>) -> Self {
                    Self { text }
                }
            }
        };
    }

    test_token!(OffToken, Off);
    test_token!(OptionalToken, Optional);
    test_token!(MandatoryToken, Mandatory);

    fn one_word(source: &str) -> LexResult<'_> {
        LexResult::new(
            source,
            vec![TokenRecord {
                kind: WORD,
                span: Span::new(0, source.len()).unwrap(),
                diagnostic: None,
            }],
            None,
        )
    }

    #[test]
    fn elements_length_tracks_the_remaining_iterator_items() {
        static METADATA: [ElementMetadata; 2] = [
            ElementMetadata {
                name: Some("fixed"),
                role: ElementRole::FixedToken,
            },
            ElementMetadata {
                name: Some("field"),
                role: ElementRole::SemanticField,
            },
        ];
        let mut elements = ViewCursor::absent().elements(&METADATA);

        assert_eq!(elements.len(), 2);
        assert!(!elements.is_empty());
        assert_eq!(elements.next().unwrap().name(), Some("fixed"));
        assert_eq!(elements.len(), 1);
        assert!(!elements.is_empty());
        assert_eq!(elements.next().unwrap().name(), Some("field"));
        assert_eq!(elements.len(), 0);
        assert!(elements.is_empty());
        assert!(elements.next().is_none());
    }

    #[test]
    fn occurrence_sequences_allocate_only_after_capture_and_preserve_ordinals() {
        let arena = Bump::new();
        let mut occurrences = Occurrences::new_in(&arena);
        occurrences.push(Occurrence::absent());
        occurrences.push(Occurrence::absent());
        assert_eq!(occurrences.retained.capacity(), 0);

        occurrences.push(Occurrence {
            node: Some(arena.alloc(OccurrenceNode {
                span: Some(Span::new(4, 5).unwrap()),
                children: BumpVec::new_in(&arena),
                repetition_separators: BumpVec::new_in(&arena),
            })),
        });
        occurrences.push(Occurrence::absent());

        let retained = occurrences.into_retained();
        assert_eq!(retained.len(), 4);
        assert!(!retained[0].is_captured());
        assert!(!retained[1].is_captured());
        assert!(retained[2].is_captured());
        assert!(!retained[3].is_captured());
    }

    #[test]
    fn occurrence_handles_are_pointer_sized_and_eager_sequences_keep_absent_ordinals() {
        assert_eq!(
            std::mem::size_of::<Occurrence<'_>>(),
            std::mem::size_of::<usize>()
        );

        let arena = Bump::new();
        let mut occurrences = Occurrences::with_capacity_in(&arena, 2, true);
        occurrences.push(Occurrence::absent());
        occurrences.push(Occurrence::absent());

        let retained = occurrences.into_retained();
        assert_eq!(retained.len(), 2);
        assert!(retained.iter().all(|occurrence| !occurrence.is_captured()));
    }

    /// Test-only grammar-local input matching the generated `Input` protocol.
    struct GrammarInput<'input>(Input<'input, ()>);

    impl<'input> GeneratedInput for GrammarInput<'input> {
        type Inner = Input<'input, ()>;

        fn __recursa_inner(&mut self) -> &mut Self::Inner {
            &mut self.0
        }
    }

    #[test]
    fn generated_input_exposes_the_parse_facade() {
        let lexed = one_word("word");
        let mut input = GrammarInput(lexed.input(&GRAMMAR));
        let parsed = <OptionalToken<'_> as Parse<GrammarInput<'_>>>::parse(&mut input).unwrap();
        assert_eq!(parsed.root().value().text.as_str(), "word");
    }

    #[test]
    fn root_capture_policy_and_request_cover_feature_and_entry_point_combinations() {
        macro_rules! assert_case {
            ($token:ident, $spanned:expr, $captured:expr) => {{
                let lexed = one_word("word");
                let mut input = lexed.input(&GRAMMAR);
                let parsed = if $spanned {
                    parse_spanned::<$token<'_>, _>(&mut input).unwrap()
                } else {
                    parse::<$token<'_>, _>(&mut input).unwrap()
                };
                let captured = cfg!(feature = "spans") && $captured;
                assert_eq!(parsed.source().is_some(), captured);
                assert_eq!(parsed.root().span().is_some(), captured);
                assert_eq!(parsed.root().value().text.as_str(), "word");
            }};
        }

        assert_case!(OffToken, false, false);
        assert_case!(OffToken, true, false);
        assert_case!(OptionalToken, false, true);
        assert_case!(OptionalToken, true, true);
        assert_case!(MandatoryToken, false, true);
        assert_case!(MandatoryToken, true, true);

        macro_rules! assert_without_spans {
            ($token:ident) => {{
                let lexed = one_word("word");
                let mut input = lexed.input(&GRAMMAR);
                let parsed = parse_without_spans::<$token<'_>, _>(&mut input).unwrap();
                assert_eq!(parsed.source(), None);
                assert_eq!(parsed.root().span(), None);
                assert_eq!(parsed.root().value().text.as_str(), "word");
            }};
        }

        assert_without_spans!(OffToken);
        assert_without_spans!(OptionalToken);
        assert_without_spans!(MandatoryToken);
    }

    #[test]
    fn invalid_generated_record_spans_are_structured_errors() {
        let lexed = LexResult::new(
            "x",
            vec![TokenRecord {
                kind: WORD,
                span: Span::new(0, 2).unwrap(),
                diagnostic: None,
            }],
            None,
        );
        let mut input = lexed.input(&GRAMMAR);

        let error = parse_spanned::<OptionalToken<'_>, _>(&mut input).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::InvalidTokenSpan);
        assert_eq!(error.code(), "RCA4103");
        assert_eq!(error.anchor(), Span::new(0, 2).unwrap());
    }

    #[test]
    fn lexical_parse_errors_preserve_mechanism_codes_and_primary_anchors() {
        let lexed = LexResult::new(
            "$x",
            vec![TokenRecord {
                kind: LEXICAL_ERROR,
                span: Span::new(0, 2).unwrap(),
                diagnostic: Some(LexDiagnostic {
                    code: LexDiagnosticCode::UnterminatedSameDelimiter,
                    anchor: Span::new(0, 1).unwrap(),
                }),
            }],
            None,
        );
        let mut input = lexed.input(&GRAMMAR);

        let error = parse::<OptionalToken<'_>, _>(&mut input).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::LexicalError);
        assert_eq!(error.code(), "RCA4001");
        assert_eq!(error.span(), Span::new(0, 2).unwrap());
        assert_eq!(error.anchor(), Span::new(0, 1).unwrap());
        assert_eq!(error.found(), Some("lexical error"));
        assert_eq!(error.expected().collect::<Vec<_>>(), vec!["word"]);
    }

    #[test]
    fn unclassified_lexical_parse_errors_use_the_runtime_fallback_diagnostic() {
        let lexed = LexResult::new(
            "?",
            vec![TokenRecord {
                kind: LEXICAL_ERROR,
                span: Span::new(0, 1).unwrap(),
                diagnostic: None,
            }],
            None,
        );
        let mut input = lexed.input(&GRAMMAR);

        let error = parse::<OptionalToken<'_>, _>(&mut input).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::LexicalError);
        assert_eq!(error.code(), "RCA4000");
        assert_eq!(error.anchor(), Span::new(0, 1).unwrap());
    }

    #[test]
    fn grammar_mismatches_expose_the_runtime_code_at_the_failure_anchor() {
        let lexed = one_word("word");
        let mut input = lexed.input(&OTHER_GRAMMAR);

        let error = parse::<OptionalToken<'_>, _>(&mut input).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::GrammarMismatch);
        assert_eq!(error.code(), "RCA4104");
        assert_eq!(error.anchor(), Span::new(0, 4).unwrap());
    }
}
