//! Grammar-erased, allocation-bounded explorer contracts.

use crate::parsed::ViewCursor;
use crate::{ElementRole, Parsed, PrettyConfig, SourceMap, Span};
use std::fmt;

type SampleFn = fn(&[u8]) -> Result<ExplorerDocument<'static>, SampleError>;

/// Grammar-erased schema and optional sample entry point for one root family.
#[derive(Clone, Copy)]
pub struct Adapter {
    schema: &'static GrammarSchemaData,
    capabilities: Capabilities,
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    sample: Option<SampleFn>,
}

impl Adapter {
    pub const fn schema(self) -> GrammarSchema {
        GrammarSchema { data: self.schema }
    }

    pub const fn capabilities(self) -> Capabilities {
        self.capabilities
    }

    /// Runs the generated lexer and retains typed static token descriptors.
    pub fn lex(self, source: &str) -> LexResult<'_> {
        (self.lex)(source)
    }

    /// Produces a deterministic synthetic document from caller-supplied bytes.
    /// `None` means this root does not configure `Arbitrary`.
    pub fn sample(self, bytes: &[u8]) -> Option<Result<ExplorerDocument<'static>, SampleError>> {
        self.sample.map(|sample| sample(bytes))
    }
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SampleError {
    message: Box<str>,
}

impl SampleError {
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
    source: &'input str,
    data: LexRecordData,
}

impl<'input> Token<'input> {
    pub const fn schema(self) -> TokenSchema {
        TokenSchema {
            data: self.data.schema,
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provenance {
    Parsed,
    Synthetic,
}

/// One owned Pretty result and its token-level source provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrettyOutput {
    pub rendered: String,
    pub source_map: SourceMap,
}

/// Read-only, grammar-erased ownership of a parsed or synthetic root.
///
/// Construction performs one erased storage allocation. All occurrence
/// handles and iterators are lazy and allocation-free.
pub struct ExplorerDocument<'input> {
    storage: Box<dyn ErasedDocument + 'input>,
}

impl<'input> ExplorerDocument<'input> {
    pub fn from_parsed<T>(parsed: Parsed<'input, T>) -> Self
    where
        T: ExplorerRoot + 'input,
    {
        Self {
            storage: Box::new(TypedDocument { parsed }),
        }
    }

    pub fn provenance(&self) -> Provenance {
        self.storage.provenance()
    }

    pub fn source(&self) -> Option<&str> {
        self.storage.source()
    }

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

trait ErasedDocument {
    fn provenance(&self) -> Provenance;
    fn source(&self) -> Option<&str>;
    fn root(&self) -> NodeData<'_>;
    fn pretty(&self, config: &PrettyConfig) -> Option<PrettyOutput>;
}

struct TypedDocument<'input, T> {
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

#[derive(Clone, Copy)]
pub struct GrammarSchema {
    data: &'static GrammarSchemaData,
}

impl GrammarSchema {
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    pub const fn root(self) -> NodeSchema {
        NodeSchema {
            data: self.data.root,
        }
    }

    pub fn nodes(self) -> NodeSchemas {
        NodeSchemas {
            remaining: self.data.nodes,
        }
    }

    pub fn tokens(self) -> TokenSchemas {
        TokenSchemas {
            remaining: self.data.tokens,
        }
    }
}

#[derive(Clone, Copy)]
pub struct NodeSchema {
    data: &'static NodeSchemaData,
}

impl NodeSchema {
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    pub const fn qualified_name(self) -> &'static str {
        self.data.qualified_name
    }

    pub fn elements(self) -> ElementSchemas {
        ElementSchemas {
            remaining: self.data.elements,
        }
    }

    pub fn variants(self) -> VariantSchemas {
        VariantSchemas {
            remaining: self.data.variants,
        }
    }
}

#[derive(Clone, Copy)]
pub struct VariantSchema {
    data: &'static VariantSchemaData,
}

impl VariantSchema {
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    pub const fn qualified_name(self) -> &'static str {
        self.data.qualified_name
    }

