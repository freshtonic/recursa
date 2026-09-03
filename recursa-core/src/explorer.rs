//! Grammar-erased, allocation-bounded explorer contracts.

use crate::input::TokenRecord;
use crate::parsed::GeneratedParse;
use crate::parsed::ViewCursor;
use crate::{
    ElementRole, GrammarInfo, Input, ParseErrorKind, Parsed, PrettyConfig, SourceMap, Span,
};
use std::fmt;

/// Grammar-local lexer producing the runtime records strict parsing consumes.
#[doc(hidden)]
pub type RawLexFn = for<'input> fn(&'input str) -> crate::input::LexResult<'input>;

/// Erased source-borrowing strict parse shared by every root family.
#[doc(hidden)]
pub type RootParseFn =
    for<'input> fn(RawLexFn, &'input str) -> Result<ExplorerDocument<'input>, ParseFailure<'input>>;

/// Erased deterministic sample callback for one root family.
#[doc(hidden)]
pub type SampleFn = fn(&[u8]) -> Result<ExplorerDocument<'static>, SampleError>;

/// Grammar-erased schema, source operations, and optional sampling for one root family.
#[derive(Clone, Copy)]
pub struct Adapter {
    /// Static schema from the same Generation as every callback.
    schema: GrammarSchemaData,
    /// Feature-selected operations available for this root.
    capabilities: Capabilities,
    /// Grammar-erased lexer callback borrowing its input.
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    /// Grammar-local lexer handed to the shared strict parse.
    raw_lex: RawLexFn,
    /// Shared strict parse instantiated for this root family.
    parse: RootParseFn,
    /// Optional static sample callback when `Arbitrary` is configured.
    sample: Option<SampleFn>,
}

impl Adapter {
    /// Returns the static schema for this grammar-root application.
    pub const fn schema(self) -> GrammarSchema {
        GrammarSchema { data: self.schema }
    }

    /// Returns the feature-selected operations supported by this adapter.
    pub const fn capabilities(self) -> Capabilities {
        self.capabilities
    }

    /// Runs the generated lexer and retains typed static token descriptors.
    pub fn lex(self, source: &str) -> LexResult<'_> {
        (self.lex)(source)
    }

    /// Parses a complete source into an explorer document with occurrence spans.
    ///
    /// Lexical errors, syntax errors, and trailing input return structured
    /// diagnostics without a partial explorer document.
    pub fn parse(self, source: &str) -> Result<ExplorerDocument<'_>, ParseFailure<'_>> {
        (self.parse)(self.raw_lex, source)
    }

    /// Produces a deterministic synthetic document from caller-supplied bytes.
    /// `None` means this root does not configure `Arbitrary`.
    pub fn sample(self, bytes: &[u8]) -> Option<Result<ExplorerDocument<'static>, SampleError>> {
        self.sample.map(|sample| sample(bytes))
    }
}

/// Stable classification of a failed explorer parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseDiagnosticKind {
    /// One source range was rejected by the generated lexer.
    LexicalError,
    /// A concrete token cannot begin any valid continuation.
    UnexpectedToken,
    /// Required syntax remains after the final token.
    UnexpectedEof,
    /// A Pratt extender is also admitted by the caller's FOLLOW set.
    PrattAmbiguity,
    /// A generated token record contains an invalid source range.
    InvalidTokenSpan,
    /// Parser input belongs to different grammar metadata.
    GrammarMismatch,
    /// A complete root was followed by additional input.
    TrailingInput,
}

/// One structured lexical or syntax diagnostic from [`Adapter::parse`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseDiagnostic<'input> {
    /// Stable failure classification.
    kind: ParseDiagnosticKind,
    /// Concrete or zero-width source location.
    span: Span,
    /// Exact rejected source text, empty at EOF or for an invalid generated span.
    text: &'input str,
    /// Grammar-local display name of the found token.
    found: Option<&'static str>,
    /// Grammar-local expected atoms in analysis order.
    expected: Box<[&'static str]>,
    /// Display explanation safe for an explorer consumer.
    message: Box<str>,
}

impl<'input> ParseDiagnostic<'input> {
    /// Returns the stable failure classification.
    pub const fn kind(&self) -> ParseDiagnosticKind {
        self.kind
    }

    /// Returns the concrete or zero-width source location.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the exact rejected source text, or an empty string when no valid range exists.
    pub const fn text(&self) -> &'input str {
        self.text
    }

    /// Returns the grammar-local found token name, when one exists.
    pub const fn found(&self) -> Option<&'static str> {
        self.found
    }

    /// Iterates grammar-local expected atoms in deterministic analysis order.
    pub fn expected(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.expected.iter().copied()
    }

    /// Returns the display explanation for this diagnostic.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseDiagnostic<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Strict explorer parse failure retaining the borrowed source and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure<'input> {
    /// Complete input shared by every diagnostic span.
    source: &'input str,
    /// Nonempty diagnostics in deterministic source order.
    diagnostics: Box<[ParseDiagnostic<'input>]>,
}

impl<'input> ParseFailure<'input> {
    /// Returns the complete source that failed to parse.
    pub const fn source(&self) -> &'input str {
        self.source
    }

    /// Returns the nonempty lexical or syntax diagnostics.
    pub fn diagnostics(&self) -> &[ParseDiagnostic<'input>] {
        &self.diagnostics
    }
}

/// Converts one transactional generated parse failure to the erased diagnostic contract.
pub(crate) fn parse_failure_from_error<'input>(
    source: &'input str,
    error: crate::ParseError,
) -> ParseFailure<'input> {
    let span = error.span();
    let kind = match error.kind() {
        ParseErrorKind::UnexpectedToken => ParseDiagnosticKind::UnexpectedToken,
        ParseErrorKind::UnexpectedEof => ParseDiagnosticKind::UnexpectedEof,
        ParseErrorKind::PrattAmbiguity => ParseDiagnosticKind::PrattAmbiguity,
        ParseErrorKind::LexicalError => ParseDiagnosticKind::LexicalError,
        ParseErrorKind::InvalidTokenSpan => ParseDiagnosticKind::InvalidTokenSpan,
        ParseErrorKind::GrammarMismatch => ParseDiagnosticKind::GrammarMismatch,
    };
    ParseFailure {
        source,
        diagnostics: vec![ParseDiagnostic {
            kind,
            span,
            text: source.get(span.range()).unwrap_or(""),
            found: error.found(),
            expected: error.expected().collect(),
            message: error.to_string().into(),
        }]
        .into_boxed_slice(),
    }
}

