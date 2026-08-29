use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::{FollowSet, GrammarInfo, HasSpan, Input, ParseError, Span, TokenRecord};

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
        let cursor = match &self.origin {
            Origin::Synthetic | Origin::Uncaptured => ViewCursor::absent(),
            Origin::Captured { source, provenance } => ViewCursor {
                occurrence: Some(&provenance.root),
                source: Some(*source),
            },
        };
        context.with_cursor(cursor, |context| self.ast.pretty(context))
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
        /// Root of the immutable occurrence sidecar.
        provenance: Box<Provenance>,
    },
}

/// Owned occurrence provenance for one parsed root.
struct Provenance {
    /// Root occurrence whose descendants follow generated child ordinals.
    root: OccurrenceNode,
}

/// One private occurrence-sidecar node.
struct OccurrenceNode {
    /// Captured extent when requested or required.
    span: Option<Span>,
    /// Semantic and fixed-element children in generated ordinal order.
    children: Box<[OccurrenceNode]>,
    /// Fixed separators retained independently from semantic children.
    repetition_separators: Box<[RepetitionSeparatorNode]>,
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

impl OccurrenceNode {
    /// Reports whether this node or any private descendant retained provenance.
    fn is_captured(&self) -> bool {
        self.span.is_some()
            || self.children.iter().any(Self::is_captured)
            || !self.repetition_separators.is_empty()
    }

    /// Reports whether public source access is justified by a captured span.
    fn has_public_capture(&self) -> bool {
        self.span.is_some() || self.children.iter().any(Self::has_public_capture)
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

    /// Returns captured source text, if this parse captured provenance.
    pub fn source(&self) -> Option<&'input str> {
        match &self.origin {
            Origin::Captured { source, provenance } if provenance.root.has_public_capture() => {
                Some(*source)
            }
            Origin::Captured { .. } => None,
            Origin::Synthetic | Origin::Uncaptured => None,
        }
    }

    /// Consumes the wrapper and discards all occurrence provenance.
    pub fn into_ast(self) -> T {
        self.ast
    }
}

impl<T: Node> Parsed<'_, T> {
    /// Returns the generated concrete view of the root occurrence.
    pub fn root(&self) -> T::View<'_> {
        self.ast.__view(match &self.origin {
            Origin::Synthetic | Origin::Uncaptured => ViewCursor::absent(),
            Origin::Captured { source, provenance } => ViewCursor {
                occurrence: Some(&provenance.root),
                source: Some(*source),
            },
        })
    }
}

#[cfg(feature = "explorer")]
impl<T> Parsed<'_, T> {
    /// Exposes the typed root and occurrence cursor to the erased Explorer ABI.
    pub(crate) fn explorer_parts(&self) -> (&T, ViewCursor<'_>) {
        let cursor = match &self.origin {
            Origin::Synthetic | Origin::Uncaptured => ViewCursor::absent(),
            Origin::Captured { source, provenance } => ViewCursor {
                occurrence: Some(&provenance.root),
                source: Some(*source),
            },
        };
        (&self.ast, cursor)
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
    occurrence: Option<&'view OccurrenceNode>,
    /// Complete parsed source used by occurrence spans.
    source: Option<&'view str>,
}

