//! Grammar-erased explorer contracts backed by generated adapters.

use crate::parsed::ViewCursor;
use crate::{ElementRole, FollowSet, Input, ParseError, ParseErrorKind, Parsed, Span};
use std::fmt;

/// Grammar-erased parsing, lexing, rendering, and sampling entry point.
#[derive(Clone, Copy)]
pub struct Adapter {
    schema: &'static SchemaData,
    capabilities: Capabilities,
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    parse: for<'input> fn(&'input str) -> Result<Document<'input>, Error>,
    sample: Option<fn(u64) -> Result<Document<'static>, Error>>,
}

impl Adapter {
    /// Static schema for this adapter's root Node family.
    pub const fn schema(self) -> Schema {
        Schema { data: self.schema }
    }

    /// Operations implemented by this generated adapter.
    pub const fn capabilities(self) -> Capabilities {
        self.capabilities
    }

    /// Runs the grammar-erased lexer while retaining typed record classes.
    pub fn lex(self, source: &str) -> LexResult<'_> {
        (self.lex)(source)
    }

    /// Parses a provenance-capturing explorer document.
    pub fn parse(self, source: &str) -> Result<Document<'_>, Error> {
        (self.parse)(source)
    }

    /// Generates a deterministic synthetic sample when Arbitrary is configured.
    pub fn sample(self, seed: u64) -> Result<Document<'static>, Error> {
        self.sample.ok_or(Error::Unsupported(Operation::Sample))?(seed)
    }
}

/// Discoverable optional operations for one adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    pretty: bool,
    arbitrary: bool,
}

impl Capabilities {
    pub const fn pretty(self) -> bool {
        self.pretty
    }

    pub const fn arbitrary(self) -> bool {
        self.arbitrary
    }
}

/// One explorer operation that may be absent from a generated adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Pretty,
    Sample,
}

/// Structured explorer failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Parse(ParseFailure),
    Unsupported(Operation),
    Sample { message: Box<str> },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Unsupported(Operation::Pretty) => {
                formatter.write_str("this explorer root does not configure Pretty")
            }
            Self::Unsupported(Operation::Sample) => {
                formatter.write_str("this explorer root does not configure Arbitrary")
            }
            Self::Sample { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<ParseError> for Error {
    fn from(error: ParseError) -> Self {
        Self::Parse(ParseFailure {
            kind: error.kind(),
            span: error.span(),
            found: error.found(),
            expected: error.expected().collect::<Vec<_>>().into_boxed_slice(),
        })
    }
}

/// Grammar-independent parse error data retained by the explorer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    kind: ParseErrorKind,
    span: Span,
    found: Option<&'static str>,
    expected: Box<[&'static str]>,
}

impl ParseFailure {
    pub const fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn found(&self) -> Option<&'static str> {
        self.found
    }

    pub fn expected(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.expected.iter().copied()
    }
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} at {}..{}",
            self.kind,
            self.span.start(),
            self.span.end()
        )
    }
}

/// Origin of an explorer document and its occurrence provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provenance {
    Parsed,
    Synthetic,
}

/// Read-only erased representation of one parsed or generated root.
pub struct Document<'input> {
    source: Option<&'input str>,
    root: NodeData,
    pretty: Option<String>,
    provenance: Provenance,
}

impl<'input> Document<'input> {
    pub const fn source(&self) -> Option<&'input str> {
        self.source
    }

    pub const fn provenance(&self) -> Provenance {
        self.provenance
    }

    pub fn root(&self) -> Node<'_, 'input> {
        Node {
            data: &self.root,
            source: self.source,
        }
    }

    pub fn pretty(&self) -> Result<&str, Error> {
        self.pretty
            .as_deref()
            .ok_or(Error::Unsupported(Operation::Pretty))
    }

    /// Attaches generated Pretty output without exposing the document storage.
    #[doc(hidden)]
    pub fn __set_pretty(&mut self, rendered: String) {
        self.pretty = Some(rendered);
    }
}

/// Opaque static schema for one Node shape.
#[derive(Clone, Copy)]
pub struct Schema {
    data: &'static SchemaData,
}