impl fmt::Display for ParseFailure<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.diagnostics.as_ref() {
            [diagnostic] => diagnostic.fmt(formatter),
            diagnostics => write!(
                formatter,
                "{} explorer parse diagnostics",
                diagnostics.len()
            ),
        }
    }
}

impl std::error::Error for ParseFailure<'_> {}

/// Optional operations configured for an Explorer root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    /// Whether the root can render structured Pretty output.
    pretty: bool,
    /// Whether the root can generate deterministic samples.
    arbitrary: bool,
}

impl Capabilities {
    /// Reports whether Pretty rendering is available.
    pub const fn pretty(self) -> bool {
        self.pretty
    }

    /// Reports whether deterministic sample generation is available.
    pub const fn arbitrary(self) -> bool {
        self.arbitrary
    }
}

/// Failure produced while constructing a deterministic Explorer sample.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SampleError {
    /// Generator- or trait-provided explanation safe for display.
    message: Box<str>,
}

impl SampleError {
    /// Returns the display explanation for this sample failure.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SampleError {}

/// One grammar-erased lexical token record.
#[derive(Clone, Copy)]
pub struct Token<'input> {
    /// Complete source from which exact matched text is borrowed.
    source: &'input str,
    /// Static schema and checked matched span.
    data: LexRecordData,
}

impl<'input> Token<'input> {
    /// Returns the static token descriptor for this record.
    pub const fn schema(self) -> TokenSchema {
        TokenSchema {
            data: self.data.schema,
        }
    }

    /// Returns the exact matched source span.
    pub const fn span(self) -> Span {
        self.data.span
    }

    /// Borrows the exact matched input text.
    pub fn text(self) -> &'input str {
        self.source
            .get(self.data.span.range())
            .expect("generated explorer lex spans are UTF-8 ranges")
    }
}

/// One rejected lexical source range.
#[derive(Clone, Copy)]
pub struct LexError<'input> {
    /// Complete source from which rejected text is borrowed.
    source: &'input str,
    /// Exact rejected UTF-8 byte range.
    span: Span,
}

impl<'input> LexError<'input> {
    /// Returns the rejected source span.
    pub const fn span(self) -> Span {
        self.span
    }

    /// Borrows the exact rejected source text.
    pub fn text(self) -> &'input str {
        self.source
            .get(self.span.range())
            .expect("generated explorer lex spans are UTF-8 ranges")
    }
}

/// Grammar-erased lexical output for Explorer consumers.
pub struct LexResult<'input> {
    /// Complete source retained for token and error text.
    source: &'input str,
    /// Successful records in source order.
    tokens: Box<[LexRecordData]>,
    /// Rejected spans in source order.
    errors: Box<[Span]>,
}

impl<'input> LexResult<'input> {
    /// Returns the complete input source.
    pub const fn source(&self) -> &'input str {
        self.source
    }

    /// Iterates over successful lexical records in source order.
    pub fn tokens(&self) -> Tokens<'_, 'input> {
        Tokens {
            source: self.source,
            remaining: &self.tokens,
        }
    }

    /// Iterates over rejected source ranges in source order.
    pub fn errors(&self) -> LexErrors<'_, 'input> {
        LexErrors {
            source: self.source,
            remaining: &self.errors,
        }
    }
}

/// Exact-size iterator over grammar-erased lexical tokens.
pub struct Tokens<'records, 'input> {
    /// Source shared by every yielded token.
    source: &'input str,
    /// Records not yet yielded.
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

/// Exact-size iterator over lexical errors.
pub struct LexErrors<'records, 'input> {
    /// Source shared by every yielded error.
    source: &'input str,
    /// Error spans not yet yielded.
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

/// Origin of an Explorer document and its span availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provenance {
    /// Produced by parsing borrowed source with occurrence spans.
    Parsed,
    /// Constructed without source by a configured generator.
    Synthetic,
}

/// One owned Pretty result and its token-level source provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrettyOutput {
    /// Fully rendered source text.
    pub rendered: String,
    /// Token-level correlations from rendered ranges to parsed source.
    pub source_map: SourceMap,
}

/// Read-only, grammar-erased ownership of a parsed or synthetic root.
///
/// Construction performs one erased storage allocation. All occurrence
/// handles and iterators are lazy and allocation-free.
pub struct ExplorerDocument<'input> {
    /// One allocation owning the typed parsed root behind an erased protocol.
    storage: Box<dyn ErasedDocument + 'input>,
}

impl<'input> ExplorerDocument<'input> {
    /// Erases a complete parsed or synthetic root while retaining ownership.
    pub fn from_parsed<T>(parsed: Parsed<'input, T>) -> Self
    where
        T: ExplorerRoot + 'input,
    {
        Self {
            storage: Box::new(TypedDocument { parsed }),
        }
    }

    /// Returns whether this document was parsed or synthesized.
    pub fn provenance(&self) -> Provenance {
        self.storage.provenance()
    }

    /// Returns the complete borrowed source when this document was parsed.
    pub fn source(&self) -> Option<&str> {
        self.storage.source()
    }

    /// Returns a lazy handle to the root occurrence.
    pub fn root(&self) -> Node<'_> {
        Node {
            data: self.storage.root(),
        }
    }

    /// Returns `None` when this root does not configure `Pretty`.
    pub fn pretty(&self, config: &PrettyConfig) -> Option<PrettyOutput> {
        self.storage.pretty(config)
    }
}

