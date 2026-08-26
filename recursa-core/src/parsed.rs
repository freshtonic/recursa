use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::{FollowSet, GrammarInfo, HasSpan, Input, ParseError, Span, TokenRecord};

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
            Origin::Captured { provenance, .. } => ViewCursor {
                occurrence: Some(&provenance.root),
            },
        })
    }
}

/// Common protocol implemented by generated concrete Node occurrence views.
pub trait NodeView: HasSpan {
    type Value: ?Sized;

    /// The semantic value at this occurrence.
    fn value(&self) -> &Self::Value;
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
}

impl ViewCursor<'_> {
    fn absent() -> Self {
        Self { occurrence: None }
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
    node: OccurrenceNode,
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
        span: Span,
        children: impl IntoIterator<Item = Occurrence>,
    ) -> Result<Occurrence, ()> {
        source.get(span.range()).ok_or(())?;
        let span = policy.captures(request).then_some(span);
        let children = children
            .into_iter()
            .map(|occurrence| occurrence.node)
            .collect::<Vec<_>>();
        let retain_children = span.is_some() || children.iter().any(OccurrenceNode::is_captured);
        Ok(Occurrence {
            node: OccurrenceNode {
                span,
                children: if retain_children {
                    children.into_boxed_slice()
                } else {
                    Box::new([])
                },
            },
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

    pub fn follow(&self) -> &FollowSet {
        self.follow
    }

    pub fn unexpected(&self, expected: &FollowSet) -> ParseError {
        ParseError::unexpected(self.input, expected)
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
                span,
                std::iter::empty(),
            )
            .map_err(|()| ParseError::invalid_token_span(self.input, span, None))?;
        Ok(ParsedNode { value, occurrence })
    }

    /// Completes a parent while retaining successful child occurrences.
    pub fn finish_with_children<T>(
        &self,
        value: T,
        span: Span,
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
            .map_err(|()| ParseError::invalid_token_span(self.input, span, None))?;
        Ok(ParsedNode { value, occurrence })
    }

    /// Invokes a generated child parser with its own FOLLOW and capture policy.
    pub fn parse_node<T>(
        &mut self,
        follow: &FollowSet,
        policy: CapturePolicy,
    ) -> Result<ParsedNode<T>, ParseError>
    where
        T: Parse<'input, A>,
    {
        if !std::ptr::eq(self.input.grammar(), T::grammar()) {
            return Err(ParseError::grammar_mismatch(self.input));
        }
        let mut child = ParseContext::new(&mut *self.input, follow, policy, self.request);
        T::parse(&mut child)
    }
}

/// Matching-version recursive parsing protocol implemented by generated code.
#[doc(hidden)]
pub trait Parse<'input, A = ()>: Node + Sized {
    const CAPTURE_POLICY: CapturePolicy;

    fn grammar() -> &'static GrammarInfo;

    fn root_follow() -> &'static FollowSet;

    fn parse(context: &mut ParseContext<'_, 'input, A>) -> Result<ParsedNode<Self>, ParseError>;
}

/// Runs the generated root parser with the default capture request.
#[doc(hidden)]
pub fn parse<'input, T, A>(input: &mut Input<'input, A>) -> Result<Parsed<'input, T>, ParseError>
where
    T: Parse<'input, A>,
{
    parse_with_request::<T, A>(input, CaptureRequest::Default)
}

/// Runs the generated root parser with the explicit span-capture request.
#[doc(hidden)]
pub fn parse_spanned<'input, T, A>(
    input: &mut Input<'input, A>,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: Parse<'input, A>,
{
    parse_with_request::<T, A>(input, CaptureRequest::Spanned)
}

fn parse_with_request<'input, T, A>(
    input: &mut Input<'input, A>,
    request: CaptureRequest,
) -> Result<Parsed<'input, T>, ParseError>
where
    T: Parse<'input, A>,
{
    if !std::ptr::eq(input.grammar(), T::grammar()) {
        return Err(ParseError::grammar_mismatch(input));
    }
    let source = input.source();
    let mut context = ParseContext::new(input, T::root_follow(), T::CAPTURE_POLICY, request);
    let ParsedNode { value, occurrence } = T::parse(&mut context)?;
    let origin = if occurrence.node.is_captured() {
        Origin::Captured {
            source,
            provenance: Box::new(Provenance {
                root: occurrence.node,
            }),
        }
    } else {
        Origin::Uncaptured
    };
    Ok(Parsed { ast: value, origin })
}