    pub fn elements(self) -> ElementSchemas {
        ElementSchemas {
            remaining: self.data.elements,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ElementSchema {
    data: &'static ElementSchemaData,
}

impl ElementSchema {
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    pub const fn role(self) -> ElementRole {
        self.data.role
    }

    pub const fn cardinality(self) -> Cardinality {
        self.data.cardinality
    }

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

#[derive(Clone, Copy)]
pub enum ElementTarget {
    Node(NodeSchema),
    ContentToken(TokenSchema),
}

#[derive(Clone, Copy)]
pub struct TokenSchema {
    data: &'static TokenSchemaData,
}

impl TokenSchema {
    pub const fn name(self) -> &'static str {
        self.data.name
    }

    pub const fn qualified_name(self) -> &'static str {
        self.data.qualified_name
    }

    pub const fn kind(self) -> &'static str {
        self.data.kind
    }

    /// Canonical fixed spelling; content token kinds return `None`.
    pub const fn canonical_text(self) -> Option<&'static str> {
        self.data.canonical_text
    }

    pub const fn highlight_class(self) -> HighlightClass {
        self.data.highlight_class
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HighlightClass {
    Keyword,
    Punctuation,
    Identifier,
    Literal,
    Content,
    Trivia,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    One,
    Optional,
    ZeroOrMore,
    OneOrMore,
    OptionalOneOrMore,
}

#[derive(Clone, Copy)]
pub struct Node<'document> {
    data: NodeData<'document>,
}

impl<'document> Node<'document> {
    pub const fn schema(self) -> OccurrenceSchema {
        match self.data.schema {
            SchemaData::Node(data) => OccurrenceSchema::Node(NodeSchema { data }),
            SchemaData::ContentToken(data) => OccurrenceSchema::ContentToken(TokenSchema { data }),
        }
    }