/// Object-safe operations needed to inspect one owned typed document.
trait ErasedDocument {
    /// Reports whether the erased root was parsed or synthesized.
    fn provenance(&self) -> Provenance;
    /// Returns parsed source when available.
    fn source(&self) -> Option<&str>;
    /// Constructs a lazy erased root occurrence.
    fn root(&self) -> NodeData<'_>;
    /// Renders Pretty output when the root configured that capability.
    fn pretty(&self, config: &PrettyConfig) -> Option<PrettyOutput>;
}

/// Typed backing allocation retained behind [`ExplorerDocument`].
struct TypedDocument<'input, T> {
    /// Complete parsed root and its occurrence provenance.
    parsed: Parsed<'input, T>,
}

impl<T: ExplorerRoot> ErasedDocument for TypedDocument<'_, T> {
    fn provenance(&self) -> Provenance {
        if self.parsed.explorer_is_synthetic() {
            Provenance::Synthetic
        } else {
            Provenance::Parsed
        }
    }

    fn source(&self) -> Option<&str> {
        self.parsed.source()
    }

    fn root(&self) -> NodeData<'_> {
        let (value, cursor) = self.parsed.explorer_parts();
        value.__recursa_explorer_node(cursor)
    }

    fn pretty(&self, config: &PrettyConfig) -> Option<PrettyOutput> {
        T::__recursa_explorer_pretty(&self.parsed, config)
    }
}

/// Static schema for one generated grammar-root application.
#[derive(Clone, Copy)]
pub struct GrammarSchema {
    /// Matching generated static descriptor.
    data: GrammarSchemaData,
}

impl GrammarSchema {
    /// Returns the configured grammar name.
    pub const fn name(self) -> &'static str {
        self.data.tables.name
    }

    /// Returns the schema of this adapter's root application.
    pub const fn root(self) -> NodeSchema {
        NodeSchema {
            data: self.data.root,
        }
    }

    /// Iterates over every materialized Node schema deterministically.
    pub fn nodes(self) -> NodeSchemas {
        NodeSchemas {
            remaining: self.data.tables.nodes,
        }
    }

    /// Iterates over every content and fixed token descriptor.
    pub fn tokens(self) -> TokenSchemas {
        TokenSchemas {
            remaining: self.data.tables.tokens,
        }
    }
}

/// Static descriptor for one materialized Node application.
#[derive(Clone, Copy)]
pub struct NodeSchema {
    /// Matching generated static descriptor.
    data: &'static NodeSchemaData,
}

impl NodeSchema {
    /// Returns the declaration's short Rust name.
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    /// Returns the grammar-qualified materialized application name.
    pub const fn qualified_name(self) -> &'static str {
        self.data.qualified_name
    }

    /// Iterates the elements of a struct-shaped Node.
    pub fn elements(self) -> ElementSchemas {
        ElementSchemas {
            remaining: self.data.elements,
        }
    }

    /// Iterates the variants of an enum-shaped Node.
    pub fn variants(self) -> VariantSchemas {
        VariantSchemas {
            remaining: self.data.variants,
        }
    }
}

/// Static descriptor for one enum variant.
#[derive(Clone, Copy)]
pub struct VariantSchema {
    /// Matching generated static descriptor.
    data: &'static VariantSchemaData,
}

impl VariantSchema {
    /// Returns the authored variant name.
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    /// Returns the grammar-qualified variant name.
    pub const fn qualified_name(self) -> &'static str {
        self.data.qualified_name
    }

    /// Iterates this variant's source-order grammar elements.
    pub fn elements(self) -> ElementSchemas {
        ElementSchemas {
            remaining: self.data.elements,
        }
    }
}

/// Static descriptor for one source-order grammar element.
#[derive(Clone, Copy)]
pub struct ElementSchema {
    /// Matching generated static descriptor.
    data: &'static ElementSchemaData,
}

impl ElementSchema {
    /// Returns the authored field name or generated fixed-token label.
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    /// Returns whether this element is semantic or fixed syntax.
    pub const fn role(self) -> ElementRole {
        self.data.role
    }

    /// Returns the semantic occurrence cardinality.
    pub const fn cardinality(self) -> Cardinality {
        self.data.cardinality
    }

    /// Returns the semantic Node or content-token target, when present.
    pub const fn target(self) -> Option<ElementTarget> {
        match self.data.target {
            Some(ElementTargetData::Node(data)) => Some(ElementTarget::Node(NodeSchema { data })),
            Some(ElementTargetData::ContentToken(data)) => {
                Some(ElementTarget::ContentToken(TokenSchema { data }))
            }
            None => None,
        }
    }

    /// Token descriptor for fixed syntax and content-token fields.
    pub const fn token(self) -> Option<TokenSchema> {
        match self.data.token {
            Some(data) => Some(TokenSchema { data }),
            None => None,
        }
    }
}

/// Static semantic target of an Explorer element.
#[derive(Clone, Copy)]
pub enum ElementTarget {
    /// A materialized grammar Node application.
    Node(NodeSchema),
    /// A generated content-bearing token type.
    ContentToken(TokenSchema),
}

/// Static descriptor for one grammar-local token kind.
#[derive(Clone, Copy)]
pub struct TokenSchema {
    /// Matching generated static descriptor.
    data: &'static TokenSchemaData,
}

impl TokenSchema {
    /// Returns the short token descriptor name.
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    /// Returns the grammar-qualified token name.
    pub const fn qualified_name(self) -> &'static str {
        self.data.qualified_name
    }

    /// Returns the grammar-local kind display name.
    pub const fn kind(self) -> &'static str {
        self.data.kind
    }

    /// Canonical fixed spelling; content token kinds return `None`.
    pub const fn canonical_text(self) -> Option<&'static str> {
        self.data.canonical_text
    }

    /// Returns the stable CSS-neutral highlighting category.
    pub const fn highlight_class(self) -> HighlightClass {
        self.data.highlight_class
    }
}

