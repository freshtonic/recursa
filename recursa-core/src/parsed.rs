use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::{FollowSet, GrammarInfo, HasSpan, Input, ParseError, Span, TokenRecord};

#[doc(hidden)]
pub mod parse_seal {
    pub trait Sealed<I: ?Sized> {}
}

/// Safe parsing capability implemented only by matching Recursa generations.
///
/// `I` is a generated grammar-local input type. Recursive parsing and FOLLOW
/// transport are deliberately absent from this public contract.
pub trait Parse<I: ?Sized>: parse_seal::Sealed<I> {
    type Output;

    fn parse(input: &mut I) -> Result<Self::Output, ParseError>;
}

/// A parsed semantic value together with its immutable occurrence provenance.
///
/// Mutating access is deliberately unavailable: consume the wrapper with
/// [`Parsed::into_ast`] before changing the semantic value.
pub struct Parsed<'input, T> {
    ast: T,
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

enum Origin<'input> {
    Synthetic,
    Uncaptured,
    Captured {
        source: &'input str,
        provenance: Box<Provenance>,
    },
}

struct Provenance {
    root: OccurrenceNode,
}

struct OccurrenceNode {
    span: Option<Span>,
    children: Box<[OccurrenceNode]>,
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
    pub(crate) name: Option<&'static str>,
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
    metadata: &'static ElementMetadata,
    cursor: ViewCursor<'view>,
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
    metadata: &'static [ElementMetadata],
    cursor: ViewCursor<'view>,
    index: usize,
}

/// A semantic repetition paired with its aggregate occurrence provenance.
pub struct Repeated<'view, T: Node, U> {
    values: &'view [U],
    cursor: ViewCursor<'view>,
    project: fn(&U) -> &T,
    marker: PhantomData<&'view T>,
}

impl<'view, T: Node, U> Repeated<'view, T, U> {
    #[doc(hidden)]
    pub fn new(values: &'view [U], cursor: ViewCursor<'view>, project: fn(&U) -> &T) -> Self {
        Self {
            values,
            cursor,
            project,
            marker: PhantomData,
        }
    }