impl<'view> ViewCursor<'view> {
    /// Constructs a cursor with no source provenance.
    fn absent() -> Self {
        Self {
            occurrence: None,
            source: None,
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
                .and_then(|occurrence| occurrence.children.get(index)),
            source: self.source,
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
                    occurrence: Some(child),
                    source: self.source,
                }
                .pretty_provenance()
                .expect("a child cursor always has an occurrence")
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let repetition_separators = occurrence
            .repetition_separators
            .iter()
            .map(|separator| {
                let text = self
                    .source
                    .and_then(|source| source.get(separator.span.range()))
                    .expect("captured separator spans were validated against their source")
                    .to_owned();
                crate::pretty::RepetitionSeparatorProvenance {
                    text,
                    span: separator.span,
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
    fn matched(source: &'input str, span: Span) -> Option<Self> {
        source
            .get(span.range())
            .map(|text| Self(ContentTextStorage::Matched(text)))
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
enum CaptureRequest {
    /// Use only captures required by the grammar plan.
    Default,
    /// Request optional captures for public occurrence views.
    Spanned,
}

impl CapturePolicy {
    /// Resolves grammar policy and caller request to an effective capture flag.
    fn captures(self, request: CaptureRequest) -> bool {
        match (self, request) {
            (Self::Off, _) => false,
            (Self::Optional, CaptureRequest::Default) => false,
            (Self::Optional, CaptureRequest::Spanned) | (Self::Mandatory, _) => true,
        }
    }
}

/// Opaque move-owned occurrence produced alongside a recursive value.
#[doc(hidden)]
pub struct Occurrence {
    /// Retained sidecar node, absent when this subtree captured nothing.
    node: Option<OccurrenceNode>,
}

impl Occurrence {
    /// Represents an authored element that has no active occurrence.
    pub fn absent() -> Self {
        Self { node: None }
    }

    /// Whether this occurrence retains any captured extent or descendant.
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

/// Lazy ordinal accumulator for runtime-sized child occurrence sequences.
#[doc(hidden)]
pub struct Occurrences {
    /// Uncaptured leading ordinals deferred until a later capture requires them.
    leading_absent: usize,
    /// Materialized occurrence ordinals beginning at the first capture.
    retained: Vec<Occurrence>,
}

impl Occurrences {
    /// Creates an empty lazy occurrence accumulator.
    pub const fn new() -> Self {
        Self {
            leading_absent: 0,
            retained: Vec::new(),
        }
    }

    /// Appends one ordinal while delaying leading absent allocation.
    pub fn push(&mut self, occurrence: Occurrence) {
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
}

impl Default for Occurrences {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for Occurrences {
    type Item = Occurrence;
    type IntoIter = std::vec::IntoIter<Occurrence>;

    fn into_iter(self) -> Self::IntoIter {
        self.retained.into_iter()
    }
}

/// Move-owned semantic value and occurrence produced by recursive generated parsing.
#[doc(hidden)]
pub struct ParsedNode<T> {
    /// Recursively constructed semantic value.
    value: T,
    /// Opaque occurrence sidecar for parent assembly.
    occurrence: Occurrence,
}

impl<T> ParsedNode<T> {
    /// Transforms a recursive semantic result without detaching its occurrence.
    pub fn map<U>(self, transform: impl FnOnce(T) -> U) -> ParsedNode<U> {
        ParsedNode {
            value: transform(self.value),
            occurrence: self.occurrence,
        }
    }

    /// Separates a child value from its opaque occurrence for parent assembly.
    pub fn into_parts(self) -> (T, Occurrence) {
        (self.value, self.occurrence)
    }
}

/// Stateless builder that validates and compacts occurrence provenance.
struct ProvenanceCollector;

impl ProvenanceCollector {
    /// Finishes provenance for a non-repetition grammar occurrence.
    fn finish(
        &self,
        source: &str,
        policy: CapturePolicy,
        request: CaptureRequest,
        span: Option<Span>,
        children: impl IntoIterator<Item = Occurrence>,
    ) -> Result<Occurrence, ()> {
        self.finish_repeated(source, policy, request, span, children, std::iter::empty())
    }

    /// Finishes provenance while retaining separators outside child ordinals.
    fn finish_repeated(
        &self,
        source: &str,
        policy: CapturePolicy,
        request: CaptureRequest,
        span: Option<Span>,
        children: impl IntoIterator<Item = Occurrence>,
        separators: impl IntoIterator<Item = RepetitionSeparatorOccurrence>,
    ) -> Result<Occurrence, ()> {
        if let Some(span) = span {
            source.get(span.range()).ok_or(())?;
        }
        let span = policy.captures(request).then_some(span).flatten();
        let mut leading_absent = 0;
        let mut retained = Vec::new();
        for occurrence in children {
            match occurrence.node {
                Some(node) => {
                    if retained.is_empty() {
                        retained.extend((0..leading_absent).map(|_| OccurrenceNode {
                            span: None,
                            children: Box::new([]),
                            repetition_separators: Box::new([]),
                        }));
                    }
                    retained.push(node);
                }
                None if retained.is_empty() => leading_absent += 1,
                None => retained.push(OccurrenceNode {
                    span: None,
                    children: Box::new([]),
                    repetition_separators: Box::new([]),
                }),
            }
        }
        let repetition_separators = separators
            .into_iter()
            .map(|separator| separator.node)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        if span.is_none() && retained.is_empty() && repetition_separators.is_empty() {
            return Ok(Occurrence::absent());
        }
        if span.is_some() && retained.is_empty() && leading_absent != 0 {
            retained.extend((0..leading_absent).map(|_| OccurrenceNode {
                span: None,
                children: Box::new([]),
                repetition_separators: Box::new([]),
            }));
        }
        Ok(Occurrence {
            node: Some(OccurrenceNode {
                span,
                children: retained.into_boxed_slice(),
                repetition_separators,
            }),
        })
    }
}

/// Recursive generated-parser context.
///
/// The input, caller FOLLOW, capture policy/request, and provenance collector
/// remain within the matching-version ABI.
#[doc(hidden)]
pub struct ParseContext<'context, 'input, A> {
    /// Grammar-bound cursor and caller construction state.
    input: &'context mut Input<'input, A>,
    /// Caller FOLLOW composed for this recursive site.
    follow: &'context FollowSet,
    /// Grammar-selected capture policy.
    policy: CapturePolicy,
    /// Root-call capture request.
    request: CaptureRequest,
    /// Builder for validated compact sidecars.
    provenance: ProvenanceCollector,
}

impl<'context, 'input, A> ParseContext<'context, 'input, A> {
    /// Binds one recursive parse invocation to input, FOLLOW, and capture state.
    fn new(
        input: &'context mut Input<'input, A>,
        follow: &'context FollowSet,
        policy: CapturePolicy,
        request: CaptureRequest,
    ) -> Self {
        Self {
            input,
            follow,
            policy,
            request,
            provenance: ProvenanceCollector,
        }
    }

    /// Returns the complete parse source.
    pub fn source(&self) -> &'input str {
        self.input.source()
    }

    /// Returns matching static grammar metadata.
    pub fn grammar(&self) -> &'static GrammarInfo {
        self.input.grammar()
    }

    /// Returns mutable caller construction state.
    pub fn builder(&mut self) -> &mut A {
        self.input.builder()
    }

    /// Returns one token record at lookahead distance without consuming it.
    pub fn peek(&self, distance: usize) -> Option<TokenRecord> {
        self.input.peek(distance)
    }

    /// Consumes and returns the next token record.
    pub fn advance(&mut self) -> Option<TokenRecord> {
        self.input.advance()
    }

    /// Returns the current absolute token cursor.
    pub fn cursor(&self) -> usize {
        self.input.cursor()
    }

    /// Returns the source extent consumed since a saved token cursor.
    pub fn consumed_span_from(&self, start: usize) -> Option<Span> {
        let end = self.input.cursor();
        let first = self.input.record(start)?;
        let last = self.input.record(end.checked_sub(1)?)?;
        Span::new(first.span.start() as usize, last.span.end() as usize).ok()
    }

    /// Returns the caller FOLLOW set for this recursive parse site.
    pub fn follow(&self) -> &FollowSet {
        self.follow
    }

    /// Returns the grammar-selected provenance capture policy.
    pub const fn capture_policy(&self) -> CapturePolicy {
        self.policy
    }

    /// Constructs an unexpected-input diagnostic at the current cursor.
    pub fn unexpected(&self, expected: &FollowSet) -> ParseError {
        ParseError::unexpected(self.input, expected)
    }

    /// Constructs an unexpected-input diagnostic at lookahead distance.
    pub fn unexpected_at(&self, distance: usize, expected: &FollowSet) -> ParseError {
        ParseError::unexpected_at(self.input, expected, distance)
    }

    /// Reports the dynamic D3 ambiguity at the current Pratt extender.
    pub fn pratt_ambiguity(&self, expected: &FollowSet) -> ParseError {
        ParseError::pratt_ambiguity(self.input, expected)
    }

    /// Reports whether the next token belongs to `expected`.
    pub fn next_is(&self, expected: &FollowSet) -> bool {
        self.peek(0)
            .is_some_and(|record| expected.contains(record.kind))
    }

    /// Reports whether no token remains.
    pub fn is_eof(&self) -> bool {
        self.peek(0).is_none()
    }

    /// Consumes one fixed element and returns its occurrence provenance.
    pub fn parse_fixed(
        &mut self,
        expected: &FollowSet,
        policy: CapturePolicy,
    ) -> Result<Occurrence, ParseError> {
        let Some(record) = self.peek(0) else {
            return Err(self.unexpected(expected));
        };
        if !expected.contains(record.kind) {
            return Err(self.unexpected(expected));
        }
        self.advance();
        self.provenance
            .finish(
                self.input.source(),
                policy,
                self.request,
                Some(record.span),
                std::iter::empty(),
            )
            .map_err(|()| ParseError::invalid_token_span(self.input, record.span, None))
    }

    /// Consumes one fixed repetition separator into its dedicated provenance
    /// channel. Separator source text remains available to Pretty regardless
    /// of the semantic Node capture policy.
    pub fn parse_repetition_separator(
        &mut self,
        expected: &FollowSet,
    ) -> Result<RepetitionSeparatorOccurrence, ParseError> {
        let Some(record) = self.peek(0) else {
            return Err(self.unexpected(expected));
        };
        if !expected.contains(record.kind) {
            return Err(self.unexpected(expected));
        }
        self.input
            .source()
            .get(record.span.range())
            .ok_or_else(|| ParseError::invalid_token_span(self.input, record.span, None))?;
        self.advance();
        Ok(RepetitionSeparatorOccurrence {
            node: RepetitionSeparatorNode { span: record.span },
        })
    }

    /// Converts a matched record into borrowed content-token text.
    pub fn matched_text(&self, record: TokenRecord) -> Result<ContentText<'input>, ParseError> {
        ContentText::matched(self.input.source(), record.span).ok_or_else(|| {
            ParseError::invalid_token_span(self.input, record.span, Some(record.kind))
        })
    }

    /// Completes a leaf Node with its semantic value and captured span.
    pub fn finish<T>(&self, value: T, span: Span) -> Result<ParsedNode<T>, ParseError> {
        let occurrence = self
            .provenance
            .finish(
                self.input.source(),
                self.policy,
                self.request,
                Some(span),
                std::iter::empty(),
            )
            .map_err(|()| ParseError::invalid_token_span(self.input, span, None))?;
        Ok(ParsedNode { value, occurrence })
    }

    /// Completes a parent while retaining successful child occurrences.
    pub fn finish_with_children<T>(
        &self,
        value: T,
        span: Option<Span>,
        children: impl IntoIterator<Item = Occurrence>,
    ) -> Result<ParsedNode<T>, ParseError> {
        let occurrence = self
            .provenance
            .finish(
                self.input.source(),
                self.policy,
                self.request,
                span,
                children,
            )
            .map_err(|()| {
                ParseError::invalid_token_span(
                    self.input,
                    span.expect("only a concrete invalid span can fail validation"),
                    None,
                )
            })?;
        Ok(ParsedNode { value, occurrence })
    }

    /// Completes a locally overridden Node/variant capture boundary.
    pub fn finish_with_children_policy<T>(
        &self,
        value: T,
        span: Option<Span>,
        policy: CapturePolicy,
        children: impl IntoIterator<Item = Occurrence>,
    ) -> Result<ParsedNode<T>, ParseError> {
        let occurrence = self
            .provenance
            .finish(self.input.source(), policy, self.request, span, children)
            .map_err(|()| {
                ParseError::invalid_token_span(
                    self.input,
                    span.expect("only a concrete invalid span can fail validation"),
                    None,
                )
            })?;
        Ok(ParsedNode { value, occurrence })
    }

    /// Completes one authored element slot, including repeated child occurrences.
    pub fn finish_group(
        &self,
        policy: CapturePolicy,
        span: Option<Span>,
        children: impl IntoIterator<Item = Occurrence>,
    ) -> Result<Occurrence, ParseError> {
        self.provenance
            .finish(self.input.source(), policy, self.request, span, children)
            .map_err(|()| {
                ParseError::invalid_token_span(
                    self.input,
                    span.expect("only a concrete invalid span can fail validation"),
                    None,
                )
            })
    }

    /// Completes one repeated authored element while keeping fixed separator
    /// occurrences separate from the semantic child sequence.
    pub fn finish_repeated_group(
        &self,
        policy: CapturePolicy,
        span: Option<Span>,
        children: impl IntoIterator<Item = Occurrence>,
        separators: impl IntoIterator<Item = RepetitionSeparatorOccurrence>,
    ) -> Result<Occurrence, ParseError> {
        self.provenance
            .finish_repeated(
                self.input.source(),
                policy,
                self.request,
                span,
                children,
                separators,
            )
            .map_err(|()| {
                ParseError::invalid_token_span(
                    self.input,
                    span.expect("only a concrete invalid span can fail validation"),
                    None,
                )
            })
    }

    /// Invokes a generated child parser with its own FOLLOW and capture policy.
    pub fn parse_node<T>(
        &mut self,
        follow: &FollowSet,
        policy: CapturePolicy,
    ) -> Result<ParsedNode<T>, ParseError>
    where
        T: GeneratedParse<'input, A>,
    {
        if !std::ptr::eq(self.input.grammar(), T::grammar()) {
            return Err(ParseError::grammar_mismatch(self.input));
        }
        let mut child = ParseContext::new(&mut *self.input, follow, policy, self.request);
        T::parse(&mut child)
    }

    /// Runs one generated recursive parser with its planned FOLLOW and capture policy.
    pub fn parse_generated<T>(
        &mut self,
        follow: &FollowSet,
        policy: CapturePolicy,
        parse: impl FnOnce(&mut ParseContext<'_, 'input, A>) -> Result<ParsedNode<T>, ParseError>,
    ) -> Result<ParsedNode<T>, ParseError> {
        let mut child = ParseContext::new(&mut *self.input, follow, policy, self.request);
        parse(&mut child)
    }
}

/// Matching-version recursive parsing protocol implemented by generated code.
#[doc(hidden)]
pub trait GeneratedParse<'input, A = ()>: Node + Sized {
    /// Grammar-fixed provenance capture policy for this Node.
    const CAPTURE_POLICY: CapturePolicy;

    /// Returns the exact grammar metadata that must own parser input.
    fn grammar() -> &'static GrammarInfo;

    /// Returns the caller FOLLOW used by this Node as a root.
    fn root_follow() -> &'static FollowSet;

    /// Runs the generated recursive parser under a matching context.
    fn parse(context: &mut ParseContext<'_, 'input, A>) -> Result<ParsedNode<Self>, ParseError>;
}

/// Runs the generated root parser with the default capture request.
#[doc(hidden)]
pub fn parse<'input, T, A>(input: &mut Input<'input, A>) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request::<T, A>(input, CaptureRequest::Default)
}

/// Runs the generated root parser with the explicit span-capture request.
#[doc(hidden)]
pub fn parse_spanned<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    parse_with_request::<T, A>(input, CaptureRequest::Spanned)
}

/// Runs a root parser transactionally under one capture request.
fn parse_with_request<'input, T, A>(
    input: &mut Input<'input, A>,
    request: CaptureRequest,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: GeneratedParse<'input, A>,
{
    if !std::ptr::eq(input.grammar(), T::grammar()) {
        return Err(ParseError::grammar_mismatch(input));
    }
    let checkpoint = input.cursor();
    let source = input.source();
    let mut context = ParseContext::new(input, T::root_follow(), T::CAPTURE_POLICY, request);
    let ParsedNode { value, occurrence } = match T::parse(&mut context) {
        Ok(parsed) => parsed,
        Err(error) => {
            input.restore(checkpoint);
            return Err(error);
        }
    };
    let origin = if let Some(root) = occurrence.node {
        debug_assert!(root.is_captured());
        Origin::Captured {
            source,
            provenance: Box::new(Provenance { root }),
        }
    } else {
        Origin::Uncaptured
    };
    Ok(Parsed { ast: value, origin })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExpectedAtomId, LexResult, ParseErrorKind};

    const WORD: u16 = 0;
    const LEXICAL_ERROR: u16 = 1;
    static GRAMMAR: GrammarInfo = GrammarInfo {
        name: "parsed-tests",
        kind_names: &["word", "lexical error"],
        atom_names: &["word"],
        lexical_error_kind: LEXICAL_ERROR,
    };
    static WORD_START: FollowSet = FollowSet::from_static(2, &[1 << WORD], &[ExpectedAtomId(0)]);

    #[derive(Clone, Copy)]
    struct TokenView<'view, T> {
        value: &'view T,
        cursor: ViewCursor<'view>,
    }