/// Presentation-neutral lexical highlighting category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HighlightClass {
    /// Word-shaped fixed syntax.
    Keyword,
    /// Non-word fixed syntax.
    Punctuation,
    /// Identifier-like content.
    Identifier,
    /// Literal content such as strings or numbers.
    Literal,
    /// Other content-bearing tokens.
    Content,
    /// Ignored or non-semantic lexical trivia.
    Trivia,
}

/// Type-level semantic cardinality of one grammar element.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    /// Exactly one occurrence.
    One,
    /// Zero or one occurrence.
    Optional,
    /// Zero or more occurrences.
    ZeroOrMore,
    /// One or more occurrences.
    OneOrMore,
    /// An optional non-empty sequence.
    OptionalOneOrMore,
}

/// Lazy grammar-erased handle to one Node or content-token occurrence.
#[derive(Clone, Copy)]
pub struct Node<'document> {
    /// Erased value, schema, active shape, and occurrence cursor.
    data: NodeData<'document>,
}

impl<'document> Node<'document> {
    /// Returns the static schema for this Node or content-token occurrence.
    pub const fn schema(self) -> OccurrenceSchema {
        match self.data.schema {
            SchemaData::Node(data) => OccurrenceSchema::Node(NodeSchema { data }),
            SchemaData::ContentToken(data) => OccurrenceSchema::ContentToken(TokenSchema { data }),
        }
    }

    /// Returns the selected enum variant, if this is an enum Node.
    pub const fn active_variant(self) -> Option<VariantSchema> {
        match self.data.active_variant {
            Some(data) => Some(VariantSchema { data }),
            None => None,
        }
    }

    /// Returns this occurrence's parsed source span when available.
    pub fn span(self) -> Option<Span> {
        self.data.cursor.span()
    }

    /// Exact matched input for a content-token leaf, or its stored text when
    /// no input span is available. Grammar Node occurrences return `None`.
    pub fn token_text(self) -> Option<&'document str> {
        match self.data.schema {
            SchemaData::Node(_) => None,
            SchemaData::ContentToken(_) => Some(
                self.data
                    .cursor
                    .text()
                    .unwrap_or(self.data.stored_text.unwrap_or("")),
            ),
        }
    }

    /// Iterates the active shape's elements in source order.
    pub fn elements(self) -> Elements<'document> {
        let schemas = match (self.data.schema, self.data.active_variant) {
            (SchemaData::Node(_), Some(variant)) => variant.elements,
            (SchemaData::Node(schema), None) => schema.elements,
            (SchemaData::ContentToken(_), _) => &[],
        };
        Elements {
            node: self,
            schemas,
            index: 0,
        }
    }
}

/// Lazy handle to one element occurrence within a Node shape.
#[derive(Clone, Copy)]
pub struct Element<'document> {
    /// Parent Node occurrence owning this element.
    node: Node<'document>,
    /// Static element descriptor from the active shape.
    schema: &'static ElementSchemaData,
    /// Source-order element ordinal used by generated child access.
    index: usize,
    /// Provenance cursor for this element occurrence.
    cursor: ViewCursor<'document>,
}

impl<'document> Element<'document> {
    /// Returns this occurrence's static element descriptor.
    pub const fn schema(self) -> ElementSchema {
        ElementSchema { data: self.schema }
    }

    /// Returns the combined span of this element occurrence when parsed.
    pub fn span(self) -> Option<Span> {
        self.cursor.span()
    }

    /// Exact matched fixed-token text, or its canonical spelling if synthetic.
    pub fn text(self) -> Option<&'document str> {
        if self.schema.role != ElementRole::FixedToken {
            return None;
        }
        self.cursor
            .text()
            .or_else(|| self.schema.token.and_then(|token| token.canonical_text))
    }

    /// Lazily iterates semantic child occurrences in repetition order.
    pub fn children(self) -> Children<'document> {
        Children {
            element: self,
            index: 0,
            len: self
                .node
                .data
                .value
                .__recursa_explorer_child_count(self.index),
        }
    }
}

/// Typed static descriptor for a Node occurrence.
#[derive(Clone, Copy)]
pub enum OccurrenceSchema {
    /// A materialized grammar Node application.
    Node(NodeSchema),
    /// A generated content-bearing token type.
    ContentToken(TokenSchema),
}

/// Exact-size iterator over one active Node shape's elements.
pub struct Elements<'document> {
    /// Node occurrence whose generated body supplies children.
    node: Node<'document>,
    /// Active shape's static element descriptors.
    schemas: &'static [ElementSchemaData],
    /// Next source-order element ordinal.
    index: usize,
}

impl<'document> Iterator for Elements<'document> {
    type Item = Element<'document>;