    pub const fn len(&self) -> usize {
        self.values.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<T::View<'view>>
    where
        T: 'view,
    {
        self.values
            .get(index)
            .map(|value| (self.project)(value).__view(self.cursor.child(index)))
    }

    pub fn iter(&self) -> RepeatedIter<'view, T, U>
    where
        T: 'view,
    {
        RepeatedIter {
            values: self.values.iter(),
            cursor: self.cursor,
            project: self.project,
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
    values: std::slice::Iter<'view, U>,
    cursor: ViewCursor<'view>,
    project: fn(&U) -> &T,
    index: usize,
    marker: PhantomData<&'view T>,
}

impl<'view, T: Node + 'view, U> Iterator for RepeatedIter<'view, T, U> {
    type Item = T::View<'view>;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.values.next()?;
        let index = self.index;
        self.index += 1;
        Some((self.project)(value).__view(self.cursor.child(index)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl<T: Node, U> ExactSizeIterator for RepeatedIter<'_, T, U> {}
impl<T: Node, U> std::iter::FusedIterator for RepeatedIter<'_, T, U> {}

impl<'view> Elements<'view> {
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
    fn is_captured(&self) -> bool {
        self.span.is_some() || self.children.iter().any(Self::is_captured)
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
            Origin::Captured { source, .. } => Some(*source),
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

/// Common protocol implemented by generated concrete Node occurrence views.
pub trait NodeView: HasSpan {
    type Value: ?Sized;

    /// The semantic value at this occurrence.
    fn value(&self) -> &Self::Value;

    /// Active grammar elements in authored source order.
    fn span_elements(&self) -> Elements<'_> {
        Elements::empty()
    }
}

mod node_seal {
    pub trait Sealed {}
}

/// Associates a semantic Node with its generated concrete occurrence view.
///
/// Implementations are supplied by matching Recursa generated code through
/// the doc-hidden ABI; grammar authors do not implement this trait directly.
pub trait Node: node_seal::Sealed {
    type View<'view>: NodeView<Value = Self>
    where
        Self: 'view;

    #[doc(hidden)]
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
    type View<'view>: NodeView<Value = Self>
    where
        Self: 'view;

    fn view<'view>(&'view self, cursor: ViewCursor<'view>) -> Self::View<'view>;
}

/// Opaque occurrence cursor passed to generated concrete views.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ViewCursor<'view> {
    occurrence: Option<&'view OccurrenceNode>,
    source: Option<&'view str>,
}

impl<'view> ViewCursor<'view> {
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
        Some(crate::pretty::PrettyProvenance::new(text, children))
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
pub struct ContentText<'input>(ContentTextStorage<'input>);

#[derive(Clone)]
enum ContentTextStorage<'input> {
    Matched(&'input str),
    Owned(Box<str>),
}

impl<'input> ContentText<'input> {
    /// Stores synthesized content independently of any caller-owned buffer.
    pub fn owned(text: Box<str>) -> Self {
        Self(ContentTextStorage::Owned(text))
    }

    fn matched(source: &'input str, span: Span) -> Option<Self> {
        source
            .get(span.range())
            .map(|text| Self(ContentTextStorage::Matched(text)))
    }

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
    Off,
    Optional,
    Mandatory,
}

#[derive(Clone, Copy)]
enum CaptureRequest {
    Default,
    Spanned,
}

impl CapturePolicy {
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

/// Lazy ordinal accumulator for runtime-sized child occurrence sequences.
#[doc(hidden)]
pub struct Occurrences {
    leading_absent: usize,
    retained: Vec<Occurrence>,
}

impl Occurrences {
    pub const fn new() -> Self {
        Self {
            leading_absent: 0,
            retained: Vec::new(),
        }
    }

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
    value: T,
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

struct ProvenanceCollector;

impl ProvenanceCollector {
    fn finish(
        &self,
        source: &str,
        policy: CapturePolicy,
        request: CaptureRequest,
        span: Option<Span>,
        children: impl IntoIterator<Item = Occurrence>,
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
                        }));
                    }
                    retained.push(node);
                }
                None if retained.is_empty() => leading_absent += 1,
                None => retained.push(OccurrenceNode {
                    span: None,
                    children: Box::new([]),
                }),
            }
        }
        if span.is_none() && retained.is_empty() {
            return Ok(Occurrence::absent());
        }
        if span.is_some() && retained.is_empty() && leading_absent != 0 {
            retained.extend((0..leading_absent).map(|_| OccurrenceNode {
                span: None,
                children: Box::new([]),
            }));
        }
        Ok(Occurrence {
            node: Some(OccurrenceNode {
                span,
                children: retained.into_boxed_slice(),
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
    input: &'context mut Input<'input, A>,
    follow: &'context FollowSet,
    policy: CapturePolicy,
    request: CaptureRequest,
    provenance: ProvenanceCollector,
}

impl<'context, 'input, A> ParseContext<'context, 'input, A> {
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

    pub fn source(&self) -> &'input str {
        self.input.source()
    }

    pub fn grammar(&self) -> &'static GrammarInfo {
        self.input.grammar()
    }

    pub fn builder(&mut self) -> &mut A {
        self.input.builder()
    }

    pub fn peek(&self, distance: usize) -> Option<TokenRecord> {
        self.input.peek(distance)
    }

    pub fn advance(&mut self) -> Option<TokenRecord> {
        self.input.advance()
    }

    pub fn cursor(&self) -> usize {
        self.input.cursor()
    }

    pub fn consumed_span_from(&self, start: usize) -> Option<Span> {
        let end = self.input.cursor();
        let first = self.input.record(start)?;
        let last = self.input.record(end.checked_sub(1)?)?;
        Span::new(first.span.start() as usize, last.span.end() as usize).ok()
    }

    pub fn follow(&self) -> &FollowSet {
        self.follow
    }

    pub const fn capture_policy(&self) -> CapturePolicy {
        self.policy
    }

    pub fn unexpected(&self, expected: &FollowSet) -> ParseError {
        ParseError::unexpected(self.input, expected)
    }

    pub fn unexpected_at(&self, distance: usize, expected: &FollowSet) -> ParseError {
        ParseError::unexpected_at(self.input, expected, distance)
    }

    /// Reports the dynamic D3 ambiguity at the current Pratt extender.
    pub fn pratt_ambiguity(&self, expected: &FollowSet) -> ParseError {
        ParseError::pratt_ambiguity(self.input, expected)
    }

    pub fn next_is(&self, expected: &FollowSet) -> bool {
        self.peek(0)
            .is_some_and(|record| expected.contains(record.kind))
    }

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

    pub fn matched_text(&self, record: TokenRecord) -> Result<ContentText<'input>, ParseError> {
        ContentText::matched(self.input.source(), record.span).ok_or_else(|| {
            ParseError::invalid_token_span(self.input, record.span, Some(record.kind))
        })
    }

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
    const CAPTURE_POLICY: CapturePolicy;

    fn grammar() -> &'static GrammarInfo;

    fn root_follow() -> &'static FollowSet;

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
                },
                TokenRecord {
                    kind: WORD,
                    span: Span::new(2, 3).unwrap(),
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