    impl<T> crate::output::span_seal::Sealed for TokenView<'_, T> {}

    impl<T> HasSpan for TokenView<'_, T> {
        fn span(&self) -> Option<Span> {
            self.cursor.span()
        }
    }

    impl<T> NodeView for TokenView<'_, T> {
        type Value = T;

        fn value(&self) -> &Self::Value {
            self.value
        }
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

                fn parse(
                    context: &mut ParseContext<'_, 'input, ()>,
                ) -> Result<ParsedNode<Self>, ParseError> {
                    let Some(record) = context.peek(0) else {
                        return Err(context.unexpected(&WORD_START));
                    };
                    if record.kind != WORD {
                        return Err(context.unexpected(&WORD_START));
                    }
                    context.advance();
                    let text = context.matched_text(record)?;
                    context.finish(Self { text }, record.span)
                }
            }
        };
    }

    test_token!(OffToken, Off);
    test_token!(OptionalToken, Optional);
    test_token!(MandatoryToken, Mandatory);

    #[derive(Debug)]
    struct FailsAfterOccurrence;

    impl NodeViewFactory for FailsAfterOccurrence {
        type View<'view> = TokenView<'view, Self>;

        fn view<'view>(&'view self, cursor: ViewCursor<'view>) -> Self::View<'view> {
            TokenView {
                value: self,
                cursor,
            }
        }
    }

    impl<'input> GeneratedParse<'input> for FailsAfterOccurrence {
        const CAPTURE_POLICY: CapturePolicy = CapturePolicy::Mandatory;

        fn grammar() -> &'static GrammarInfo {
            &GRAMMAR
        }

        fn root_follow() -> &'static FollowSet {
            &WORD_START
        }

        fn parse(
            context: &mut ParseContext<'_, 'input, ()>,
        ) -> Result<ParsedNode<Self>, ParseError> {
            let record = context.advance().expect("fixture has a first token");
            let _partial = context.finish(Self, record.span)?;
            Err(context.unexpected(&WORD_START))
        }
    }

    fn one_word(source: &str) -> LexResult<'_> {
        LexResult::new(
            source,
            vec![TokenRecord {
                kind: WORD,
                span: Span::new(0, source.len()).unwrap(),
                diagnostic: None,
            }],
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
        let mut occurrences = Occurrences::new();
        occurrences.push(Occurrence::absent());
        occurrences.push(Occurrence::absent());
        assert_eq!(occurrences.retained.capacity(), 0);

        occurrences.push(Occurrence {
            node: Some(OccurrenceNode {
                span: Some(Span::new(4, 5).unwrap()),
                children: Box::new([]),
                repetition_separators: Box::new([]),
            }),
        });
        occurrences.push(Occurrence::absent());

        let retained = occurrences.into_iter().collect::<Vec<_>>();
        assert_eq!(retained.len(), 4);
        assert!(!retained[0].is_captured());
        assert!(!retained[1].is_captured());
        assert!(retained[2].is_captured());
        assert!(!retained[3].is_captured());
    }

    #[test]
    fn root_capture_policy_and_request_cover_all_six_combinations() {
        macro_rules! assert_case {
            ($token:ident, $spanned:expr, $captured:expr) => {{
                let lexed = one_word("word");
                let mut input = lexed.input(&GRAMMAR);
                let parsed = if $spanned {
                    parse_spanned::<$token<'_>, _>(&mut input).unwrap()
                } else {
                    parse::<$token<'_>, _>(&mut input).unwrap()
                };
                assert_eq!(parsed.source().is_some(), $captured);
                assert_eq!(parsed.root().span().is_some(), $captured);
                assert_eq!(parsed.root().value().text.as_str(), "word");
            }};
        }

        assert_case!(OffToken, false, false);
        assert_case!(OffToken, true, false);
        assert_case!(OptionalToken, false, false);
        assert_case!(OptionalToken, true, true);
        assert_case!(MandatoryToken, false, true);
        assert_case!(MandatoryToken, true, true);
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
        );
        let mut input = lexed.input(&GRAMMAR);

        let error = parse_spanned::<OptionalToken<'_>, _>(&mut input).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::InvalidTokenSpan);
    }

    #[test]
    fn parse_errors_expose_symbolic_names_without_numeric_runtime_ids() {
        let lexed = LexResult::new(
            "?",
            vec![TokenRecord {
                kind: LEXICAL_ERROR,
                span: Span::new(0, 1).unwrap(),
                diagnostic: None,
            }],
        );
        let mut input = lexed.input(&GRAMMAR);

        let error = parse::<OptionalToken<'_>, _>(&mut input).unwrap_err();

        assert_eq!(error.kind(), ParseErrorKind::LexicalError);
        assert_eq!(error.found(), Some("lexical error"));
        assert_eq!(error.expected().collect::<Vec<_>>(), vec!["word"]);
    }

    #[test]
    fn failed_parses_publish_no_partial_occurrence_provenance() {
        let lexed = LexResult::new(
            "a b",
            vec![
                TokenRecord {
                    kind: WORD,
                    span: Span::new(0, 1).unwrap(),
                    diagnostic: None,
                },
                TokenRecord {
                    kind: WORD,
                    span: Span::new(2, 3).unwrap(),
                    diagnostic: None,
                },
            ],
        );
        let mut input = lexed.input(&GRAMMAR);

        assert!(parse::<FailsAfterOccurrence, _>(&mut input).is_err());
        let parsed = parse_spanned::<OptionalToken<'_>, _>(&mut input).unwrap();

        assert_eq!(parsed.root().value().text.as_str(), "a");
        assert_eq!(parsed.root().span(), Some(Span::new(0, 1).unwrap()));
    }
}