    fn next(&mut self) -> Option<Self::Item> {
        let schema = self.schemas.get(self.index)?;
        let index = self.index;
        self.index += 1;
        Some(Element {
            node: self.node,
            schema,
            index,
            cursor: self.node.data.cursor.child(index),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.schemas.len().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Elements<'_> {}
impl std::iter::FusedIterator for Elements<'_> {}

/// Exact-size iterator over one semantic element's child occurrences.
pub struct Children<'document> {
    /// Element occurrence whose generated body supplies children.
    element: Element<'document>,
    /// Next repeated child ordinal.
    index: usize,
    /// Total child occurrences fixed when the iterator is constructed.
    len: usize,
}

impl<'document> Iterator for Children<'document> {
    type Item = Node<'document>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.len {
            return None;
        }
        let index = self.index;
        self.index += 1;
        let data = self.element.node.data.value.__recursa_explorer_child(
            self.element.node.data.cursor,
            self.element.index,
            index,
        )?;
        Some(Node { data })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Children<'_> {}
impl std::iter::FusedIterator for Children<'_> {}

/// Defines an exact-size iterator over generated static schema descriptors.
macro_rules! static_iterator {
    ($name:ident, $data:ty, $item:ty, $map:expr) => {
        #[doc = concat!("Iterator over generated `", stringify!($data), "` descriptors.")]
        pub struct $name {
            /// Static descriptors not yet yielded.
            remaining: &'static [&'static $data],
        }

        impl Iterator for $name {
            type Item = $item;

            fn next(&mut self) -> Option<Self::Item> {
                let (data, remaining) = self.remaining.split_first()?;
                self.remaining = remaining;
                Some(($map)(*data))
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                (self.remaining.len(), Some(self.remaining.len()))
            }
        }

        impl ExactSizeIterator for $name {}
        impl std::iter::FusedIterator for $name {}
    };
}

static_iterator!(NodeSchemas, NodeSchemaData, NodeSchema, |data| NodeSchema {
    data
});
static_iterator!(VariantSchemas, VariantSchemaData, VariantSchema, |data| {
    VariantSchema { data }
});
static_iterator!(TokenSchemas, TokenSchemaData, TokenSchema, |data| {
    TokenSchema { data }
});

/// Exact-size iterator over source-order element schemas.
pub struct ElementSchemas {
    /// Static descriptors not yet yielded.
    remaining: &'static [ElementSchemaData],
}

impl Iterator for ElementSchemas {
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

impl ExactSizeIterator for ElementSchemas {}
impl std::iter::FusedIterator for ElementSchemas {}

#[doc(hidden)]
/// Generated grammar-wide schema tables shared by every root adapter.
pub struct GrammarTablesData {
    /// Configured grammar name.
    name: &'static str,
    /// Deterministically ordered Node applications.
    nodes: &'static [&'static NodeSchemaData],
    /// Deterministically ordered token descriptors.
    tokens: &'static [&'static TokenSchemaData],
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Schema backing one [`GrammarSchema`] handle: the shared tables plus one root.
pub struct GrammarSchemaData {
    /// Grammar-wide tables emitted once per Generation.
    tables: &'static GrammarTablesData,
    /// Root materialized application.
    root: &'static NodeSchemaData,
}

#[doc(hidden)]
/// Generated static schema backing one [`NodeSchema`] handle.
pub struct NodeSchemaData {
    /// Short declaration name.
    name: &'static str,
    /// Grammar-qualified materialized application name.
    qualified_name: &'static str,
    /// Struct-shaped elements, empty for enum Nodes.
    elements: &'static [ElementSchemaData],
    /// Enum variants, empty for struct Nodes.
    variants: &'static [&'static VariantSchemaData],
}

#[doc(hidden)]
/// Generated static schema backing one [`VariantSchema`] handle.
pub struct VariantSchemaData {
    /// Authored variant name.
    name: &'static str,
    /// Grammar-qualified variant name.
    qualified_name: &'static str,
    /// Source-order elements for this variant.
    elements: &'static [ElementSchemaData],
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Generated static schema backing one [`ElementSchema`] handle.
pub struct ElementSchemaData {
    /// Authored field name or generated fixed-token label.
    name: &'static str,
    /// Semantic-field or fixed-token classification.
    role: ElementRole,
    /// Type-level semantic cardinality.
    cardinality: Cardinality,
    /// Semantic target, absent for fixed syntax.
    target: Option<ElementTargetData>,
    /// Token descriptor for fixed syntax and content-token fields.
    token: Option<&'static TokenSchemaData>,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Generated representation of an element's semantic target.
pub enum ElementTargetData {
    /// Target is another materialized Node application.
    Node(
        /// Static schema for the materialized Node application.
        &'static NodeSchemaData,
    ),
    /// Target is a content-bearing token type.
    ContentToken(
        /// Static schema for the content-bearing token type.
        &'static TokenSchemaData,
    ),
}

#[doc(hidden)]
/// Generated static schema backing one [`TokenSchema`] handle.
pub struct TokenSchemaData {
    /// Short token name.
    name: &'static str,
    /// Grammar-qualified token name.
    qualified_name: &'static str,
    /// Grammar-local token-kind display name.
    kind: &'static str,
    /// Canonical fixed spelling, absent for content tokens.
    canonical_text: Option<&'static str>,
    /// Presentation-neutral highlighting category.
    highlight_class: HighlightClass,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Generated erased lexical record validated against source.
pub struct LexRecordData {
    /// Static token descriptor for the matched kind.
    schema: &'static TokenSchemaData,
    /// Exact matched source range.
    span: Span,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Generated erased Node occurrence returned through the lazy protocol.
pub struct NodeData<'document> {
    /// Semantic value implementing the matching generated child protocol.
    value: &'document dyn ExplorerNode,
    /// Static Node or content-token schema.
    schema: SchemaData,
    /// Selected enum variant for Node occurrences.
    active_variant: Option<&'static VariantSchemaData>,
    /// Parsed-provenance cursor for this occurrence.
    cursor: ViewCursor<'document>,
    /// Content-token storage used when no parsed span exists.
    stored_text: Option<&'document str>,
}

/// Internal discriminant between Node and content-token occurrence schemas.
#[derive(Clone, Copy)]
enum SchemaData {
    /// Grammar Node schema.
    Node(&'static NodeSchemaData),
    /// Content-bearing token schema.
    ContentToken(&'static TokenSchemaData),
}

/// Private lazy protocol implemented by matching generated Node bodies.
///
/// # Safety
/// Implementations must return schema entries, child ordinals, cursors, and
/// semantic references from the same Generation and active Node shape.
#[doc(hidden)]
pub unsafe trait ExplorerNode {
    /// Returns this value's active schema, occurrence cursor, and erased value.
    fn __recursa_explorer_node<'document>(
        &'document self,
        cursor: ViewCursor<'document>,
    ) -> NodeData<'document>;

    /// Returns the number of semantic children for one active element ordinal.
    fn __recursa_explorer_child_count(&self, element: usize) -> usize;

    /// Returns one semantic child occurrence by element and repetition ordinal.
    fn __recursa_explorer_child<'document>(
        &'document self,
        cursor: ViewCursor<'document>,
        element: usize,
        child: usize,
    ) -> Option<NodeData<'document>>;
}

/// Private root protocol implemented only for generated source-visible roots.
///
/// # Safety
/// The Pretty rendering must belong to the same Generation as the Node.
#[doc(hidden)]
pub unsafe trait ExplorerRoot: ExplorerNode + Sized {
    /// Renders parsed or synthetic Pretty output; roots without `Pretty` report none.
    fn __recursa_explorer_pretty(
        parsed: &Parsed<'_, Self>,
        config: &PrettyConfig,
    ) -> Option<PrettyOutput> {
        let _ = (parsed, config);
        None
    }
}

/// Private root family naming one materialized root application for every input lifetime.
///
/// Generated code implements this for the `'static` spelling of a root type so the
/// shared generic root operations can be instantiated once per family instead of
/// being emitted once per Node.
///
/// # Safety
/// `Root` must be the same generated Node type for every lifetime.
#[doc(hidden)]
pub unsafe trait ExplorerRootFamily: 'static {
    /// The root Node type for one input lifetime.
    type Root<'input>: GeneratedParse<'input> + ExplorerRoot + 'input;
}

/// Tests one complete strict root against a generated static variant identity.
pub(crate) fn parsed_active_variant_is<T: ExplorerRoot>(
    parsed: &Parsed<'_, T>,
    expected: &'static VariantSchemaData,
) -> bool {
    let (value, cursor) = parsed.explorer_parts();
    value
        .__recursa_explorer_node(cursor)
        .active_variant
        .is_some_and(|active| std::ptr::eq(active, expected))
}

/// Reports whether one static variant belongs directly to the selected root.
pub(crate) fn root_contains_variant(
    root: &'static NodeSchemaData,
    variant: &'static VariantSchemaData,
) -> bool {
    root.variants
        .iter()
        .any(|candidate| std::ptr::eq(*candidate, variant))
}

#[doc(hidden)]
/// Constructs the generated grammar-wide schema tables under the matching protocol.
pub const fn grammar_tables(
    name: &'static str,
    nodes: &'static [&'static NodeSchemaData],
    tokens: &'static [&'static TokenSchemaData],
) -> GrammarTablesData {
    GrammarTablesData {
        name,
        nodes,
        tokens,
    }
}

#[doc(hidden)]
/// Constructs generated static Node schema under the matching protocol.
pub const fn node_schema(
    name: &'static str,
    qualified_name: &'static str,
    elements: &'static [ElementSchemaData],
    variants: &'static [&'static VariantSchemaData],
) -> NodeSchemaData {
    NodeSchemaData {
        name,
        qualified_name,
        elements,
        variants,
    }
}