    pub const fn active_variant(self) -> Option<VariantSchema> {
        match self.data.active_variant {
            Some(data) => Some(VariantSchema { data }),
            None => None,
        }
    }

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

#[derive(Clone, Copy)]
pub struct Element<'document> {
    node: Node<'document>,
    schema: &'static ElementSchemaData,
    index: usize,
    cursor: ViewCursor<'document>,
}

impl<'document> Element<'document> {
    pub const fn schema(self) -> ElementSchema {
        ElementSchema { data: self.schema }
    }

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
    Node(NodeSchema),
    ContentToken(TokenSchema),
}

pub struct Elements<'document> {
    node: Node<'document>,
    schemas: &'static [ElementSchemaData],
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

pub struct Children<'document> {
    element: Element<'document>,
    index: usize,
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

macro_rules! static_iterator {
    ($name:ident, $data:ty, $item:ty, $map:expr) => {
        pub struct $name {
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

pub struct ElementSchemas {
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
pub struct GrammarSchemaData {
    name: &'static str,
    root: &'static NodeSchemaData,
    nodes: &'static [&'static NodeSchemaData],
    tokens: &'static [&'static TokenSchemaData],
}

#[doc(hidden)]
pub struct NodeSchemaData {
    name: &'static str,
    qualified_name: &'static str,
    elements: &'static [ElementSchemaData],
    variants: &'static [&'static VariantSchemaData],
}

#[doc(hidden)]
pub struct VariantSchemaData {
    name: &'static str,
    qualified_name: &'static str,
    elements: &'static [ElementSchemaData],
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ElementSchemaData {
    name: &'static str,
    role: ElementRole,
    cardinality: Cardinality,
    target: Option<ElementTargetData>,
    token: Option<&'static TokenSchemaData>,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum ElementTargetData {
    Node(&'static NodeSchemaData),
    ContentToken(&'static TokenSchemaData),
}

#[doc(hidden)]
pub struct TokenSchemaData {
    name: &'static str,
    qualified_name: &'static str,
    kind: &'static str,
    canonical_text: Option<&'static str>,
    highlight_class: HighlightClass,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct LexRecordData {
    schema: &'static TokenSchemaData,
    span: Span,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct NodeData<'document> {
    value: &'document dyn ExplorerNode,
    schema: SchemaData,
    active_variant: Option<&'static VariantSchemaData>,
    cursor: ViewCursor<'document>,
    stored_text: Option<&'document str>,
}

#[derive(Clone, Copy)]
enum SchemaData {
    Node(&'static NodeSchemaData),
    ContentToken(&'static TokenSchemaData),
}

/// Private lazy protocol implemented by matching generated Node bodies.
///
/// # Safety
/// Implementations must return schema entries, child ordinals, cursors, and
/// semantic references from the same Generation and active Node shape.
#[doc(hidden)]
pub unsafe trait ExplorerNode {
    fn __recursa_explorer_node<'document>(
        &'document self,
        cursor: ViewCursor<'document>,
    ) -> NodeData<'document>;

    fn __recursa_explorer_child_count(&self, element: usize) -> usize;

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
/// The schema and operation must belong to the same Generation as the Node.
#[doc(hidden)]
pub unsafe trait ExplorerRoot: ExplorerNode + Sized {
    fn __recursa_explorer_schema() -> &'static GrammarSchemaData;

    fn __recursa_explorer_pretty(
        parsed: &Parsed<'_, Self>,
        config: &PrettyConfig,
    ) -> Option<PrettyOutput>;
}

#[doc(hidden)]
pub const fn grammar_schema(
    name: &'static str,
    root: &'static NodeSchemaData,
    nodes: &'static [&'static NodeSchemaData],
    tokens: &'static [&'static TokenSchemaData],
) -> GrammarSchemaData {
    GrammarSchemaData {
        name,
        root,
        nodes,
        tokens,
    }
}

#[doc(hidden)]
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
pub const fn node_target(schema: &'static NodeSchemaData) -> ElementTargetData {
    ElementTargetData::Node(schema)
}

#[doc(hidden)]
pub const fn content_token_target(schema: &'static TokenSchemaData) -> ElementTargetData {
    ElementTargetData::ContentToken(schema)
}

#[doc(hidden)]
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
pub fn node_child(data: NodeData<'_>) -> NodeData<'_> {
    data
}

#[doc(hidden)]
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
pub fn pretty_output((rendered, source_map): (String, SourceMap)) -> PrettyOutput {
    PrettyOutput {
        rendered,
        source_map,
    }
}

#[doc(hidden)]
pub fn sample_error(message: impl Into<Box<str>>) -> SampleError {
    SampleError {
        message: message.into(),
    }
}

#[doc(hidden)]
pub const fn lex_record(schema: &'static TokenSchemaData, span: Span) -> LexRecordData {
    LexRecordData { schema, span }
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

/// # Safety
/// The schema and sample function must belong to the same Generation.
#[doc(hidden)]
pub const unsafe fn construct_adapter(
    schema: &'static GrammarSchemaData,
    pretty: bool,
    arbitrary: bool,
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    sample: Option<SampleFn>,
) -> Adapter {
    Adapter {
        schema,
        capabilities: Capabilities { pretty, arbitrary },
        lex,
        sample,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    static GRAMMAR: GrammarSchemaData = grammar_schema("grammar", &ROOT, &NODES, &TOKENS);

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

    unsafe impl ExplorerRoot for Root {
        fn __recursa_explorer_schema() -> &'static GrammarSchemaData {
            &GRAMMAR
        }

        fn __recursa_explorer_pretty(
            _: &Parsed<'_, Self>,
            _: &PrettyConfig,
        ) -> Option<PrettyOutput> {
            None
        }
    }

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

        let grammar = GrammarSchema { data: &GRAMMAR };
        let nodes = grammar.nodes();
        let tokens = grammar.tokens();
        exact_fused(&nodes);
        exact_fused(&tokens);
        assert_eq!(nodes.len(), 2);
        assert_eq!(tokens.len(), 1);
        assert_eq!(grammar.root().qualified_name(), "grammar::Root");
        assert_eq!(tokens.into_iter().next().unwrap().kind(), "WORD");
    }
}