impl Schema {
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    pub const fn variant(self) -> Option<&'static str> {
        self.data.variant
    }

    pub fn elements(self) -> SchemaElements {
        SchemaElements {
            remaining: self.data.elements,
        }
    }

    /// Every declared enum or Pratt variant, including inactive variants.
    pub fn variants(self) -> SchemaVariants {
        SchemaVariants {
            remaining: self.data.variants,
        }
    }
}

/// Opaque read-only handle to one Node or content-token occurrence.
#[derive(Clone, Copy)]
pub struct Node<'document, 'input> {
    data: &'document NodeData,
    source: Option<&'input str>,
}

impl<'document, 'input> Node<'document, 'input> {
    pub const fn schema(self) -> Schema {
        Schema {
            data: self.data.schema,
        }
    }

    pub const fn span(self) -> Option<Span> {
        self.data.span
    }

    /// Matched input when spanned, otherwise the generated static/owned fallback.
    pub fn display(self) -> Option<&'document str>
    where
        'input: 'document,
    {
        display_text(self.source, self.data.span, self.data.display.as_deref())
    }

    pub fn elements(self) -> Elements<'document, 'input> {
        Elements {
            remaining: &self.data.elements,
            source: self.source,
        }
    }
}

/// One active source-order grammar element occurrence.
#[derive(Clone, Copy)]
pub struct Element<'document, 'input> {
    data: &'document ElementData,
    source: Option<&'input str>,
}

impl<'document, 'input> Element<'document, 'input> {
    pub const fn schema(self) -> ElementSchema {
        ElementSchema {
            data: self.data.schema,
        }
    }

    pub const fn span(self) -> Option<Span> {
        self.data.span
    }

    pub fn display(self) -> Option<&'document str>
    where
        'input: 'document,
    {
        display_text(self.source, self.data.span, self.data.display.as_deref())
    }

    pub fn children(self) -> Nodes<'document, 'input> {
        Nodes {
            remaining: &self.data.children,
            source: self.source,
        }
    }
}

fn display_text<'document>(
    source: Option<&'document str>,
    span: Option<Span>,
    fallback: Option<&'document str>,
) -> Option<&'document str> {
    span.and_then(|span| source?.get(span.range())).or(fallback)
}

/// Static schema for one authored grammar element.
#[derive(Clone, Copy)]
pub struct ElementSchema {
    data: &'static ElementSchemaData,
}

impl ElementSchema {
    pub const fn name(self) -> Option<&'static str> {
        self.data.name
    }

    pub const fn role(self) -> ElementRole {
        self.data.role
    }

    pub const fn cardinality(self) -> Cardinality {
        self.data.cardinality
    }

    pub const fn token_classification(self) -> Option<TokenClassification> {
        self.data.token_classification
    }
}

/// Semantic cardinality of an authored element.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    One,
    Optional,
    ZeroOrMore,
    OneOrMore,
    OptionalOneOrMore,
}

/// Explorer-facing lexical classification without a grammar-local identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenClassification {
    Fixed,
    Content,
}

macro_rules! slice_iterator {
    ($name:ident, $item:ty, $map:expr) => {
        pub struct $name<'document, 'input> {
            remaining: &'document [$item],
            source: Option<&'input str>,
        }

        impl<'document, 'input> Iterator for $name<'document, 'input> {
            type Item = Element<'document, 'input>;

            fn next(&mut self) -> Option<Self::Item> {
                let (first, remaining) = self.remaining.split_first()?;
                self.remaining = remaining;
                Some(($map)(first, self.source))
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                (self.remaining.len(), Some(self.remaining.len()))
            }
        }

        impl ExactSizeIterator for $name<'_, '_> {}
        impl std::iter::FusedIterator for $name<'_, '_> {}
    };
}

slice_iterator!(Elements, ElementData, |data, source| Element {
    data,
    source
});

/// Exact-size fused iterator over child Node occurrences.
pub struct Nodes<'document, 'input> {
    remaining: &'document [NodeData],
    source: Option<&'input str>,
}

impl<'document, 'input> Iterator for Nodes<'document, 'input> {
    type Item = Node<'document, 'input>;