#[doc(hidden)]
/// Constructs generated static variant schema under the matching protocol.
pub const fn variant_schema(
    name: &'static str,
    qualified_name: &'static str,
    elements: &'static [ElementSchemaData],
) -> VariantSchemaData {
    VariantSchemaData {
        name,
        qualified_name,
        elements,
    }
}

#[doc(hidden)]
/// Constructs generated static element schema under the matching protocol.
pub const fn element_schema(
    name: &'static str,
    role: ElementRole,
    cardinality: Cardinality,
    target: Option<ElementTargetData>,
    token: Option<&'static TokenSchemaData>,
) -> ElementSchemaData {
    ElementSchemaData {
        name,
        role,
        cardinality,
        target,
        token,
    }
}

#[doc(hidden)]
/// Wraps a Node schema as a generated element target.
pub const fn node_target(schema: &'static NodeSchemaData) -> ElementTargetData {
    ElementTargetData::Node(schema)
}

#[doc(hidden)]
/// Wraps a content-token schema as a generated element target.
pub const fn content_token_target(schema: &'static TokenSchemaData) -> ElementTargetData {
    ElementTargetData::ContentToken(schema)
}

#[doc(hidden)]
/// Constructs generated static token schema under the matching protocol.
pub const fn token_schema(
    name: &'static str,
    qualified_name: &'static str,
    kind: &'static str,
    canonical_text: Option<&'static str>,
    highlight_class: HighlightClass,
) -> TokenSchemaData {
    TokenSchemaData {
        name,
        qualified_name,
        kind,
        canonical_text,
        highlight_class,
    }
}

#[doc(hidden)]
/// Constructs a lazy grammar Node occurrence from matching generated parts.
pub fn node<'document>(
    value: &'document dyn ExplorerNode,
    schema: &'static NodeSchemaData,
    active_variant: Option<&'static VariantSchemaData>,
    cursor: ViewCursor<'document>,
) -> NodeData<'document> {
    NodeData {
        value,
        schema: SchemaData::Node(schema),
        active_variant,
        cursor,
        stored_text: None,
    }
}

#[doc(hidden)]
/// Preserves a child occurrence's inferred document lifetime across emission.
pub fn node_child(data: NodeData<'_>) -> NodeData<'_> {
    data
}

#[doc(hidden)]
/// Constructs a content-token leaf from matching generated parts.
pub fn token_node<'document>(
    value: &'document dyn ExplorerNode,
    schema: &'static TokenSchemaData,
    cursor: ViewCursor<'document>,
    stored_text: &'document str,
) -> NodeData<'document> {
    NodeData {
        value,
        schema: SchemaData::ContentToken(schema),
        active_variant: None,
        cursor,
        stored_text: Some(stored_text),
    }
}

#[doc(hidden)]
/// Converts the runtime Pretty tuple into Explorer-owned output.
pub fn pretty_output((rendered, source_map): (String, SourceMap)) -> PrettyOutput {
    PrettyOutput {
        rendered,
        source_map,
    }
}

#[doc(hidden)]
/// Constructs an erased deterministic sample failure.
pub fn sample_error(message: impl Into<Box<str>>) -> SampleError {
    SampleError {
        message: message.into(),
    }
}

#[doc(hidden)]
/// Constructs one generated erased lexical record.
pub const fn lex_record(schema: &'static TokenSchemaData, span: Span) -> LexRecordData {
    LexRecordData { schema, span }
}

#[doc(hidden)]
/// Collects generated lexical records and errors into Explorer output.
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

/// Lexes and strictly parses one root family's document.
///
/// One instantiation per family replaces one generated parse callback per Node.
/// The input lifetime stays late-bound, so the instantiated item coerces to
/// [`RootParseFn`].
#[doc(hidden)]
pub fn parse_root<'input, F: ExplorerRootFamily>(
    lex: RawLexFn,
    source: &'input str,
) -> Result<ExplorerDocument<'input>, ParseFailure<'input>> {
    let lexed = lex(source);
    let grammar = <F::Root<'input> as GeneratedParse<'input>>::grammar();
    let lexical_errors = lexed
        .records()
        .iter()
        .filter(|record| record.kind == grammar.lexical_error_kind)
        .map(|record| record.span)
        .collect::<Vec<_>>();
    let mut input = lexed.input(grammar);
    parse_document::<F::Root<'input>, ()>(&mut input, lexical_errors)
}

/// Produces one root family's deterministic synthetic document.
///
/// One instantiation per family replaces one generated sample callback per Node.
#[cfg(feature = "arbitrary")]
#[doc(hidden)]
pub fn sample_root<F: ExplorerRootFamily>(
    bytes: &[u8],
) -> Result<ExplorerDocument<'static>, SampleError>
where
    for<'bytes> F::Root<'static>: arbitrary::Arbitrary<'bytes>,
{
    let mut input = arbitrary::Unstructured::new(bytes);
    let value = <F::Root<'static> as arbitrary::Arbitrary<'_>>::arbitrary(&mut input)
        .map_err(|error| sample_error(error.to_string()))?;
    Ok(ExplorerDocument::from_parsed(Parsed::synthetic(value)))
}

/// Renders structured Pretty output for a root that configures `Pretty`.
#[doc(hidden)]
pub fn pretty_root<T: crate::Pretty>(
    parsed: &Parsed<'_, T>,
    config: &PrettyConfig,
) -> Option<PrettyOutput> {
    Some(pretty_output(crate::Pretty::render_mapped_with(
        parsed, *config,
    )))
}

/// Runs one generated root parser as a strict grammar-erased transaction.
///
/// Lexical errors take precedence and are all retained. A successful root
/// parse must also consume every token before its typed value is erased.
fn parse_document<'input, T, A>(
    input: &mut Input<'input, A>,
    lexical_errors: impl IntoIterator<Item = Span>,
) -> Result<ExplorerDocument<'input>, ParseFailure<'input>>
where
    T: GeneratedParse<'input, A> + ExplorerRoot + 'input,
{
    let source = input.source();
    let lexical_diagnostics = lexical_errors
        .into_iter()
        .map(|span| ParseDiagnostic {
            kind: ParseDiagnosticKind::LexicalError,
            span,
            text: source.get(span.range()).unwrap_or(""),
            found: None,
            expected: Box::new([]),
            message: "unrecognized input".into(),
        })
        .collect::<Vec<_>>();
    if !lexical_diagnostics.is_empty() {
        return Err(ParseFailure {
            source,
            diagnostics: lexical_diagnostics.into_boxed_slice(),
        });
    }

    let parsed = crate::parsed::parse_spanned::<T, A>(input)
        .map_err(|error| parse_failure_from_error(source, error))?;

    if let Some(record) = input.peek(0) {
        return Err(ParseFailure {
            source,
            diagnostics: vec![trailing_input_diagnostic(source, input.grammar(), record)]
                .into_boxed_slice(),
        });
    }

    Ok(ExplorerDocument::from_parsed(parsed))
}

/// Describes one significant token left after a complete strict root.
///
/// Explorer parsing and document framing share this conversion so a leftover
/// token reports the same classification, expectation, and message everywhere.
pub(crate) fn trailing_input_diagnostic<'input>(
    source: &'input str,
    grammar: &'static GrammarInfo,
    record: TokenRecord,
) -> ParseDiagnostic<'input> {
    let found = grammar.kind_name(record.kind);
    let message = found.map_or_else(
        || "unexpected trailing input".to_owned(),
        |name| format!("unexpected trailing {name}"),
    );
    ParseDiagnostic {
        kind: ParseDiagnosticKind::TrailingInput,
        span: record.span,
        text: source.get(record.span.range()).unwrap_or(""),
        found,
        expected: vec!["end of input"].into_boxed_slice(),
        message: message.into(),
    }
}