    fn next(&mut self) -> Option<Self::Item> {
        let (data, remaining) = self.remaining.split_first()?;
        self.remaining = remaining;
        Some(Node {
            data,
            source: self.source,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining.len(), Some(self.remaining.len()))
    }
}

impl ExactSizeIterator for Nodes<'_, '_> {}
impl std::iter::FusedIterator for Nodes<'_, '_> {}

/// Exact-size fused iterator over static element schemas.
pub struct SchemaElements {
    remaining: &'static [ElementSchemaData],
}

impl Iterator for SchemaElements {
    type Item = ElementSchema;

    fn next(&mut self) -> Option<Self::Item> {
        let (data, remaining) = self.remaining.split_first()?;
        self.remaining = remaining;
        Some(ElementSchema { data })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining.len(), Some(self.remaining.len()))
    }
}

impl ExactSizeIterator for SchemaElements {}
impl std::iter::FusedIterator for SchemaElements {}

/// Exact-size fused iterator over every declared variant schema.
pub struct SchemaVariants {
    remaining: &'static [&'static SchemaData],
}

impl Iterator for SchemaVariants {
    type Item = Schema;

    fn next(&mut self) -> Option<Self::Item> {
        let (data, remaining) = self.remaining.split_first()?;
        self.remaining = remaining;
        Some(Schema { data })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining.len(), Some(self.remaining.len()))
    }
}

impl ExactSizeIterator for SchemaVariants {}
impl std::iter::FusedIterator for SchemaVariants {}

/// Grammar-erased lexical record class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexicalClass {
    Fixed,
    Content,
}

#[derive(Clone, Copy)]
pub struct Token<'input> {
    source: &'input str,
    data: LexRecordData,
}

impl<'input> Token<'input> {
    pub const fn kind(self) -> &'static str {
        self.data.kind
    }

    pub const fn class(self) -> LexicalClass {
        self.data.class
    }

    pub const fn span(self) -> Span {
        self.data.span
    }

    pub fn text(self) -> &'input str {
        self.source
            .get(self.data.span.range())
            .expect("generated explorer lex spans are UTF-8 ranges")
    }
}

#[derive(Clone, Copy)]
pub struct LexError<'input> {
    source: &'input str,
    span: Span,
}

impl<'input> LexError<'input> {
    pub const fn span(self) -> Span {
        self.span
    }

    pub fn text(self) -> &'input str {
        self.source
            .get(self.span.range())
            .expect("generated explorer lex spans are UTF-8 ranges")
    }
}

pub struct LexResult<'input> {
    source: &'input str,
    tokens: Box<[LexRecordData]>,
    errors: Box<[Span]>,
}

impl<'input> LexResult<'input> {
    pub const fn source(&self) -> &'input str {
        self.source
    }

    pub fn tokens(&self) -> Tokens<'_, 'input> {
        Tokens {
            source: self.source,
            remaining: &self.tokens,
        }
    }

    pub fn errors(&self) -> LexErrors<'_, 'input> {
        LexErrors {
            source: self.source,
            remaining: &self.errors,
        }
    }
}

pub struct Tokens<'records, 'input> {
    source: &'input str,
    remaining: &'records [LexRecordData],
}

impl<'input> Iterator for Tokens<'_, 'input> {
    type Item = Token<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        let (data, remaining) = self.remaining.split_first()?;
        self.remaining = remaining;
        Some(Token {
            source: self.source,
            data: *data,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining.len(), Some(self.remaining.len()))
    }
}

impl ExactSizeIterator for Tokens<'_, '_> {}
impl std::iter::FusedIterator for Tokens<'_, '_> {}

pub struct LexErrors<'records, 'input> {
    source: &'input str,
    remaining: &'records [Span],
}

impl<'input> Iterator for LexErrors<'_, 'input> {
    type Item = LexError<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        let (span, remaining) = self.remaining.split_first()?;
        self.remaining = remaining;
        Some(LexError {
            source: self.source,
            span: *span,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining.len(), Some(self.remaining.len()))
    }
}

impl ExactSizeIterator for LexErrors<'_, '_> {}
impl std::iter::FusedIterator for LexErrors<'_, '_> {}

#[doc(hidden)]
pub struct SchemaData {
    name: &'static str,
    variant: Option<&'static str>,
    elements: &'static [ElementSchemaData],
    variants: &'static [&'static SchemaData],
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ElementSchemaData {
    name: Option<&'static str>,
    role: ElementRole,
    cardinality: Cardinality,
    token_classification: Option<TokenClassification>,
}

#[doc(hidden)]
pub struct NodeData {
    schema: &'static SchemaData,
    span: Option<Span>,
    display: Option<Box<str>>,
    elements: Box<[ElementData]>,
}

#[doc(hidden)]
pub struct ElementData {
    schema: &'static ElementSchemaData,
    span: Option<Span>,
    display: Option<Box<str>>,
    children: Box<[NodeData]>,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct LexRecordData {
    kind: &'static str,
    class: LexicalClass,
    span: Span,
}

/// Private protocol implemented by generated Node and content-token bodies.
///
/// # Safety
///
/// Implementations must preserve the active generated schema, source-order
/// element layout, and occurrence-cursor correlation for their Generation.
#[doc(hidden)]
pub unsafe trait ExplorerNode {
    fn __recursa_explorer_node(&self, cursor: ViewCursor<'_>) -> NodeData;
}

#[doc(hidden)]
pub const fn schema(
    name: &'static str,
    variant: Option<&'static str>,
    elements: &'static [ElementSchemaData],
    variants: &'static [&'static SchemaData],
) -> SchemaData {
    SchemaData {
        name,
        variant,
        elements,
        variants,
    }
}

#[doc(hidden)]
pub const fn element_schema(
    name: Option<&'static str>,
    role: ElementRole,
    cardinality: Cardinality,
    token_classification: Option<TokenClassification>,
) -> ElementSchemaData {
    ElementSchemaData {
        name,
        role,
        cardinality,
        token_classification,
    }
}

#[doc(hidden)]
pub fn node(
    schema: &'static SchemaData,
    span: Option<Span>,
    display: Option<Box<str>>,
    elements: impl IntoIterator<Item = ElementData>,
) -> NodeData {
    NodeData {
        schema,
        span,
        display,
        elements: elements.into_iter().collect(),
    }
}

#[doc(hidden)]
pub fn element(
    schema: &'static ElementSchemaData,
    cursor: ViewCursor<'_>,
    display: Option<Box<str>>,
    children: impl IntoIterator<Item = NodeData>,
) -> ElementData {
    ElementData {
        schema,
        span: cursor.span(),
        display,
        children: children.into_iter().collect(),
    }
}

#[doc(hidden)]
pub fn document<'input, T: ExplorerNode>(
    parsed: &Parsed<'input, T>,
    source: Option<&'input str>,
) -> Document<'input> {
    let (ast, cursor) = parsed.explorer_parts();
    Document {
        source,
        root: ast.__recursa_explorer_node(cursor),
        pretty: None,
        provenance: if source.is_some() {
            Provenance::Parsed
        } else {
            Provenance::Synthetic
        },
    }
}

#[doc(hidden)]
/// # Safety
///
/// Every function pointer and schema must belong to the same validated
/// Generation. Only matching Recursa-generated code may construct an adapter.
pub const unsafe fn construct_adapter(
    schema: &'static SchemaData,
    pretty: bool,
    arbitrary: bool,
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    parse: for<'input> fn(&'input str) -> Result<Document<'input>, Error>,
    sample: Option<fn(u64) -> Result<Document<'static>, Error>>,
) -> Adapter {
    Adapter {
        schema,
        capabilities: Capabilities { pretty, arbitrary },
        lex,
        parse,
        sample,
    }
}

#[doc(hidden)]
pub const fn lex_record(kind: &'static str, class: LexicalClass, span: Span) -> LexRecordData {
    LexRecordData { kind, class, span }
}

#[doc(hidden)]
pub fn lex_result<'input>(
    source: &'input str,
    tokens: impl IntoIterator<Item = LexRecordData>,
    errors: impl IntoIterator<Item = Span>,
) -> LexResult<'input> {
    LexResult {
        source,
        tokens: tokens.into_iter().collect(),
        errors: errors.into_iter().collect(),
    }
}

#[doc(hidden)]
pub fn require_eof<A>(input: &Input<'_, A>, expected: &FollowSet) -> Result<(), ParseError> {
    if input.is_eof() {
        Ok(())
    } else {
        Err(ParseError::unexpected(input, expected))
    }
}