/// Constructs one generated grammar-erased root adapter.
///
/// # Safety
/// The tables, root schema, lexers, capability flags, parse, and sample function
/// must belong to the same Generation and materialized root application.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub const unsafe fn construct_adapter(
    tables: &'static GrammarTablesData,
    root: &'static NodeSchemaData,
    pretty: bool,
    arbitrary: bool,
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    raw_lex: RawLexFn,
    parse: RootParseFn,
    sample: Option<SampleFn>,
) -> Adapter {
    Adapter {
        schema: GrammarSchemaData { tables, root },
        capabilities: Capabilities { pretty, arbitrary },
        lex,
        raw_lex,
        parse,
        sample,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct CountingAllocator;

    std::thread_local! {
        static COUNTING: Cell<bool> = const { Cell::new(false) };
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            COUNTING.with(|counting| {
                if counting.get() {
                    ALLOCATIONS.with(|allocations| allocations.set(allocations.get() + 1));
                }
            });
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) }
        }
    }

    #[global_allocator]
    static TEST_ALLOCATOR: CountingAllocator = CountingAllocator;

    fn count_allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
        COUNTING.with(|counting| counting.set(false));
        ALLOCATIONS.with(|allocations| allocations.set(0));
        COUNTING.with(|counting| counting.set(true));
        let result = operation();
        COUNTING.with(|counting| counting.set(false));
        let allocations = ALLOCATIONS.with(Cell::get);
        (result, allocations)
    }

    static WORD: TokenSchemaData = token_schema(
        "Word",
        "grammar::Word",
        "WORD",
        None,
        HighlightClass::Identifier,
    );
    static CHILD_ELEMENTS: [ElementSchemaData; 0] = [];
    static CHILD: NodeSchemaData = node_schema("Child", "grammar::Child", &CHILD_ELEMENTS, &[]);
    static ROOT_ELEMENTS: [ElementSchemaData; 1] = [element_schema(
        "child",
        ElementRole::SemanticField,
        Cardinality::One,
        Some(node_target(&CHILD)),
        None,
    )];
    static ROOT: NodeSchemaData = node_schema("Root", "grammar::Root", &ROOT_ELEMENTS, &[]);
    static NODES: [&NodeSchemaData; 2] = [&ROOT, &CHILD];
    static TOKENS: [&TokenSchemaData; 1] = [&WORD];
    static TABLES: GrammarTablesData = grammar_tables("grammar", &NODES, &TOKENS);
    static GRAMMAR: GrammarSchemaData = GrammarSchemaData {
        tables: &TABLES,
        root: &ROOT,
    };

    struct Child;

    unsafe impl ExplorerNode for Child {
        fn __recursa_explorer_node<'document>(
            &'document self,
            cursor: ViewCursor<'document>,
        ) -> NodeData<'document> {
            node(self, &CHILD, None, cursor)
        }

        fn __recursa_explorer_child_count(&self, _: usize) -> usize {
            0
        }

        fn __recursa_explorer_child<'document>(
            &'document self,
            _: ViewCursor<'document>,
            _: usize,
            _: usize,
        ) -> Option<NodeData<'document>> {
            None
        }
    }

    struct Root {
        child: Child,
    }

    unsafe impl ExplorerNode for Root {
        fn __recursa_explorer_node<'document>(
            &'document self,
            cursor: ViewCursor<'document>,
        ) -> NodeData<'document> {
            node(self, &ROOT, None, cursor)
        }

        fn __recursa_explorer_child_count(&self, element: usize) -> usize {
            usize::from(element == 0)
        }

        fn __recursa_explorer_child<'document>(
            &'document self,
            cursor: ViewCursor<'document>,
            element: usize,
            child: usize,
        ) -> Option<NodeData<'document>> {
            (element == 0 && child == 0)
                .then(|| self.child.__recursa_explorer_node(cursor.child(0).child(0)))
        }
    }

    unsafe impl ExplorerRoot for Root {}

    #[test]
    fn synthetic_documents_walk_lazily_through_copy_handles() {
        fn copy<T: Copy>(value: T) -> T {
            value
        }
        fn exact_fused<I: ExactSizeIterator + std::iter::FusedIterator>(_: &I) {}

        let document = ExplorerDocument::from_parsed(Parsed::synthetic(Root { child: Child }));
        assert_eq!(document.provenance(), Provenance::Synthetic);
        assert!(document.source().is_none());
        assert!(document.pretty(&PrettyConfig::default()).is_none());

        let root = copy(document.root());
        let OccurrenceSchema::Node(schema) = root.schema() else {
            panic!("root is a Node schema")
        };
        assert_eq!(schema.qualified_name(), "grammar::Root");
        let elements = root.elements();
        exact_fused(&elements);
        let element = copy(elements.into_iter().next().expect("root child element"));
        assert_eq!(element.schema().name(), "child");
        assert_eq!(element.text(), None);
        let children = element.children();
        exact_fused(&children);
        assert_eq!(children.len(), 1);
        let child = children.into_iter().next().expect("child Node");
        let OccurrenceSchema::Node(schema) = child.schema() else {
            panic!("child is a Node schema")
        };
        assert_eq!(schema.name(), "Child");
        assert_eq!(child.elements().len(), 0);
    }

    #[test]
    fn static_schema_iterators_are_exact_size_and_fused() {
        fn exact_fused<I: ExactSizeIterator + std::iter::FusedIterator>(_: &I) {}

        let grammar = GrammarSchema { data: GRAMMAR };
        let nodes = grammar.nodes();
        let tokens = grammar.tokens();
        exact_fused(&nodes);
        exact_fused(&tokens);
        assert_eq!(nodes.len(), 2);
        assert_eq!(tokens.len(), 1);
        assert_eq!(grammar.root().qualified_name(), "grammar::Root");
        assert_eq!(tokens.into_iter().next().unwrap().kind(), "WORD");
    }

    #[test]
    fn adapters_report_unsupported_sampling_as_none() {
        fn lex(source: &str) -> LexResult<'_> {
            lex_result(source, [], [])
        }
        fn raw_lex(source: &str) -> crate::input::LexResult<'_> {
            crate::input::LexResult::new(source, Vec::new(), None)
        }
        fn parse(_: RawLexFn, source: &str) -> Result<ExplorerDocument<'_>, ParseFailure<'_>> {
            Err(ParseFailure {
                source,
                diagnostics: vec![ParseDiagnostic {
                    kind: ParseDiagnosticKind::UnexpectedEof,
                    span: Span::new(0, 0).unwrap(),
                    text: "",
                    found: None,
                    expected: Box::new([]),
                    message: "unexpected end of input".into(),
                }]
                .into_boxed_slice(),
            })
        }

        let adapter =
            unsafe { construct_adapter(&TABLES, &ROOT, false, false, lex, raw_lex, parse, None) };
        assert!(!adapter.capabilities().arbitrary());
        assert!(adapter.sample(&[]).is_none());
    }

    #[test]
    fn document_erasure_allocates_once_and_navigation_allocates_nothing() {
        // Warm the thread-local counters before measuring the erased Box.
        let _ = count_allocations(|| ());
        let (document, erasure_allocations) = count_allocations(|| {
            ExplorerDocument::from_parsed(Parsed::synthetic(Root { child: Child }))
        });
        assert_eq!(erasure_allocations, 1);

        let ((), navigation_allocations) = count_allocations(|| {
            for _ in 0..128 {
                let root = document.root();
                let mut elements = root.elements();
                let element = elements.next().expect("root element");
                assert!(elements.next().is_none());
                let mut children = element.children();
                let child = children.next().expect("child occurrence");
                assert_eq!(child.elements().len(), 0);
                assert!(children.next().is_none());
            }
        });
        assert_eq!(navigation_allocations, 0);
    }
}
