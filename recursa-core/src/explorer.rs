//! Grammar-erased, allocation-bounded explorer contracts.

use crate::input::TokenRecord;
use crate::parsed::GeneratedParse;
use crate::parsed::ViewCursor;
use crate::{
    ElementRole, FollowSet, GrammarInfo, Input, ParseErrorKind, Parsed, PrettyConfig, SourceMap,
    Span,
};
use std::collections::BTreeSet;
use std::fmt;

/// Erased source-borrowing parse callback generated for one root application.
type ParseFn =
    for<'input> fn(&'input str) -> Result<ExplorerDocument<'input>, ParseFailure<'input>>;

/// Erased source-borrowing exploration callback generated for one root application.
type ExploreFn =
    for<'input> fn(&'input str) -> Result<Exploration<'input>, ExplorationError<'input>>;

/// Erased deterministic sample callback generated for one root application.
type SampleFn = fn(&[u8]) -> Result<ExplorerDocument<'static>, SampleError>;

/// Grammar-erased schema, source operations, and optional sampling for one root family.
#[derive(Clone, Copy)]
pub struct Adapter {
    /// Static schema from the same Generation as every callback.
    schema: &'static GrammarSchemaData,
    /// Feature-selected operations available for this root.
    capabilities: Capabilities,
    /// Grammar-erased lexer callback borrowing its input.
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    /// Grammar-erased strict parser callback borrowing its input.
    parse: ParseFn,
    /// Optional grammar-erased recovery callback borrowing its input.
    explore: Option<ExploreFn>,
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
        (self.parse)(source)
    }

    /// Explores complete or recoverable source when this root supports recovery.
    ///
    /// `None` means the root's grammar includes a recovery form not implemented by
    /// this Recursa release. Fatal grammar or span invariants are returned separately
    /// from construction-free recovery documents.
    pub fn explore(self, source: &str) -> Option<Result<Exploration<'_>, ExplorationError<'_>>> {
        self.explore.map(|explore| explore(source))
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
    /// Required syntax is absent at a checked insertion position.
    MissingSyntax,
    /// One contiguous input range was discarded while synchronizing.
    SkippedSyntax,
    /// Frozen restart and stop evidence did not establish one safe action.
    UnresolvedAlternative,
    /// A generated recovery plan violated its progress or identity contract.
    InvariantViolation,
}

/// Source edit associated with one recovery diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    /// Suggests insertion at a checked zero-width source span.
    Missing {
        /// Checked zero-width insertion position.
        at: Span,
    },
    /// Identifies a contiguous source range discarded during synchronization.
    Skipped {
        /// Checked nonempty discarded range.
        span: Span,
    },
}

/// One structured lexical or syntax diagnostic from [`Adapter::parse`].
#[derive(Clone)]
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
    /// Recovery action attached to this diagnostic, when any.
    action: Option<RecoveryAction>,
    /// Recovery frame that owns this diagnostic, absent on strict-only results.
    owner: Option<&'static NodeSchemaData>,
    /// Recovery element site, absent for root dispatch and trailing-input actions.
    site: Option<&'static ElementSchemaData>,
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

    /// Returns the construction-free recovery action attached to this diagnostic.
    pub const fn action(&self) -> Option<RecoveryAction> {
        self.action
    }

    /// Returns the recovery Node that owns this diagnostic, when recovery ran.
    pub const fn owner(&self) -> Option<NodeSchema> {
        match self.owner {
            Some(data) => Some(NodeSchema { data }),
            None => None,
        }
    }

    /// Returns the recovery element site, when the action belongs to one.
    pub const fn site(&self) -> Option<ElementSchema> {
        match self.site {
            Some(data) => Some(ElementSchema { data }),
            None => None,
        }
    }
}

impl PartialEq for ParseDiagnostic<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.span == other.span
            && self.text == other.text
            && self.found == other.found
            && self.expected == other.expected
            && self.message == other.message
            && self.action == other.action
            && match (self.owner, other.owner) {
                (Some(left), Some(right)) => std::ptr::eq(left, right),
                (None, None) => true,
                _ => false,
            }
            && match (self.site, other.site) {
                (Some(left), Some(right)) => std::ptr::eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for ParseDiagnostic<'_> {}

impl fmt::Debug for ParseDiagnostic<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParseDiagnostic")
            .field("kind", &self.kind)
            .field("span", &self.span)
            .field("text", &self.text)
            .field("found", &self.found)
            .field("expected", &self.expected)
            .field("message", &self.message)
            .field("action", &self.action)
            .field("owner", &self.owner.map(|owner| owner.qualified_name))
            .field("site", &self.site.map(|site| site.name))
            .finish()
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
            action: None,
            owner: None,
            site: None,
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

/// Mutually exclusive result of exploring one source input.
pub enum Exploration<'input> {
    /// Strict parsing produced one complete authored value and Explorer document.
    Complete(ExplorerDocument<'input>),
    /// Strict parsing failed and construction-free recovery described the source.
    Recovered(RecoveryDocument<'input>),
}

/// A construction-free recovery projection produced after strict parsing fails.
pub struct RecoveryDocument<'input> {
    /// Complete source borrowed by the recovery projection.
    source: &'input str,
    /// Eager grammar-erased recovery root.
    root: RecoveryNode,
    /// Ordered strict and recovery diagnostics.
    diagnostics: Box<[ParseDiagnostic<'input>]>,
    /// Retained execution evidence for the bounded iterative recovery machine.
    progress: RecoveryProgressTrace,
}

/// Internal construction-free projection that does not imply a parse failure.
pub(crate) struct RecoveryProjection<'input> {
    /// Complete coordinate source borrowed by recovery.
    source: &'input str,
    /// Eager grammar-erased recovery root.
    root: RecoveryNode,
    /// Optional seed and recovery diagnostics.
    diagnostics: Box<[ParseDiagnostic<'input>]>,
    /// Retained bounded-machine progress evidence.
    progress: RecoveryProgressTrace,
}

impl RecoveryProjection<'_> {
    /// Borrows the eager grammar-erased root.
    pub(crate) const fn root(&self) -> &RecoveryNode {
        &self.root
    }

    /// Borrows optional seed and recovery diagnostics.
    pub(crate) fn diagnostics(&self) -> &[ParseDiagnostic<'_>] {
        &self.diagnostics
    }

    /// Returns retained bounded-machine progress evidence.
    pub(crate) const fn progress_trace(&self) -> RecoveryProgressTrace {
        self.progress
    }
}

impl RecoveryDocument<'_> {
    /// Returns the complete source inspected by recovery.
    pub const fn source(&self) -> &str {
        self.source
    }

    /// Returns the eager construction-free root projection.
    pub const fn root(&self) -> &RecoveryNode {
        &self.root
    }

    /// Returns strict diagnostics seeded unchanged plus later recovery diagnostics.
    pub fn diagnostics(&self) -> &[ParseDiagnostic<'_>] {
        &self.diagnostics
    }

    /// Returns the retained progress evidence for this recovery execution.
    pub const fn progress_trace(&self) -> RecoveryProgressTrace {
        self.progress
    }
}

/// Retained evidence that one recovery execution stayed within its finite bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryProgressTrace {
    /// Number of finalized token and lexical-error records available to recovery.
    input_records: usize,
    /// Number of explicit machine frames entered during recovery.
    entered_frames: usize,
    /// Number of explicit machine frames permanently closed during recovery.
    closed_frames: usize,
    /// Number of distinct guarded machine states executed.
    guarded_states: usize,
    /// Conservative maximum guarded states for the entered frames and input positions.
    state_bound: usize,
}

impl RecoveryProgressTrace {
    /// Returns the number of input records available to recovery.
    pub const fn input_records(self) -> usize {
        self.input_records
    }

    /// Returns the number of explicit frames entered.
    pub const fn entered_frames(self) -> usize {
        self.entered_frames
    }

    /// Returns the number of explicit frames permanently closed.
    pub const fn closed_frames(self) -> usize {
        self.closed_frames
    }

    /// Returns the number of distinct guarded machine states executed.
    pub const fn guarded_states(self) -> usize {
        self.guarded_states
    }

    /// Returns the finite execution bound derived from input positions and entered states.
    pub const fn state_bound(self) -> usize {
        self.state_bound
    }

    /// Reports whether the retained trace satisfies the progress invariant.
    pub const fn is_within_bound(self) -> bool {
        self.entered_frames == self.closed_frames && self.guarded_states <= self.state_bound
    }
}

/// One eager construction-free Node projection.
pub struct RecoveryNode {
    /// Static schema from the same generated recovery plan.
    schema: &'static NodeSchemaData,
    /// Permanently committed enum variant, absent for structs and uncommitted enums.
    committed_variant: Option<&'static VariantSchemaData>,
    /// Checked contiguous navigation extent owned by this frame.
    extent: Span,
    /// Recognized, missing, separator, and skipped parts in traversal order.
    parts: Box<[RecoveryPart]>,
}

impl RecoveryNode {
    /// Returns this projection's static materialized Node schema.
    pub const fn schema(&self) -> NodeSchema {
        NodeSchema { data: self.schema }
    }

    /// Returns the permanently committed enum variant, when dispatch selected one.
    pub const fn committed_variant(&self) -> Option<VariantSchema> {
        match self.committed_variant {
            Some(data) => Some(VariantSchema { data }),
            None => None,
        }
    }

    /// Returns the checked contiguous source navigation extent.
    pub const fn extent(&self) -> Span {
        self.extent
    }

    /// Iterates source- and grammar-ordered recovery parts.
    pub fn parts(&self) -> impl ExactSizeIterator<Item = &RecoveryPart> {
        self.parts.iter()
    }

    /// Reports whether this node and every descendant contain only recognized syntax.
    pub fn is_complete(&self) -> bool {
        (self.schema.variants.is_empty() || self.committed_variant.is_some())
            && self.parts.iter().all(|part| match part {
                RecoveryPart::Element(element) => match &element.value {
                    RecoveryValue::Token(_) => true,
                    RecoveryValue::Node(node) => node.is_complete(),
                },
                RecoveryPart::Separator(_) => true,
                RecoveryPart::Missing(_) | RecoveryPart::Skipped(_) => false,
            })
    }
}

/// One grammar- and traversal-ordered recovery part.
pub enum RecoveryPart {
    /// Recognized fixed or semantic element occurrence.
    Element(RecoveryElement),
    /// Recognized repetition separator kept distinct from element ordinals.
    Separator(RecoverySeparator),
    /// Required syntax absent at a checked insertion position.
    Missing(RecoveryMissing),
    /// Maximal contiguous input run discarded by one recovery frame.
    Skipped(RecoverySkipped),
}

/// One recognized element occurrence.
pub struct RecoveryElement {
    /// Static schema for this authored grammar element.
    schema: &'static ElementSchemaData,
    /// Checked recognized source extent.
    span: Span,
    /// Recognized token or recursively recovered Node.
    value: RecoveryValue,
}

impl RecoveryElement {
    /// Returns this occurrence's static element schema.
    pub const fn schema(&self) -> ElementSchema {
        ElementSchema { data: self.schema }
    }

    /// Returns the checked recognized source extent.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the recognized token or recursively recovered Node.
    pub const fn value(&self) -> &RecoveryValue {
        &self.value
    }
}

/// Recognized value retained without constructing an authored semantic value.
pub enum RecoveryValue {
    /// One exact token record.
    Token(RecoveryToken),
    /// One eager recursively recovered Node.
    Node(Box<RecoveryNode>),
}

/// One exact recognized token record.
#[derive(Clone, Copy)]
pub struct RecoveryToken {
    /// Static grammar token schema.
    schema: &'static TokenSchemaData,
    /// Checked exact source extent.
    span: Span,
}

impl RecoveryToken {
    /// Returns this token's static grammar schema.
    pub const fn schema(self) -> TokenSchema {
        TokenSchema { data: self.schema }
    }

    /// Returns its checked exact source extent.
    pub const fn span(self) -> Span {
        self.span
    }
}

/// One recognized repetition separator.
#[derive(Clone, Copy)]
pub struct RecoverySeparator {
    /// Static token schema.
    schema: &'static TokenSchemaData,
    /// Checked exact source extent.
    span: Span,
}

impl RecoverySeparator {
    /// Returns the separator token schema.
    pub const fn schema(self) -> TokenSchema {
        TokenSchema { data: self.schema }
    }

    /// Returns the checked exact source extent.
    pub const fn span(self) -> Span {
        self.span
    }
}

/// Required syntax absent at one grammar site.
pub struct RecoveryMissing {
    /// Static element schema retaining site identity.
    element: &'static ElementSchemaData,
    /// Checked zero-width insertion position.
    at: Span,
    /// Analysis-ordered expected atoms.
    expected: Box<[&'static str]>,
}

impl RecoveryMissing {
    /// Returns the static element schema for the missing site.
    pub const fn element(&self) -> ElementSchema {
        ElementSchema { data: self.element }
    }

    /// Returns the checked zero-width insertion position.
    pub const fn at(&self) -> Span {
        self.at
    }

    /// Iterates analysis-ordered expected atoms.
    pub fn expected(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.expected.iter().copied()
    }
}

/// One contiguous input run discarded by a recovery frame.
pub struct RecoverySkipped {
    /// Checked maximal source extent.
    span: Span,
    /// Exact input items contained by the discarded run.
    items: Box<[RecoveryInputItem]>,
}

impl RecoverySkipped {
    /// Returns the checked discarded source extent.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Iterates discarded token and lexical-error items in lexer order.
    pub fn items(&self) -> impl ExactSizeIterator<Item = RecoveryInputItem> + '_ {
        self.items.iter().copied()
    }
}

/// One finalized input item owned by a skipped recovery part.
#[derive(Clone, Copy)]
pub struct RecoveryInputItem {
    /// Static token schema, absent for lexical-error records.
    schema: Option<&'static TokenSchemaData>,
    /// Checked exact source extent.
    span: Span,
}

impl RecoveryInputItem {
    /// Returns the static token schema, absent for lexical errors.
    pub const fn schema(self) -> Option<TokenSchema> {
        match self.schema {
            Some(data) => Some(TokenSchema { data }),
            None => None,
        }
    }

    /// Returns the checked exact source extent.
    pub const fn span(self) -> Span {
        self.span
    }
}

/// Fatal failure while exploring source, distinct from recoverable syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplorationError<'input> {
    /// Complete source shared by every fatal diagnostic.
    source: &'input str,
    /// Nonempty diagnostics describing the invariant failure.
    diagnostics: Box<[ParseDiagnostic<'input>]>,
}

impl ExplorationError<'_> {
    /// Returns the complete source that could not be explored safely.
    pub const fn source(&self) -> &str {
        self.source
    }

    /// Returns fatal exploration diagnostics.
    pub fn diagnostics(&self) -> &[ParseDiagnostic<'_>] {
        &self.diagnostics
    }
}

impl fmt::Display for ExplorationError<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.diagnostics.as_ref() {
            [diagnostic] => diagnostic.fmt(formatter),
            diagnostics => write!(
                formatter,
                "{} fatal exploration diagnostics",
                diagnostics.len()
            ),
        }
    }
}

impl std::error::Error for ExplorationError<'_> {}

/// Optional operations configured for an Explorer root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    /// Whether the root can render structured Pretty output.
    pretty: bool,
    /// Whether the root can generate deterministic samples.
    arbitrary: bool,
    /// Whether construction-free recovery is available for this root.
    recovery: bool,
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

    /// Reports whether construction-free recovery is available.
    pub const fn recovery(self) -> bool {
        self.recovery
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
    data: &'static GrammarSchemaData,
}

impl GrammarSchema {
    /// Returns the configured grammar name.
    pub const fn name(self) -> &'static str {
        self.data.name
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
            remaining: self.data.nodes,
        }
    }

    /// Iterates over every content and fixed token descriptor.
    pub fn tokens(self) -> TokenSchemas {
        TokenSchemas {
            remaining: self.data.tokens,
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
/// Generated static schema backing one [`GrammarSchema`] handle.
pub struct GrammarSchemaData {
    /// Configured grammar name.
    name: &'static str,
    /// Root materialized application.
    root: &'static NodeSchemaData,
    /// Deterministically ordered Node applications.
    nodes: &'static [&'static NodeSchemaData],
    /// Deterministically ordered token descriptors.
    tokens: &'static [&'static TokenSchemaData],
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
/// Immutable generated grammar-erased recovery plan for one root application.
pub struct RecoveryPlanData {
    /// Matching generated grammar metadata.
    grammar: &'static GrammarInfo,
    /// Root application interpreted after strict failure.
    root: &'static RecoveryNodePlanData,
    /// Empty caller FOLLOW supplied to the complete root.
    root_follow: &'static FollowSet,
    /// Token schemas indexed by grammar-local token kind; lexical errors are absent.
    tokens: &'static [Option<&'static TokenSchemaData>],
    /// Frozen spelling equivalence applied to predictive text predicates.
    text_equivalence: RecoveryTextEquivalenceData,
}

#[doc(hidden)]
/// Runtime spelling equivalence retained without a primitive flag.
pub enum RecoveryTextEquivalenceData {
    /// Source spellings compare exactly.
    Sensitive,
    /// Source spellings compare with ASCII case folding.
    AsciiInsensitive,
}

#[doc(hidden)]
/// Immutable recovery recipe for one materialized Node application.
pub struct RecoveryNodePlanData {
    /// Matching static Explorer Node schema.
    schema: &'static NodeSchemaData,
    /// Construction-free body recipe.
    body: RecoveryNodeBodyPlanData,
}

#[doc(hidden)]
/// Construction-free body recipe selected during generation.
pub enum RecoveryNodeBodyPlanData {
    /// One source-order structural sequence.
    Struct {
        /// Immutable element recipes in source order.
        elements: &'static [RecoveryElementPlanData],
        /// Frozen FIRST set proving a distinct occurrence can restart this struct root.
        restart: &'static FollowSet,
    },
    /// Enum support is generated in #94 and interpreted through its frozen dispatch.
    Enum {
        /// Variant recipes in dispatch-target order.
        variants: &'static [RecoveryVariantPlanData],
        /// Bounded frozen predictive trie.
        dispatch: &'static RecoveryDispatchNodeData,
        /// Frozen union of branch prefixes that can restart dispatch.
        restart: &'static FollowSet,
    },
    /// One binding-power-aware Pratt expression family.
    Pratt(
        /// Immutable nud, led, binding-power, and caller-FOLLOW products.
        RecoveryPrattPlanData,
    ),
}

#[doc(hidden)]
/// Immutable construction-free recovery recipe for one Pratt expression family.
pub struct RecoveryPrattPlanData {
    /// Null-denotation variants in frozen dispatch-target order.
    nud: &'static [RecoveryPrattNudPlanData],
    /// Left-denotation variants in frozen dispatch-target order.
    led: &'static [RecoveryPrattLedPlanData],
    /// Bounded frozen selector for the initial null denotation.
    nud_dispatch: &'static RecoveryDispatchNodeData,
    /// Frozen union of null-denotation prefixes used for synchronization.
    nud_restart: &'static FollowSet,
    /// Exact frozen failure evidence when no null denotation can start.
    nud_failure: RecoveryFollowData,
    /// Frozen extender starters admitted while completing nud dispatch.
    nud_selection_follow: &'static FollowSet,
    /// Bounded frozen selector for extenders, absent when none are authored.
    led_dispatch: Option<&'static RecoveryDispatchNodeData>,
    /// Frozen caller-FOLLOW guard branches paired with extender binding powers.
    caller_follow_guard: &'static [RecoveryPrattGuardPlanData],
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// One null-denotation recovery variant and its exact binding role.
pub struct RecoveryPrattNudPlanData {
    /// Schema and source-ordered body elements of the committed variant.
    variant: RecoveryVariantPlanData,
    /// Exact recursive operands frozen with their minimum binding powers.
    ///
    /// An atom freezes no operands; a prefix freezes each recursive operand
    /// beside its exact minimum binding power. The interpreter never needs the
    /// nud role itself because operand thresholds carry the whole distinction.
    operands: &'static [RecoveryPrattOperandPlanData],
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Direct association between one Pratt branch element and its recursive threshold.
pub struct RecoveryPrattOperandPlanData {
    /// Generated element index that owns the recursive occurrence.
    element_index: RecoveryElementIndexData,
    /// Minimum binding power supplied when entering that occurrence.
    minimum_binding_power: u8,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// One left-denotation recovery variant and its exact binding role.
pub struct RecoveryPrattLedPlanData {
    /// Schema and source-ordered tail elements of the committed variant.
    variant: RecoveryVariantPlanData,
    /// Static schema of the distinguished already-recovered left operand.
    lhs: &'static ElementSchemaData,
    /// Infix or postfix role frozen by semantic analysis.
    position: RecoveryPrattLedPositionData,
    /// Exact recursive tail operands frozen with their minimum binding powers.
    operands: &'static [RecoveryPrattOperandPlanData],
}

#[doc(hidden)]
/// Frozen left-denotation binding role retained without associativity inference.
#[derive(Clone, Copy)]
pub enum RecoveryPrattLedPositionData {
    /// An infix extender with independent left and right thresholds.
    Infix {
        /// Minimum binding power required to select this extender.
        left_binding_power: u8,
    },
    /// A postfix extender with one selection threshold.
    Postfix {
        /// Minimum binding power required to select this extender.
        binding_power: u8,
    },
}

impl RecoveryPrattLedPositionData {
    /// Returns the frozen left threshold used by strict and recovery selection.
    const fn left_binding_power(self) -> u8 {
        match self {
            Self::Infix { left_binding_power } => left_binding_power,
            Self::Postfix { binding_power } => binding_power,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// One frozen caller-FOLLOW guard contribution for a Pratt extender family.
pub struct RecoveryPrattGuardPlanData {
    /// Extender threshold at which this guard contribution qualifies.
    left_binding_power: u8,
    /// Token set admitted by the matching frozen extender branch.
    expected: &'static FollowSet,
}

#[doc(hidden)]
/// One generated enum branch recovery recipe.
#[derive(Clone, Copy)]
pub struct RecoveryVariantPlanData {
    /// Matching static Explorer variant schema.
    schema: &'static VariantSchemaData,
    /// Branch elements in source order.
    elements: &'static [RecoveryElementPlanData],
}

#[doc(hidden)]
/// One immutable grammar-element recovery recipe.
pub struct RecoveryElementPlanData {
    /// Matching static Explorer element schema.
    schema: &'static ElementSchemaData,
    /// Evaluated FOLLOW policy at this exact parse site.
    stop: RecoveryFollowData,
    /// Fixed or semantic construction-free operation.
    kind: RecoveryElementKindData,
}

#[doc(hidden)]
/// One fixed or semantic construction-free recovery operation.
pub enum RecoveryElementKindData {
    /// Required fixed token.
    Fixed {
        /// Exact expected kinds and analysis-ordered atom names.
        expected: &'static FollowSet,
    },
    /// Atomic optional fixed group.
    OptionalFixed {
        /// Required tokens in sequence order.
        tokens: &'static [RecoveryFixedPlanData],
    },
    /// Semantic terminal controlled by a frozen container machine.
    Semantic {
        /// Grammar-erased terminal application or content token.
        terminal: RecoveryTerminalPlanData,
        /// Construction-free container transition recipe.
        container: RecoveryContainerPlanData,
        /// Required or optional fixed syntax before the semantic value.
        before: &'static [RecoveryFixedSyntaxPlanData],
        /// Required or optional fixed syntax after the semantic value.
        after: &'static [RecoveryFixedSyntaxPlanData],
    },
    /// Fixed-presence fields retain no authored boolean during recovery.
    FixedPresence {
        /// Required or atomic optional syntax before the presence sequence.
        before: &'static [RecoveryFixedSyntaxPlanData],
        /// Optional fixed sequence selected atomically.
        sequence: &'static [RecoveryFixedPlanData],
        /// Required or atomic optional syntax after the presence sequence.
        after: &'static [RecoveryFixedSyntaxPlanData],
    },
}

#[doc(hidden)]
/// Required token or atomic optional group inside a fixed attachment.
pub enum RecoveryFixedSyntaxPlanData {
    /// One required fixed token.
    Required(
        /// Exact expectation for the required token.
        RecoveryFixedPlanData,
    ),
    /// One fixed sequence selected atomically.
    Optional(
        /// Exact expectations in sequence order.
        &'static [RecoveryFixedPlanData],
    ),
}

#[doc(hidden)]
/// One fixed-token expectation within an attachment or optional group.
pub struct RecoveryFixedPlanData {
    /// Exact expected kinds and analysis-ordered atom names.
    expected: &'static FollowSet,
}

#[doc(hidden)]
/// Grammar-erased terminal reached by one semantic element.
#[derive(Clone, Copy)]
pub enum RecoveryTerminalPlanData {
    /// Another materialized generated Node application.
    Node(
        /// Immutable recipe for the nested application.
        &'static RecoveryNodePlanData,
    ),
    /// One content-bearing token schema.
    Content(
        /// Static schema retained by a recognized token occurrence.
        &'static TokenSchemaData,
    ),
}

#[doc(hidden)]
/// Frozen construction-free container machine.
pub enum RecoveryContainerPlanData {
    /// Exactly one required terminal.
    Direct {
        /// FIRST/failure evidence for the required terminal.
        restart: RecoveryFollowData,
        /// FOLLOW context supplied to a nested Node frame.
        child_follow: RecoveryFollowData,
    },
    /// Zero or one terminal.
    Optional {
        /// FIRST selector that commits the optional occurrence.
        restart: &'static FollowSet,
        /// FOLLOW context supplied to a nested Node frame.
        child_follow: RecoveryFollowData,
    },
    /// Frozen repeated-container states.
    Repeated {
        /// Frozen minimum occurrence policy.
        minimum: RecoveryMinimumData,
        /// Whether the entire nonempty repetition may be absent.
        presence: RecoveryRepetitionPresenceData,
        /// FIRST selector for an occurrence.
        element: &'static FollowSet,
        /// Optional fixed separator selector.
        separator: Option<&'static FollowSet>,
        /// Frozen trailing-separator policy.
        trailing: RecoveryTrailingSeparatorData,
        /// Evaluated repetition stop policy.
        stop: RecoveryFollowData,
        /// FOLLOW context supplied to nested Node frames.
        child_follow: RecoveryFollowData,
        /// Frozen failure evidence when the required minimum is absent.
        minimum_failure: Option<RecoveryFollowData>,
        /// Frozen failure evidence between completed occurrences.
        continuation_failure: RecoveryFollowData,
        /// Frozen failure evidence after a non-trailing separator.
        after_separator_failure: Option<RecoveryFollowData>,
    },
}

#[doc(hidden)]
/// Minimum occurrence policy for one repeated recovery container.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum RecoveryMinimumData {
    /// Zero occurrences are valid.
    Zero,
    /// At least one occurrence is required once the container is present.
    One,
}

#[doc(hidden)]
/// Presence policy for one repeated recovery container.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum RecoveryRepetitionPresenceData {
    /// The container itself is required.
    Required,
    /// The entire nonempty container may be absent.
    Optional,
}

#[doc(hidden)]
/// Trailing-separator policy for one repeated recovery container.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum RecoveryTrailingSeparatorData {
    /// A consumed separator requires another occurrence.
    Forbidden,
    /// A consumed final separator closes cleanly.
    Allowed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
/// Requiredness of one direct or optional semantic container.
enum RecoveryRequirement {
    /// A stopped container emits one missing transition.
    Required,
    /// A stopped container closes without a missing transition.
    Optional,
}

#[doc(hidden)]
/// Static, caller-dependent, or combined FOLLOW recipe.
#[derive(Clone, Copy)]
pub enum RecoveryFollowData {
    /// Uses only the caller's evaluated FOLLOW set.
    Caller,
    /// Uses one generated static set.
    Static(
        /// Frozen set used without the caller context.
        &'static FollowSet,
    ),
    /// Unions one generated static set with the caller context.
    StaticAndCaller(
        /// Frozen set united with the caller at frame entry.
        &'static FollowSet,
    ),
}

#[doc(hidden)]
/// One bounded immutable predictive-dispatch trie state.
pub struct RecoveryDispatchNodeData {
    /// Unique branch selected when this prefix ends.
    target: Option<RecoveryDispatchTargetData>,
    /// Variant uniquely determined by every terminal below this prefix.
    commit: Option<RecoveryVariantIndexData>,
    /// Analysis-ordered outgoing transitions.
    edges: &'static [RecoveryDispatchEdgeData],
}

#[doc(hidden)]
/// One frozen predictive-dispatch terminal.
#[derive(Clone, Copy)]
pub struct RecoveryDispatchTargetData {
    /// Zero-based variant index.
    variant: RecoveryVariantIndexData,
    /// Frozen terminal condition at this dispatch depth.
    condition: RecoveryDispatchTerminalConditionData,
}

#[doc(hidden)]
/// Typed zero-based identity of one generated recovery variant.
#[derive(Clone, Copy)]
pub struct RecoveryVariantIndexData(usize);

impl RecoveryVariantIndexData {
    /// Returns the raw index only at the generated variant-table boundary.
    const fn get(self) -> usize {
        self.0
    }
}

#[doc(hidden)]
/// Terminal condition retained without a primitive flag.
#[derive(Clone, Copy)]
pub enum RecoveryDispatchTerminalConditionData {
    /// Selection is unconditional at this prefix.
    Unconditional,
    /// Selection requires EOF or the caller FOLLOW set.
    CallerFollow,
}

#[doc(hidden)]
/// One immutable predictive-dispatch transition.
pub struct RecoveryDispatchEdgeData {
    /// Kinds accepted at this lookahead position.
    expected: &'static FollowSet,
    /// Optional exact spelling refinement.
    text: RecoveryTextPredicateData,
    /// Next bounded trie state.
    next: &'static RecoveryDispatchNodeData,
}

#[doc(hidden)]
/// Frozen source-spelling predicate for predictive dispatch.
pub enum RecoveryTextPredicateData {
    /// No spelling refinement.
    Any,
    /// One exact normalized spelling.
    Exact(
        /// Canonical normalized spelling.
        &'static str,
    ),
    /// One spelling from a canonical normalized set.
    OneOf(
        /// Canonical normalized spellings accepted by this edge.
        &'static [&'static str],
    ),
    /// Any spelling except a canonical normalized set.
    NotIn(
        /// Canonical normalized spellings excluded from this edge.
        &'static [&'static str],
    ),
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
/// The schema and operation must belong to the same Generation as the Node.
#[doc(hidden)]
pub unsafe trait ExplorerRoot: ExplorerNode + Sized {
    /// Returns the static schema belonging to this root's Generation.
    fn __recursa_explorer_schema() -> &'static GrammarSchemaData;

    /// Renders parsed or synthetic Pretty output when configured.
    fn __recursa_explorer_pretty(
        parsed: &Parsed<'_, Self>,
        config: &PrettyConfig,
    ) -> Option<PrettyOutput>;
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

/// Validates one generated recovery plan against its framing root identities.
pub(crate) fn recovery_plan_matches_root(
    plan: &RecoveryPlanData,
    grammar: &'static GrammarInfo,
    schema: &'static GrammarSchemaData,
) -> bool {
    std::ptr::eq(plan.grammar, grammar) && std::ptr::eq(plan.root.schema, schema.root)
}

/// Reports whether one token has frozen evidence to restart the recovery root.
pub(crate) fn recovery_root_restart_matches(plan: &RecoveryPlanData, record: TokenRecord) -> bool {
    match &plan.root.body {
        RecoveryNodeBodyPlanData::Struct { restart, .. } => restart.contains(record.kind),
        RecoveryNodeBodyPlanData::Enum { restart, .. } => restart.contains(record.kind),
        RecoveryNodeBodyPlanData::Pratt(pratt) => pratt.nud_restart.contains(record.kind),
    }
}

/// Reports whether one static variant belongs directly to the selected root.
pub(crate) fn root_contains_variant(
    schema: &'static GrammarSchemaData,
    variant: &'static VariantSchemaData,
) -> bool {
    schema
        .root
        .variants
        .iter()
        .any(|candidate| std::ptr::eq(*candidate, variant))
}

#[doc(hidden)]
/// Constructs generated static grammar schema under the matching protocol.
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
/// Constructs one immutable generated recovery plan.
pub const fn recovery_plan(
    grammar: &'static GrammarInfo,
    root: &'static RecoveryNodePlanData,
    root_follow: &'static FollowSet,
    tokens: &'static [Option<&'static TokenSchemaData>],
    text_equivalence: RecoveryTextEquivalenceData,
) -> RecoveryPlanData {
    RecoveryPlanData {
        grammar,
        root,
        root_follow,
        tokens,
        text_equivalence,
    }
}

#[doc(hidden)]
/// Constructs one enum-shaped generated recovery Node recipe.
pub const fn recovery_enum_plan(
    schema: &'static NodeSchemaData,
    variants: &'static [RecoveryVariantPlanData],
    dispatch: &'static RecoveryDispatchNodeData,
    restart: &'static FollowSet,
) -> RecoveryNodePlanData {
    RecoveryNodePlanData {
        schema,
        body: RecoveryNodeBodyPlanData::Enum {
            variants,
            dispatch,
            restart,
        },
    }
}

#[doc(hidden)]
/// Constructs one enum branch recovery recipe.
pub const fn recovery_variant_plan(
    schema: &'static VariantSchemaData,
    elements: &'static [RecoveryElementPlanData],
) -> RecoveryVariantPlanData {
    RecoveryVariantPlanData { schema, elements }
}

#[doc(hidden)]
/// Constructs one bounded predictive-dispatch trie state.
pub const fn recovery_dispatch_node(
    target: Option<RecoveryDispatchTargetData>,
    commit: Option<RecoveryVariantIndexData>,
    edges: &'static [RecoveryDispatchEdgeData],
) -> RecoveryDispatchNodeData {
    RecoveryDispatchNodeData {
        target,
        commit,
        edges,
    }
}

#[doc(hidden)]
/// Constructs one predictive-dispatch terminal.
pub const fn recovery_dispatch_target(
    variant: RecoveryVariantIndexData,
    condition: RecoveryDispatchTerminalConditionData,
) -> RecoveryDispatchTargetData {
    RecoveryDispatchTargetData { variant, condition }
}

#[doc(hidden)]
/// Constructs one typed generated recovery-variant identity.
pub const fn recovery_variant_index(index: usize) -> RecoveryVariantIndexData {
    RecoveryVariantIndexData(index)
}

#[doc(hidden)]
/// Constructs one typed generated recovery-element position.
pub const fn recovery_element_index(index: usize) -> RecoveryElementIndexData {
    RecoveryElementIndexData(index)
}

#[doc(hidden)]
/// Constructs one predictive-dispatch edge.
pub const fn recovery_dispatch_edge(
    expected: &'static FollowSet,
    text: RecoveryTextPredicateData,
    next: &'static RecoveryDispatchNodeData,
) -> RecoveryDispatchEdgeData {
    RecoveryDispatchEdgeData {
        expected,
        text,
        next,
    }
}

#[doc(hidden)]
/// Returns an unrefined predictive spelling predicate.
pub const fn recovery_text_any() -> RecoveryTextPredicateData {
    RecoveryTextPredicateData::Any
}

#[doc(hidden)]
/// Returns an exact predictive spelling predicate.
pub const fn recovery_text_exact(text: &'static str) -> RecoveryTextPredicateData {
    RecoveryTextPredicateData::Exact(text)
}

#[doc(hidden)]
/// Returns a predictive spelling-set predicate.
pub const fn recovery_text_one_of(text: &'static [&'static str]) -> RecoveryTextPredicateData {
    RecoveryTextPredicateData::OneOf(text)
}

#[doc(hidden)]
/// Returns an excluding predictive spelling-set predicate.
pub const fn recovery_text_not_in(text: &'static [&'static str]) -> RecoveryTextPredicateData {
    RecoveryTextPredicateData::NotIn(text)
}

#[doc(hidden)]
/// Constructs one struct-shaped generated recovery Node recipe.
pub const fn recovery_struct_plan(
    schema: &'static NodeSchemaData,
    elements: &'static [RecoveryElementPlanData],
    restart: &'static FollowSet,
) -> RecoveryNodePlanData {
    RecoveryNodePlanData {
        schema,
        body: RecoveryNodeBodyPlanData::Struct { elements, restart },
    }
}

#[doc(hidden)]
/// Constructs one binding-power-aware Pratt recovery Node recipe.
#[allow(clippy::too_many_arguments)]
pub const fn recovery_pratt_plan(
    schema: &'static NodeSchemaData,
    nud: &'static [RecoveryPrattNudPlanData],
    led: &'static [RecoveryPrattLedPlanData],
    nud_dispatch: &'static RecoveryDispatchNodeData,
    nud_restart: &'static FollowSet,
    nud_failure: RecoveryFollowData,
    nud_selection_follow: &'static FollowSet,
    led_dispatch: Option<&'static RecoveryDispatchNodeData>,
    caller_follow_guard: &'static [RecoveryPrattGuardPlanData],
) -> RecoveryNodePlanData {
    RecoveryNodePlanData {
        schema,
        body: RecoveryNodeBodyPlanData::Pratt(RecoveryPrattPlanData {
            nud,
            led,
            nud_dispatch,
            nud_restart,
            nud_failure,
            nud_selection_follow,
            led_dispatch,
            caller_follow_guard,
        }),
    }
}

#[doc(hidden)]
/// Constructs one atom-shaped null-denotation recovery variant.
pub const fn recovery_pratt_atom(variant: RecoveryVariantPlanData) -> RecoveryPrattNudPlanData {
    RecoveryPrattNudPlanData {
        variant,
        operands: &[],
    }
}

#[doc(hidden)]
/// Constructs one prefix null-denotation recovery variant.
pub const fn recovery_pratt_prefix(
    variant: RecoveryVariantPlanData,
    operands: &'static [RecoveryPrattOperandPlanData],
) -> RecoveryPrattNudPlanData {
    RecoveryPrattNudPlanData { variant, operands }
}

#[doc(hidden)]
/// Constructs one infix left-denotation recovery variant.
pub const fn recovery_pratt_infix(
    variant: RecoveryVariantPlanData,
    lhs: &'static ElementSchemaData,
    left_binding_power: u8,
    operands: &'static [RecoveryPrattOperandPlanData],
) -> RecoveryPrattLedPlanData {
    RecoveryPrattLedPlanData {
        variant,
        lhs,
        position: RecoveryPrattLedPositionData::Infix { left_binding_power },
        operands,
    }
}

#[doc(hidden)]
/// Constructs one postfix left-denotation recovery variant.
pub const fn recovery_pratt_postfix(
    variant: RecoveryVariantPlanData,
    lhs: &'static ElementSchemaData,
    binding_power: u8,
    operands: &'static [RecoveryPrattOperandPlanData],
) -> RecoveryPrattLedPlanData {
    RecoveryPrattLedPlanData {
        variant,
        lhs,
        position: RecoveryPrattLedPositionData::Postfix { binding_power },
        operands,
    }
}

#[doc(hidden)]
/// Constructs one exact recursive Pratt operand association.
pub const fn recovery_pratt_operand(
    element_index: RecoveryElementIndexData,
    minimum_binding_power: u8,
) -> RecoveryPrattOperandPlanData {
    RecoveryPrattOperandPlanData {
        element_index,
        minimum_binding_power,
    }
}

#[doc(hidden)]
/// Constructs one frozen caller-FOLLOW guard contribution.
pub const fn recovery_pratt_guard(
    left_binding_power: u8,
    expected: &'static FollowSet,
) -> RecoveryPrattGuardPlanData {
    RecoveryPrattGuardPlanData {
        left_binding_power,
        expected,
    }
}

#[doc(hidden)]
/// Constructs one generated element recovery recipe.
pub const fn recovery_element_plan(
    schema: &'static ElementSchemaData,
    stop: RecoveryFollowData,
    kind: RecoveryElementKindData,
) -> RecoveryElementPlanData {
    RecoveryElementPlanData { schema, stop, kind }
}

#[doc(hidden)]
/// Constructs a required fixed-token recovery operation.
pub const fn recovery_fixed_kind(expected: &'static FollowSet) -> RecoveryElementKindData {
    RecoveryElementKindData::Fixed { expected }
}

#[doc(hidden)]
/// Constructs one fixed-token expectation inside an atomic group.
pub const fn recovery_fixed_plan(expected: &'static FollowSet) -> RecoveryFixedPlanData {
    RecoveryFixedPlanData { expected }
}

#[doc(hidden)]
/// Constructs an atomic optional fixed-token recovery operation.
pub const fn recovery_optional_fixed_kind(
    tokens: &'static [RecoveryFixedPlanData],
) -> RecoveryElementKindData {
    RecoveryElementKindData::OptionalFixed { tokens }
}

#[doc(hidden)]
/// Constructs one required fixed attachment operation.
pub const fn recovery_required_fixed_syntax(
    expected: &'static FollowSet,
) -> RecoveryFixedSyntaxPlanData {
    RecoveryFixedSyntaxPlanData::Required(RecoveryFixedPlanData { expected })
}

#[doc(hidden)]
/// Constructs one atomic optional fixed attachment operation.
pub const fn recovery_optional_fixed_syntax(
    tokens: &'static [RecoveryFixedPlanData],
) -> RecoveryFixedSyntaxPlanData {
    RecoveryFixedSyntaxPlanData::Optional(tokens)
}

#[doc(hidden)]
/// Constructs a boolean-presence recovery operation without a boolean value.
pub const fn recovery_fixed_presence_kind(
    before: &'static [RecoveryFixedSyntaxPlanData],
    sequence: &'static [RecoveryFixedPlanData],
    after: &'static [RecoveryFixedSyntaxPlanData],
) -> RecoveryElementKindData {
    RecoveryElementKindData::FixedPresence {
        before,
        sequence,
        after,
    }
}

#[doc(hidden)]
/// Constructs a semantic terminal recovery operation.
pub const fn recovery_semantic_kind(
    terminal: RecoveryTerminalPlanData,
    container: RecoveryContainerPlanData,
    before: &'static [RecoveryFixedSyntaxPlanData],
    after: &'static [RecoveryFixedSyntaxPlanData],
) -> RecoveryElementKindData {
    RecoveryElementKindData::Semantic {
        terminal,
        container,
        before,
        after,
    }
}

#[doc(hidden)]
/// Wraps a generated Node recipe as a construction-free semantic terminal.
pub const fn recovery_node_terminal(
    node: &'static RecoveryNodePlanData,
) -> RecoveryTerminalPlanData {
    RecoveryTerminalPlanData::Node(node)
}

#[doc(hidden)]
/// Wraps a content token schema as a construction-free semantic terminal.
pub const fn recovery_content_terminal(
    token: &'static TokenSchemaData,
) -> RecoveryTerminalPlanData {
    RecoveryTerminalPlanData::Content(token)
}

#[doc(hidden)]
/// Constructs an exactly-one recovery container.
pub const fn recovery_direct_container(
    restart: RecoveryFollowData,
    child_follow: RecoveryFollowData,
) -> RecoveryContainerPlanData {
    RecoveryContainerPlanData::Direct {
        restart,
        child_follow,
    }
}

#[doc(hidden)]
/// Constructs a zero-or-one recovery container.
pub const fn recovery_optional_container(
    restart: &'static FollowSet,
    child_follow: RecoveryFollowData,
) -> RecoveryContainerPlanData {
    RecoveryContainerPlanData::Optional {
        restart,
        child_follow,
    }
}

#[doc(hidden)]
/// Constructs a frozen repeated recovery container.
#[allow(clippy::too_many_arguments)]
pub const fn recovery_repeated_container(
    minimum: RecoveryMinimumData,
    presence: RecoveryRepetitionPresenceData,
    element: &'static FollowSet,
    separator: Option<&'static FollowSet>,
    trailing: RecoveryTrailingSeparatorData,
    stop: RecoveryFollowData,
    child_follow: RecoveryFollowData,
    minimum_failure: Option<RecoveryFollowData>,
    continuation_failure: RecoveryFollowData,
    after_separator_failure: Option<RecoveryFollowData>,
) -> RecoveryContainerPlanData {
    RecoveryContainerPlanData::Repeated {
        minimum,
        presence,
        element,
        separator,
        trailing,
        stop,
        child_follow,
        minimum_failure,
        continuation_failure,
        after_separator_failure,
    }
}

#[doc(hidden)]
/// Returns the dynamic caller FOLLOW recovery recipe.
pub const fn recovery_caller_follow() -> RecoveryFollowData {
    RecoveryFollowData::Caller
}

#[doc(hidden)]
/// Wraps a generated static FOLLOW set as a recovery recipe.
pub const fn recovery_static_follow(follow: &'static FollowSet) -> RecoveryFollowData {
    RecoveryFollowData::Static(follow)
}

#[doc(hidden)]
/// Unions a generated static FOLLOW set with the caller at runtime.
pub const fn recovery_static_and_caller_follow(follow: &'static FollowSet) -> RecoveryFollowData {
    RecoveryFollowData::StaticAndCaller(follow)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Stable identity assigned to one entered iterative recovery frame.
struct RecoveryFrameId(usize);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Index of the active frame within the explicit recovery stack.
struct RecoveryFrameIndex(usize);

impl RecoveryFrameIndex {
    /// Returns the raw index only at the stack access boundary.
    const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Absolute token-record boundary within one recovery input.
struct RecoveryInputIndex(usize);

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Source-order position of one generated element operation.
pub struct RecoveryElementIndexData(usize);

impl RecoveryElementIndexData {
    /// Returns the raw index only at a generated-plan lookup boundary.
    const fn get(self) -> usize {
        self.0
    }

    /// Advances permanently to the next generated element operation.
    fn advance(&mut self) {
        self.0 += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Number of semantic occurrences retained by a repeated container.
struct RecoveryOccurrenceCount(usize);

impl RecoveryOccurrenceCount {
    /// Returns the raw count only for a minimum comparison.
    const fn get(self) -> usize {
        self.0
    }

    /// Records one permanently retained occurrence.
    fn increment(&mut self) {
        self.0 += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Position of the next fixed-syntax operation in one generated sequence.
struct RecoverySyntaxIndex(usize);

impl RecoverySyntaxIndex {
    /// Returns the raw index only at a generated-plan lookup boundary.
    const fn get(self) -> usize {
        self.0
    }

    /// Returns the next irreversible syntax position.
    const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Position of the next token in one atomically committed fixed sequence.
struct RecoverySequenceIndex(usize);

impl RecoverySequenceIndex {
    /// Returns the raw index only at a generated sequence lookup boundary.
    const fn get(self) -> usize {
        self.0
    }

    /// Advances permanently to the next token in the committed sequence.
    const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Progress through a generated fixed-syntax operation and any committed optional sequence.
enum RecoverySyntaxState {
    /// Select and execute the next generated fixed-syntax operation.
    Operation(RecoverySyntaxIndex),
    /// Continue an optional sequence already selected by complete lookahead.
    CommittedOptional {
        /// Owning generated fixed-syntax operation.
        operation: RecoverySyntaxIndex,
        /// Next token to consume from that operation.
        token: RecoverySequenceIndex,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Selection or token progress for one atomic fixed sequence.
enum RecoverySequencePhase {
    /// Decide atomically whether the complete sequence is present.
    Select,
    /// Consume the next token of a sequence whose presence is committed.
    Committed(RecoverySequenceIndex),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Checked source byte boundary retained by a recovery frame.
struct RecoveryByteOffset(usize);

impl RecoveryByteOffset {
    /// Returns the raw byte offset only at a checked `Span` boundary.
    const fn get(self) -> usize {
        self.0
    }

    /// Extends this boundary monotonically.
    fn include(&mut self, candidate: u32) {
        self.0 = self.0.max(candidate as usize);
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Position within the iterative state machine for a repeated semantic site.
enum RecoveryRepeatedPhase {
    /// Select or recover the next semantic occurrence.
    NeedElement,
    /// Decide whether the container ends or continues with another occurrence.
    NeedContinuation,
    /// Require an element after a consumed non-trailing separator.
    AfterSeparator,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Active repeated-container state retained on its owning recovery frame.
struct RecoveryRepeatedState {
    /// Element-plan index that owns this container state.
    element: RecoveryElementIndexData,
    /// Number of semantic occurrences already retained.
    count: RecoveryOccurrenceCount,
    /// Next irreversible transition required for the container.
    phase: RecoveryRepeatedPhase,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Position within fixed syntax attached around one semantic value.
enum RecoverySemanticPhase {
    /// Interpret one fixed-prefix operation per guarded transition.
    Before(RecoverySyntaxState),
    /// Recover the semantic value after recognizing its fixed prefix.
    Value,
    /// Interpret one fixed-suffix operation per guarded transition.
    After(RecoverySyntaxState),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Active semantic-attachment state retained on its owning recovery frame.
struct RecoverySemanticState {
    /// Element-plan index that owns the attachment.
    element: RecoveryElementIndexData,
    /// Next irreversible transition required for the attachment.
    phase: RecoverySemanticPhase,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Position within a fixed-presence element submachine.
enum RecoveryPresencePhase {
    /// Interpret one prefix operation per guarded transition.
    Before(RecoverySyntaxState),
    /// Select the optional presence sequence atomically.
    Sequence(RecoverySequencePhase),
    /// Interpret one suffix operation per guarded transition.
    After(RecoverySyntaxState),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Active fixed-presence state retained on its owning frame.
struct RecoveryPresenceState {
    /// Element-plan index that owns this presence state.
    element: RecoveryElementIndexData,
    /// Next irreversible presence transition.
    phase: RecoveryPresencePhase,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Active standalone optional fixed sequence retained on its owning frame.
struct RecoveryOptionalFixedState {
    /// Element-plan index that owns the optional sequence.
    element: RecoveryElementIndexData,
    /// Atomic selection or committed token progress.
    phase: RecoverySequencePhase,
}

/// Frozen uncommitted enum data retained without an anonymous tuple.
#[derive(Clone, Copy)]
struct RecoveryPendingDispatch {
    /// Generated enum variants in dispatch-target order.
    variants: &'static [RecoveryVariantPlanData],
    /// Frozen predictive dispatch root.
    dispatch: &'static RecoveryDispatchNodeData,
    /// Union of prefixes capable of restarting selection.
    restart: &'static FollowSet,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Exact Pratt minimum binding power retained in a guarded frame state.
struct RecoveryBindingPower(u8);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Irreversible control phase of one construction-free Pratt frame.
enum RecoveryPrattPhase {
    /// Select and permanently commit one null-denotation variant.
    NudDispatch,
    /// Interpret the committed null-denotation body.
    NudElements,
    /// Select one qualifying extender or close the expression.
    LedDispatch,
    /// Interpret one committed extender tail around the retained lhs.
    LedElements,
    /// The final recovered expression is ready to leave the frame.
    Complete,
}

/// Binding-power-aware state owned by one Pratt recovery frame.
struct RecoveryPrattState {
    /// Immutable generated Pratt recipe for this application.
    plan: &'static RecoveryPrattPlanData,
    /// Exact threshold supplied by the parse site that entered this frame.
    minimum_binding_power: RecoveryBindingPower,
    /// Next irreversible Pratt transition.
    phase: RecoveryPrattPhase,
    /// Direct recursive operands of the committed branch, empty for atoms.
    active_operands: &'static [RecoveryPrattOperandPlanData],
    /// Recognized expression accumulated between extender selections.
    lhs: Option<RecoveryNode>,
}

/// One explicit iterative recovery frame. It contains no authored value,
/// builder, construction callback, or typed parser state.
struct RecoveryFrame {
    /// Stable runtime identity used by the progress trace without pointer casting.
    id: RecoveryFrameId,
    /// Immutable generated recipe for the active Node application.
    plan: &'static RecoveryNodePlanData,
    /// Elements of the struct or committed enum variant being interpreted.
    elements: &'static [RecoveryElementPlanData],
    /// Variant selected by predictive dispatch, if this frame owns an enum.
    committed_variant: Option<&'static VariantSchemaData>,
    /// Uncommitted enum alternatives, trie, and synchronization restart set.
    pending_dispatch: Option<RecoveryPendingDispatch>,
    /// Binding-power state when this frame interprets a Pratt application.
    pratt: Option<RecoveryPrattState>,
    /// Source-order index of the next element operation.
    next_element: RecoveryElementIndexData,
    /// FOLLOW set supplied by the parent application at frame entry.
    caller_follow: FollowSet,
    /// First byte covered by this Node occurrence.
    start: RecoveryByteOffset,
    /// Furthest byte covered by recognized or recovered parts.
    end: RecoveryByteOffset,
    /// Source-order recovery projection accumulated for this Node.
    parts: Vec<RecoveryPart>,
    /// Parent element that receives this frame when it closes.
    return_element: Option<&'static RecoveryElementPlanData>,
    /// Repeated-container state when the current element has not closed.
    repeated: Option<RecoveryRepeatedState>,
    /// Fixed-attachment state when the current semantic element has not closed.
    semantic: Option<RecoverySemanticState>,
    /// Fixed-presence state when the current element has not closed.
    presence: Option<RecoveryPresenceState>,
    /// Standalone optional fixed sequence when its current element has not closed.
    optional_fixed: Option<RecoveryOptionalFixedState>,
}

impl RecoveryFrame {
    /// Opens one struct, predictive-enum, or Pratt frame from an immutable recipe.
    fn new(
        id: RecoveryFrameId,
        plan: &'static RecoveryNodePlanData,
        caller_follow: FollowSet,
        start: usize,
        return_element: Option<&'static RecoveryElementPlanData>,
        minimum_binding_power: RecoveryBindingPower,
    ) -> Self {
        let (elements, committed_variant, pending_dispatch, pratt) = match &plan.body {
            RecoveryNodeBodyPlanData::Struct { elements, .. } => (*elements, None, None, None),
            RecoveryNodeBodyPlanData::Enum {
                variants,
                dispatch,
                restart,
            } => (
                &[][..],
                None,
                Some(RecoveryPendingDispatch {
                    variants,
                    dispatch,
                    restart,
                }),
                None,
            ),
            RecoveryNodeBodyPlanData::Pratt(pratt) => (
                &[][..],
                None,
                None,
                Some(RecoveryPrattState {
                    plan: pratt,
                    minimum_binding_power,
                    phase: RecoveryPrattPhase::NudDispatch,
                    active_operands: &[],
                    lhs: None,
                }),
            ),
        };
        Self {
            id,
            plan,
            elements,
            committed_variant,
            pending_dispatch,
            pratt,
            next_element: RecoveryElementIndexData(0),
            caller_follow,
            start: RecoveryByteOffset(start),
            end: RecoveryByteOffset(start),
            parts: Vec::new(),
            return_element,
            repeated: None,
            semantic: None,
            presence: None,
            optional_fixed: None,
        }
    }

    /// Seals the accumulated frame into the public construction-free projection.
    fn finish(self) -> Result<RecoveryNode, ()> {
        let extent = Span::new(self.start.get(), self.end.get()).map_err(|_| ())?;
        Ok(RecoveryNode {
            schema: self.plan.schema,
            committed_variant: self.committed_variant,
            extent,
            parts: self.parts.into_boxed_slice(),
        })
    }

    /// Retains one source-order part and extends the frame to cover its range.
    fn push_part(&mut self, part: RecoveryPart) {
        let end = match &part {
            RecoveryPart::Element(element) => element.span.end(),
            RecoveryPart::Separator(separator) => separator.span.end(),
            RecoveryPart::Missing(missing) => missing.at.end(),
            RecoveryPart::Skipped(skipped) => skipped.span.end(),
        };
        self.end.include(end);
        self.parts.push(part);
    }

    /// Retains one recognized token under the supplied element schema.
    fn push_token(
        &mut self,
        element: &'static RecoveryElementPlanData,
        schema: &'static TokenSchemaData,
        span: Span,
    ) {
        self.push_part(RecoveryPart::Element(RecoveryElement {
            schema: element.schema,
            span,
            value: RecoveryValue::Token(RecoveryToken { schema, span }),
        }));
    }

    /// Advances the active semantic attachment to its fixed suffix.
    fn finish_semantic_value(&mut self) {
        self.semantic
            .as_mut()
            .expect("semantic element retains its state")
            .phase =
            RecoverySemanticPhase::After(RecoverySyntaxState::Operation(RecoverySyntaxIndex(0)));
    }

    /// Permanently closes an active repeated container and advances its attachment.
    fn finish_repeated_value(&mut self) {
        self.repeated = None;
        self.finish_semantic_value();
    }
}

impl RecoveryFollowData {
    /// Materializes a frozen static/caller FOLLOW recipe for one invocation.
    fn evaluate(self, caller: &FollowSet) -> Result<FollowSet, ()> {
        match self {
            Self::Caller => Ok(caller.clone()),
            Self::Static(follow) => Ok(follow.clone()),
            Self::StaticAndCaller(follow) => follow.union(caller).map_err(|_| ()),
        }
    }
}

/// Resolves one expected set to stable grammar atom names for diagnostics.
fn recovery_expected_names(
    grammar: &'static GrammarInfo,
    expected: &FollowSet,
) -> Box<[&'static str]> {
    expected
        .expected()
        .iter()
        .filter_map(|atom| grammar.atom_name(*atom))
        .collect()
}

/// Checks an atomic fixed-token sequence without moving the input cursor.
fn recovery_fixed_sequence_matches(input: &Input<'_>, tokens: &[RecoveryFixedPlanData]) -> bool {
    tokens.iter().enumerate().all(|(offset, token)| {
        input
            .peek(offset)
            .is_some_and(|record| token.expected.contains(record.kind))
    })
}

/// Consumes one already-selected fixed token into its owning element projection.
fn recovery_consume_fixed(
    input: &mut Input<'_>,
    plan: &'static RecoveryPlanData,
    element: &'static RecoveryElementPlanData,
    expected: &FollowSet,
    frame: &mut RecoveryFrame,
) -> Result<(), ()> {
    let record = input.advance().ok_or(())?;
    if !expected.contains(record.kind) || input.source().get(record.span.range()).is_none() {
        return Err(());
    }
    let schema = plan
        .tokens
        .get(usize::from(record.kind))
        .copied()
        .flatten()
        .ok_or(())?;
    frame.push_token(element, schema, record.span);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Result of executing at most one fixed-syntax operation.
enum RecoveryFixedSyntaxStep {
    /// Another generated operation or committed optional token remains.
    Pending(RecoverySyntaxState),
    /// The fixed-syntax sequence has permanently closed.
    Complete,
}

/// Advances past one complete fixed-syntax operation without executing another one.
fn recovery_finish_fixed_syntax_operation(
    syntax: &[RecoveryFixedSyntaxPlanData],
    operation: RecoverySyntaxIndex,
) -> RecoveryFixedSyntaxStep {
    let next = operation.next();
    if next.get() == syntax.len() {
        RecoveryFixedSyntaxStep::Complete
    } else {
        RecoveryFixedSyntaxStep::Pending(RecoverySyntaxState::Operation(next))
    }
}

#[allow(clippy::too_many_arguments)]
/// Interprets at most one required or atomic-optional fixed-syntax operation.
fn recovery_fixed_syntax<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    element: &'static RecoveryElementPlanData,
    syntax: &[RecoveryFixedSyntaxPlanData],
    state: RecoverySyntaxState,
    frame: &mut RecoveryFrame,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<RecoveryFixedSyntaxStep, ()> {
    match state {
        RecoverySyntaxState::Operation(index) => {
            let Some(operation) = syntax.get(index.get()) else {
                return Ok(RecoveryFixedSyntaxStep::Complete);
            };
            match operation {
                RecoveryFixedSyntaxPlanData::Required(token) => {
                    if input
                        .peek(0)
                        .is_some_and(|record| token.expected.contains(record.kind))
                    {
                        recovery_consume_fixed(input, plan, element, token.expected, frame)?;
                    } else {
                        let missing = recovery_missing(
                            input,
                            plan.grammar,
                            owner,
                            element,
                            token.expected,
                            diagnostics,
                        )?;
                        frame.push_part(missing);
                    }
                    Ok(recovery_finish_fixed_syntax_operation(syntax, index))
                }
                RecoveryFixedSyntaxPlanData::Optional(tokens) => {
                    let Some(first) = tokens.first() else {
                        return Ok(recovery_finish_fixed_syntax_operation(syntax, index));
                    };
                    if !recovery_fixed_sequence_matches(input, tokens) {
                        return Ok(recovery_finish_fixed_syntax_operation(syntax, index));
                    }
                    recovery_consume_fixed(input, plan, element, first.expected, frame)?;
                    let next = RecoverySequenceIndex(1);
                    if next.get() == tokens.len() {
                        Ok(recovery_finish_fixed_syntax_operation(syntax, index))
                    } else {
                        Ok(RecoveryFixedSyntaxStep::Pending(
                            RecoverySyntaxState::CommittedOptional {
                                operation: index,
                                token: next,
                            },
                        ))
                    }
                }
            }
        }
        RecoverySyntaxState::CommittedOptional { operation, token } => {
            let Some(RecoveryFixedSyntaxPlanData::Optional(tokens)) = syntax.get(operation.get())
            else {
                return Err(());
            };
            let next_token = tokens.get(token.get()).ok_or(())?;
            recovery_consume_fixed(input, plan, element, next_token.expected, frame)?;
            let next = token.next();
            if next.get() == tokens.len() {
                Ok(recovery_finish_fixed_syntax_operation(syntax, operation))
            } else {
                Ok(RecoveryFixedSyntaxStep::Pending(
                    RecoverySyntaxState::CommittedOptional {
                        operation,
                        token: next,
                    },
                ))
            }
        }
    }
}

/// Emits one missing part and attaches or creates its owner/site diagnostic.
fn recovery_missing<'input>(
    input: &Input<'input>,
    grammar: &'static GrammarInfo,
    owner: &'static NodeSchemaData,
    element: &'static RecoveryElementPlanData,
    expected: &FollowSet,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<RecoveryPart, ()> {
    let position = input.byte_position();
    let at = Span::new(position, position).map_err(|_| ())?;
    let action = RecoveryAction::Missing { at };
    if let Some(seed) = diagnostics.iter_mut().find(|diagnostic| {
        diagnostic.action.is_none()
            && matches!(
                diagnostic.kind,
                ParseDiagnosticKind::UnexpectedToken | ParseDiagnosticKind::UnexpectedEof
            )
    }) {
        seed.action = Some(action);
        seed.owner = Some(owner);
        seed.site = Some(element.schema);
    } else {
        diagnostics.push(ParseDiagnostic {
            kind: ParseDiagnosticKind::MissingSyntax,
            span: at,
            text: "",
            found: None,
            expected: recovery_expected_names(grammar, expected),
            message: "required syntax is missing".into(),
            action: Some(action),
            owner: Some(owner),
            site: Some(element.schema),
        });
    }
    Ok(RecoveryPart::Missing(RecoveryMissing {
        element: element.schema,
        at,
        expected: recovery_expected_names(grammar, expected),
    }))
}

/// Consumes one maximal invalid run and retains its items and diagnostic action.
fn recovery_skip<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    site: Option<&'static ElementSchemaData>,
    restart: &FollowSet,
    stop: &FollowSet,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<Option<RecoveryPart>, ()> {
    let start_cursor = input.cursor();
    let mut items = Vec::new();
    while let Some(record) = input.peek(0) {
        if restart.contains(record.kind) || stop.contains(record.kind) {
            break;
        }
        if input.source().get(record.span.range()).is_none() {
            return Err(());
        }
        input.advance();
        let schema = if record.kind == plan.grammar.lexical_error_kind {
            None
        } else {
            plan.tokens.get(usize::from(record.kind)).copied().flatten()
        };
        items.push(RecoveryInputItem {
            schema,
            span: record.span,
        });
    }
    if input.cursor() == start_cursor {
        return Ok(None);
    }
    let first = items
        .first()
        .expect("a moved recovery cursor retains an item");
    let last = items
        .last()
        .expect("a moved recovery cursor retains an item");
    let span = Span::new(first.span.start() as usize, last.span.end() as usize).map_err(|_| ())?;
    let action = RecoveryAction::Skipped { span };
    let mut attached = false;
    for diagnostic in diagnostics.iter_mut().filter(|diagnostic| {
        diagnostic.action.is_none()
            && diagnostic.span.start() >= span.start()
            && diagnostic.span.end() <= span.end()
    }) {
        diagnostic.action = Some(action);
        diagnostic.owner = Some(owner);
        diagnostic.site = site;
        attached = true;
    }
    if !attached {
        let text = input.source().get(span.range()).ok_or(())?;
        let found = items
            .first()
            .and_then(|item| item.schema)
            .map(|schema| schema.kind);
        diagnostics.push(ParseDiagnostic {
            kind: ParseDiagnosticKind::SkippedSyntax,
            span,
            text,
            found,
            expected: recovery_expected_names(plan.grammar, restart),
            message: "skipped unexpected input".into(),
            action: Some(action),
            owner: Some(owner),
            site,
        });
    }
    Ok(Some(RecoveryPart::Skipped(RecoverySkipped {
        span,
        items: items.into_boxed_slice(),
    })))
}

/// Emits one missing transition into its frame with a checked diagnostic site.
#[allow(clippy::too_many_arguments)]
fn recovery_push_missing<'input>(
    input: &Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    element: &'static RecoveryElementPlanData,
    expected: &FollowSet,
    frame: &mut RecoveryFrame,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
    invalid_message: &'static str,
) -> Result<(), ExplorationError<'input>> {
    let part = recovery_missing(input, plan.grammar, owner, element, expected, diagnostics)
        .map_err(|()| {
            recovery_fatal(
                input.source(),
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvalidTokenSpan,
                invalid_message,
            )
        })?;
    frame.push_part(part);
    Ok(())
}

/// Consumes one mandatory synchronization run directly into its owning frame.
#[allow(clippy::too_many_arguments)]
fn recovery_push_skipped<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    site: Option<&'static ElementSchemaData>,
    restart: &FollowSet,
    stop: &FollowSet,
    frame: &mut RecoveryFrame,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
    invalid_message: &'static str,
) -> Result<(), ExplorationError<'input>> {
    let skipped = recovery_skip(input, plan, owner, site, restart, stop, diagnostics)
        .map_err(|()| {
            recovery_fatal(
                input.source(),
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvalidTokenSpan,
                invalid_message,
            )
        })?
        .ok_or_else(|| {
            let position = input.byte_position();
            let span = Span::new(position, position).unwrap_or_else(|_| Span::new(0, 0).unwrap());
            recovery_fatal(
                input.source(),
                span,
                ParseDiagnosticKind::InvariantViolation,
                "a mandatory recovery synchronization did not consume input",
            )
        })?;
    frame.push_part(skipped);
    Ok(())
}

/// Consumes one selected content record into its element projection.
fn recovery_push_content<'input>(
    input: &mut Input<'input>,
    element: &'static RecoveryElementPlanData,
    schema: &'static TokenSchemaData,
    frame: &mut RecoveryFrame,
    invalid_message: &'static str,
) -> Result<(), ExplorationError<'input>> {
    let record = input.advance().expect("selected recovery token exists");
    if input.source().get(record.span.range()).is_none() {
        return Err(recovery_fatal(
            input.source(),
            record.span,
            ParseDiagnosticKind::InvalidTokenSpan,
            invalid_message,
        ));
    }
    frame.push_token(element, schema, record.span);
    Ok(())
}

/// Advances one required fixed element by matching, closing missing, or synchronizing.
#[allow(clippy::too_many_arguments)]
fn recovery_fixed_element_transition<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    element: &'static RecoveryElementPlanData,
    expected: &FollowSet,
    stop: &FollowSet,
    frame: &mut RecoveryFrame,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<(), ExplorationError<'input>> {
    if input
        .peek(0)
        .is_some_and(|record| expected.contains(record.kind))
    {
        let record = input.advance().expect("matched recovery token exists");
        if input.source().get(record.span.range()).is_none() {
            return Err(recovery_fatal(
                input.source(),
                record.span,
                ParseDiagnosticKind::InvalidTokenSpan,
                "recovery token span is not a UTF-8 source range",
            ));
        }
        let Some(schema) = element.schema.token else {
            return Err(recovery_fatal(
                input.source(),
                record.span,
                ParseDiagnosticKind::InvariantViolation,
                "fixed recovery element lacks its token schema",
            ));
        };
        frame.next_element.advance();
        frame.push_token(element, schema, record.span);
    } else if input.peek(0).is_none()
        || input
            .peek(0)
            .is_some_and(|record| stop.contains(record.kind))
    {
        recovery_push_missing(
            input,
            plan,
            owner,
            element,
            expected,
            frame,
            diagnostics,
            "missing recovery position exceeds checked span range",
        )?;
        frame.next_element.advance();
    } else {
        recovery_push_skipped(
            input,
            plan,
            owner,
            Some(element.schema),
            expected,
            stop,
            frame,
            diagnostics,
            "skipped recovery input is not a checked UTF-8 range",
        )?;
    }
    Ok(())
}

/// Interprets an atomic optional fixed group and permanently closes its site.
fn recovery_optional_fixed_transition<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    element: &'static RecoveryElementPlanData,
    tokens: &[RecoveryFixedPlanData],
    frame: &mut RecoveryFrame,
) -> Result<(), ExplorationError<'input>> {
    let element_index = frame.next_element;
    let state = frame
        .optional_fixed
        .get_or_insert(RecoveryOptionalFixedState {
            element: element_index,
            phase: RecoverySequencePhase::Select,
        });
    debug_assert_eq!(state.element, element_index);
    let phase = state.phase;
    let selected = match phase {
        RecoverySequencePhase::Select => {
            if !recovery_fixed_sequence_matches(input, tokens) {
                frame.optional_fixed = None;
                frame.next_element.advance();
                return Ok(());
            }
            RecoverySequenceIndex(0)
        }
        RecoverySequencePhase::Committed(index) => index,
    };
    let Some(token) = tokens.get(selected.get()) else {
        frame.optional_fixed = None;
        frame.next_element.advance();
        return Ok(());
    };
    recovery_consume_fixed(input, plan, element, token.expected, frame).map_err(|()| {
        recovery_fatal(
            input.source(),
            Span::new(0, 0).unwrap(),
            ParseDiagnosticKind::InvalidTokenSpan,
            "optional fixed recovery token is not a checked input record",
        )
    })?;
    let next = selected.next();
    if next.get() == tokens.len() {
        frame.optional_fixed = None;
        frame.next_element.advance();
    } else {
        frame
            .optional_fixed
            .as_mut()
            .expect("optional fixed state exists")
            .phase = RecoverySequencePhase::Committed(next);
    }
    Ok(())
}

/// Interprets fixed syntax retained by a presence field without constructing its boolean.
#[allow(clippy::too_many_arguments)]
fn recovery_fixed_presence_transition<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    element: &'static RecoveryElementPlanData,
    before: &[RecoveryFixedSyntaxPlanData],
    sequence: &[RecoveryFixedPlanData],
    after: &[RecoveryFixedSyntaxPlanData],
    frame: &mut RecoveryFrame,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<(), ExplorationError<'input>> {
    let element_index = frame.next_element;
    let state = frame.presence.get_or_insert(RecoveryPresenceState {
        element: element_index,
        phase: RecoveryPresencePhase::Before(RecoverySyntaxState::Operation(RecoverySyntaxIndex(
            0,
        ))),
    });
    debug_assert_eq!(state.element, element_index);
    match state.phase {
        RecoveryPresencePhase::Before(index) => {
            let step = recovery_fixed_syntax(
                input,
                plan,
                owner,
                element,
                before,
                index,
                frame,
                diagnostics,
            )
            .map_err(|()| {
                recovery_fatal(
                    input.source(),
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "fixed-presence prefix is not a checked input record",
                )
            })?;
            frame
                .presence
                .as_mut()
                .expect("presence state exists")
                .phase = match step {
                RecoveryFixedSyntaxStep::Pending(next) => RecoveryPresencePhase::Before(next),
                RecoveryFixedSyntaxStep::Complete => {
                    RecoveryPresencePhase::Sequence(RecoverySequencePhase::Select)
                }
            };
        }
        RecoveryPresencePhase::Sequence(phase) => {
            let selected = match phase {
                RecoverySequencePhase::Select => {
                    if !recovery_fixed_sequence_matches(input, sequence) {
                        frame
                            .presence
                            .as_mut()
                            .expect("presence state exists")
                            .phase = RecoveryPresencePhase::After(RecoverySyntaxState::Operation(
                            RecoverySyntaxIndex(0),
                        ));
                        return Ok(());
                    }
                    RecoverySequenceIndex(0)
                }
                RecoverySequencePhase::Committed(index) => index,
            };
            let Some(token) = sequence.get(selected.get()) else {
                frame
                    .presence
                    .as_mut()
                    .expect("presence state exists")
                    .phase = RecoveryPresencePhase::After(RecoverySyntaxState::Operation(
                    RecoverySyntaxIndex(0),
                ));
                return Ok(());
            };
            recovery_consume_fixed(input, plan, element, token.expected, frame).map_err(|()| {
                recovery_fatal(
                    input.source(),
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "fixed-presence token is not a checked input record",
                )
            })?;
            let next = selected.next();
            frame
                .presence
                .as_mut()
                .expect("presence state exists")
                .phase = if next.get() == sequence.len() {
                RecoveryPresencePhase::After(RecoverySyntaxState::Operation(RecoverySyntaxIndex(0)))
            } else {
                RecoveryPresencePhase::Sequence(RecoverySequencePhase::Committed(next))
            };
        }
        RecoveryPresencePhase::After(index) => {
            let step = recovery_fixed_syntax(
                input,
                plan,
                owner,
                element,
                after,
                index,
                frame,
                diagnostics,
            )
            .map_err(|()| {
                recovery_fatal(
                    input.source(),
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "fixed-presence suffix is not a checked input record",
                )
            })?;
            match step {
                RecoveryFixedSyntaxStep::Pending(next) => {
                    frame
                        .presence
                        .as_mut()
                        .expect("presence state exists")
                        .phase = RecoveryPresencePhase::After(next);
                }
                RecoveryFixedSyntaxStep::Complete => {
                    frame.presence = None;
                    frame.next_element.advance();
                }
            }
        }
    }
    Ok(())
}

/// Enters an attached semantic value or closes it by interpreting its fixed suffix.
#[allow(clippy::too_many_arguments)]
fn recovery_semantic_attachment_transition<'input>(
    input: &mut Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    element: &'static RecoveryElementPlanData,
    element_index: RecoveryElementIndexData,
    before: &[RecoveryFixedSyntaxPlanData],
    after: &[RecoveryFixedSyntaxPlanData],
    frame: &mut RecoveryFrame,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<bool, ExplorationError<'input>> {
    if frame
        .semantic
        .is_none_or(|state| state.element != element_index)
    {
        frame.semantic = Some(RecoverySemanticState {
            element: element_index,
            phase: RecoverySemanticPhase::Before(RecoverySyntaxState::Operation(
                RecoverySyntaxIndex(0),
            )),
        });
    }
    let phase = frame.semantic.expect("semantic state exists").phase;
    match phase {
        RecoverySemanticPhase::Before(index) => {
            let step = recovery_fixed_syntax(
                input,
                plan,
                owner,
                element,
                before,
                index,
                frame,
                diagnostics,
            )
            .map_err(|()| {
                recovery_fatal(
                    input.source(),
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "semantic attachment prefix is not a checked input record",
                )
            })?;
            frame
                .semantic
                .as_mut()
                .expect("semantic state exists")
                .phase = match step {
                RecoveryFixedSyntaxStep::Pending(next) => RecoverySemanticPhase::Before(next),
                RecoveryFixedSyntaxStep::Complete => RecoverySemanticPhase::Value,
            };
            return Ok(true);
        }
        RecoverySemanticPhase::After(index) => {
            let step = recovery_fixed_syntax(
                input,
                plan,
                owner,
                element,
                after,
                index,
                frame,
                diagnostics,
            )
            .map_err(|()| {
                recovery_fatal(
                    input.source(),
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "semantic attachment suffix is not a checked input record",
                )
            })?;
            match step {
                RecoveryFixedSyntaxStep::Pending(next) => {
                    frame
                        .semantic
                        .as_mut()
                        .expect("semantic state exists")
                        .phase = RecoverySemanticPhase::After(next);
                }
                RecoveryFixedSyntaxStep::Complete => {
                    frame.semantic = None;
                    frame.next_element.advance();
                }
            }
            return Ok(true);
        }
        RecoverySemanticPhase::Value => {}
    }
    Ok(false)
}

#[derive(Clone, Copy)]
/// Distinguishes an early unique recovery commitment from a strict trie terminal.
enum RecoveryDispatchSelection {
    /// Every terminal below the current prefix selects this variant.
    RecoveryCommit(RecoveryVariantIndexData),
    /// The complete frozen selection condition chose this variant.
    Terminal(RecoveryVariantIndexData),
}

impl RecoveryDispatchSelection {
    /// Returns the selected variant identity at the generated table boundary.
    const fn variant(self) -> RecoveryVariantIndexData {
        match self {
            Self::RecoveryCommit(variant) | Self::Terminal(variant) => variant,
        }
    }
}

/// Selects a committed variant through the frozen bounded dispatch trie.
fn recovery_dispatch_select(
    input: &Input<'_>,
    plan: &'static RecoveryPlanData,
    mut node: &'static RecoveryDispatchNodeData,
    caller_follow: &FollowSet,
) -> Result<Option<RecoveryDispatchSelection>, ()> {
    let mut depth = 0;
    loop {
        let mut selected = None;
        if let Some(record) = input.peek(depth) {
            let text = input.source().get(record.span.range()).ok_or(())?;
            selected = node.edges.iter().find(|edge| {
                if !edge.expected.contains(record.kind) {
                    return false;
                }
                let equals = |candidate: &str| {
                    if matches!(
                        plan.text_equivalence,
                        RecoveryTextEquivalenceData::AsciiInsensitive
                    ) {
                        text.eq_ignore_ascii_case(candidate)
                    } else {
                        text == candidate
                    }
                };
                match edge.text {
                    RecoveryTextPredicateData::Any => true,
                    RecoveryTextPredicateData::Exact(candidate) => equals(candidate),
                    RecoveryTextPredicateData::OneOf(candidates) => {
                        candidates.iter().any(|candidate| equals(candidate))
                    }
                    RecoveryTextPredicateData::NotIn(candidates) => {
                        !candidates.iter().any(|candidate| equals(candidate))
                    }
                }
            });
        }
        if let Some(edge) = selected {
            node = edge.next;
            depth += 1;
            continue;
        }
        if let Some(target) = node.target
            && (matches!(
                target.condition,
                RecoveryDispatchTerminalConditionData::Unconditional
            ) || input.peek(depth).is_none()
                || input
                    .peek(depth)
                    .is_some_and(|record| caller_follow.contains(record.kind)))
        {
            return Ok(Some(RecoveryDispatchSelection::Terminal(target.variant)));
        }
        if let Some(variant) = node.commit {
            return Ok(Some(RecoveryDispatchSelection::RecoveryCommit(variant)));
        }
        return Ok(None);
    }
}

/// Records a stopped enum whose frozen dispatch trie could not commit a variant.
fn recovery_unresolved<'input>(
    input: &Input<'input>,
    plan: &'static RecoveryPlanData,
    owner: &'static NodeSchemaData,
    restart: &FollowSet,
    message: &'static str,
    diagnostics: &mut Vec<ParseDiagnostic<'input>>,
) -> Result<Span, ()> {
    let position = input.byte_position();
    let at = Span::new(position, position).map_err(|_| ())?;
    let action = RecoveryAction::Missing { at };
    let (seed_kind, seed_span) = input
        .peek(0)
        .map_or((ParseDiagnosticKind::UnexpectedEof, at), |record| {
            (ParseDiagnosticKind::UnexpectedToken, record.span)
        });
    if let Some(seed) = diagnostics.iter_mut().find(|diagnostic| {
        diagnostic.action.is_none()
            && diagnostic.kind == seed_kind
            && diagnostic.span == seed_span
            && diagnostic.site.is_none()
    }) {
        seed.action = Some(action);
        seed.owner = Some(owner);
        seed.site = None;
    }
    diagnostics.push(ParseDiagnostic {
        kind: ParseDiagnosticKind::UnresolvedAlternative,
        span: at,
        text: "",
        found: None,
        expected: recovery_expected_names(plan.grammar, restart),
        message: message.into(),
        action: Some(action),
        owner: Some(owner),
        site: None,
    });
    Ok(at)
}

/// Stable-sorts diagnostics and removes only exact recovery diagnostic duplicates.
fn recovery_finalize_diagnostics<'input>(
    diagnostics: Vec<ParseDiagnostic<'input>>,
) -> Box<[ParseDiagnostic<'input>]> {
    let mut ordered = diagnostics.into_iter().enumerate().collect::<Vec<_>>();
    ordered.sort_by_key(|(ordinal, diagnostic)| (diagnostic.span.start(), *ordinal));
    let mut unique: Vec<ParseDiagnostic<'input>> = Vec::with_capacity(ordered.len());
    for (_, diagnostic) in ordered {
        let duplicate = unique.iter().any(|prior| {
            prior.kind == diagnostic.kind
                && prior.span == diagnostic.span
                && prior.found == diagnostic.found
                && prior.expected == diagnostic.expected
                && prior.action == diagnostic.action
                && match (prior.owner, diagnostic.owner) {
                    (Some(left), Some(right)) => std::ptr::eq(left, right),
                    (None, None) => true,
                    _ => false,
                }
                && match (prior.site, diagnostic.site) {
                    (Some(left), Some(right)) => std::ptr::eq(left, right),
                    (None, None) => true,
                    _ => false,
                }
        });
        if !duplicate {
            unique.push(diagnostic);
        }
    }
    unique.into_boxed_slice()
}

/// Creates a fatal exploration error for a violated runtime plan invariant.
fn recovery_fatal<'input>(
    source: &'input str,
    span: Span,
    kind: ParseDiagnosticKind,
    message: &'static str,
) -> ExplorationError<'input> {
    ExplorationError {
        source,
        diagnostics: vec![ParseDiagnostic {
            kind,
            span,
            text: source.get(span.range()).unwrap_or(""),
            found: None,
            expected: Box::new([]),
            message: message.into(),
            action: None,
            owner: None,
            site: None,
        }]
        .into_boxed_slice(),
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Typed substate of one guarded generated element transition.
enum RecoveryElementProgress {
    /// No container or attachment submachine is active.
    Ready,
    /// A semantic fixed attachment is active.
    Semantic(RecoverySemanticPhase),
    /// A repeated container is active.
    Repeated(RecoveryOccurrenceCount, RecoveryRepeatedPhase),
    /// A fixed-presence element is active.
    Presence(RecoveryPresencePhase),
    /// A standalone optional fixed sequence is selected or committed.
    OptionalFixed(RecoverySequencePhase),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Typed state of the active recovery frame.
enum RecoveryFrameProgress {
    /// Predictive enum dispatch has not yet committed.
    Dispatch,
    /// One generated element and its active substate.
    Element(RecoveryElementIndexData, RecoveryElementProgress),
    /// One exact binding-power Pratt transition.
    Pratt {
        /// Minimum binding power supplied at frame entry.
        minimum_binding_power: RecoveryBindingPower,
        /// Current irreversible Pratt phase.
        phase: RecoveryPrattPhase,
        /// Active element and substate for a committed variant body.
        element: Option<(RecoveryElementIndexData, RecoveryElementProgress)>,
    },
    /// Every generated element has permanently closed.
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Identity of one guarded same-cursor interpreter state.
struct RecoveryProgressKey {
    /// Absolute input record boundary.
    input: RecoveryInputIndex,
    /// Stable identity of the entered runtime frame.
    frame: RecoveryFrameId,
    /// Typed transition state within that frame.
    state: RecoveryFrameProgress,
}

impl RecoveryFrame {
    /// Returns the typed substate of the active generated element, when any.
    fn element_progress(&self) -> RecoveryElementProgress {
        if let Some(state) = self.optional_fixed {
            RecoveryElementProgress::OptionalFixed(state.phase)
        } else if let Some(state) = self.presence {
            RecoveryElementProgress::Presence(state.phase)
        } else if let Some(state) = self.repeated {
            RecoveryElementProgress::Repeated(state.count, state.phase)
        } else if let Some(state) = self.semantic {
            RecoveryElementProgress::Semantic(state.phase)
        } else {
            RecoveryElementProgress::Ready
        }
    }

    /// Returns the complete typed state guarded before the next transition.
    fn progress(&self) -> RecoveryFrameProgress {
        if self.pending_dispatch.is_some() {
            return RecoveryFrameProgress::Dispatch;
        }
        if let Some(pratt) = &self.pratt {
            let element = matches!(
                pratt.phase,
                RecoveryPrattPhase::NudElements | RecoveryPrattPhase::LedElements
            )
            .then(|| (self.next_element, self.element_progress()));
            return RecoveryFrameProgress::Pratt {
                minimum_binding_power: pratt.minimum_binding_power,
                phase: pratt.phase,
                element,
            };
        }
        if self.next_element.get() == self.elements.len() {
            return RecoveryFrameProgress::Complete;
        }
        RecoveryFrameProgress::Element(self.next_element, self.element_progress())
    }
}

/// Mutable progress proof retained by the private recovery machine.
struct RecoveryProgress {
    /// Finite record count fixed before recovery starts.
    input_records: usize,
    /// Number of runtime frames assigned stable identities.
    entered_frames: usize,
    /// Number of frames permanently removed from the stack.
    closed_frames: usize,
    /// Sum of conservative state slots for every entered frame.
    state_slots: usize,
    /// Every guarded state identity; duplicate insertion is fatal.
    guarded: BTreeSet<RecoveryProgressKey>,
}

impl RecoveryProgress {
    /// Starts a trace against one fixed set of input records.
    fn new(input_records: usize) -> Self {
        Self {
            input_records,
            entered_frames: 0,
            closed_frames: 0,
            state_slots: 0,
            guarded: BTreeSet::new(),
        }
    }

    /// Assigns one stable frame identity and extends the finite state bound.
    fn enter_frame(&mut self, plan: &'static RecoveryNodePlanData) -> RecoveryFrameId {
        let id = RecoveryFrameId(self.entered_frames);
        self.entered_frames += 1;
        self.state_slots = self
            .state_slots
            .saturating_add(recovery_frame_state_slots(plan));
        id
    }

    /// Records that one entered frame has permanently closed.
    fn close_frame(&mut self) {
        self.closed_frames += 1;
    }

    /// Rejects a duplicate guarded state before it can execute another transition.
    fn guard<'input>(
        &mut self,
        input: &Input<'input>,
        frame: &RecoveryFrame,
    ) -> Result<(), ExplorationError<'input>> {
        let key = RecoveryProgressKey {
            input: RecoveryInputIndex(input.cursor()),
            frame: frame.id,
            state: frame.progress(),
        };
        if self.retain_guarded(key).is_ok() {
            return Ok(());
        }
        let position = input.byte_position();
        let span = Span::new(position, position).unwrap_or_else(|_| Span::new(0, 0).unwrap());
        Err(recovery_fatal(
            input.source(),
            span,
            ParseDiagnosticKind::InvariantViolation,
            "a zero-consumption recovery progress key repeated",
        ))
    }

    /// Retains one key exactly once and rejects a direct duplicate.
    fn retain_guarded(&mut self, key: RecoveryProgressKey) -> Result<(), ()> {
        self.guarded.insert(key).then_some(()).ok_or(())
    }

    /// Freezes the retained public trace and checks its explicit finite bound.
    fn finish(self) -> Result<RecoveryProgressTrace, ()> {
        let positions = self.input_records.checked_add(1).ok_or(())?;
        let state_bound = positions.checked_mul(self.state_slots).ok_or(())?;
        let trace = RecoveryProgressTrace {
            input_records: self.input_records,
            entered_frames: self.entered_frames,
            closed_frames: self.closed_frames,
            guarded_states: self.guarded.len(),
            state_bound,
        };
        trace.is_within_bound().then_some(trace).ok_or(())
    }
}

/// Computes a conservative finite state-slot count for one generated frame.
fn recovery_frame_state_slots(plan: &'static RecoveryNodePlanData) -> usize {
    fn fixed_syntax_slots(syntax: &[RecoveryFixedSyntaxPlanData]) -> usize {
        syntax
            .iter()
            .map(|operation| match operation {
                RecoveryFixedSyntaxPlanData::Required(_) => 1,
                RecoveryFixedSyntaxPlanData::Optional(tokens) => tokens.len().max(1),
            })
            .sum()
    }

    fn element_slots(element: &RecoveryElementPlanData) -> usize {
        match &element.kind {
            RecoveryElementKindData::Fixed { .. } => 1,
            RecoveryElementKindData::OptionalFixed { tokens } => tokens.len().max(1),
            RecoveryElementKindData::FixedPresence {
                before,
                sequence,
                after,
            } => fixed_syntax_slots(before)
                .saturating_add(sequence.len().max(1))
                .saturating_add(fixed_syntax_slots(after))
                .saturating_add(3),
            RecoveryElementKindData::Semantic { before, after, .. } => fixed_syntax_slots(before)
                .saturating_add(fixed_syntax_slots(after))
                .saturating_add(7),
        }
    }

    let body_slots = match &plan.body {
        RecoveryNodeBodyPlanData::Struct { elements, .. } => {
            elements.iter().map(element_slots).sum()
        }
        RecoveryNodeBodyPlanData::Enum { variants, .. } => variants
            .iter()
            .map(|variant| variant.elements.iter().map(element_slots).sum())
            .max()
            .unwrap_or(0),
        RecoveryNodeBodyPlanData::Pratt(pratt) => {
            let nud = pratt
                .nud
                .iter()
                .map(|variant| variant.variant.elements.iter().map(element_slots).sum())
                .max()
                .unwrap_or(0usize);
            let led = pratt
                .led
                .iter()
                .map(|variant| variant.variant.elements.iter().map(element_slots).sum())
                .max()
                .unwrap_or(0usize);
            nud.saturating_add(led).saturating_add(5)
        }
    };
    body_slots.saturating_add(2)
}

/// One grammar-erased iterative interpreter and its complete mutable state.
struct RecoveryMachine<'machine, 'input> {
    /// Transactional input restored to its initial cursor.
    input: &'machine mut Input<'input>,
    /// Immutable generated recovery recipe.
    plan: &'static RecoveryPlanData,
    /// Source borrowed by input and all diagnostics.
    source: &'input str,
    /// Strict diagnostics plus recovery diagnostics accumulated in source order.
    diagnostics: Vec<ParseDiagnostic<'input>>,
    /// Explicit stack of entered application frames.
    frames: Vec<RecoveryFrame>,
    /// Progress proof guarding every transition.
    progress: RecoveryProgress,
}

#[derive(Clone, Copy)]
/// Stable metadata for the active generated element transition.
struct RecoveryElementContext {
    /// Owning frame within the explicit stack.
    frame: RecoveryFrameIndex,
    /// Source-order element position within the frame.
    element_index: RecoveryElementIndexData,
    /// Static schema owner for diagnostics.
    owner: &'static NodeSchemaData,
    /// Immutable generated element recipe.
    element: &'static RecoveryElementPlanData,
}

#[derive(Clone, Copy)]
/// Cohesive generated recipe for one repeated-container transition.
struct RecoveryRepeatedPlan {
    /// Required minimum once the container is present.
    minimum: RecoveryMinimumData,
    /// Whether the entire nonempty container may be absent.
    presence: RecoveryRepetitionPresenceData,
    /// FIRST selector for the next occurrence.
    element: &'static FollowSet,
    /// Optional separator selector.
    separator: Option<&'static FollowSet>,
    /// Final-separator policy.
    trailing: RecoveryTrailingSeparatorData,
    /// Frozen stop recipe.
    stop: RecoveryFollowData,
    /// FOLLOW recipe supplied to nested occurrences.
    child_follow: RecoveryFollowData,
    /// Failure evidence for an absent required minimum.
    minimum_failure: Option<RecoveryFollowData>,
    /// Failure evidence for a missing separator.
    continuation_failure: RecoveryFollowData,
    /// Failure evidence after a non-trailing separator.
    after_separator_failure: Option<RecoveryFollowData>,
}

impl RecoveryRepeatedPlan {
    /// Projects the repeated variant of one immutable container recipe.
    fn from_container(container: &'static RecoveryContainerPlanData) -> Option<Self> {
        let RecoveryContainerPlanData::Repeated {
            minimum,
            presence,
            element,
            separator,
            trailing,
            stop,
            child_follow,
            minimum_failure,
            continuation_failure,
            after_separator_failure,
        } = container
        else {
            return None;
        };
        Some(Self {
            minimum: *minimum,
            presence: *presence,
            element,
            separator: *separator,
            trailing: *trailing,
            stop: *stop,
            child_follow: *child_follow,
            minimum_failure: *minimum_failure,
            continuation_failure: *continuation_failure,
            after_separator_failure: *after_separator_failure,
        })
    }
}

/// Result of one completed-frame transition.
enum RecoveryFrameClose {
    /// The machine should execute the next guarded transition.
    Continue,
    /// The root frame closed and can finalize the recovery document.
    Root(RecoveryNode),
}

/// Interprets one generated immutable recovery plan after transactional strict failure.
///
/// The interpreter is grammar-erased and iterative. Its interface contains no
/// authored `T`, builder, construction callback, `Parsed`, or complete document.
///
/// # Long-function contract
///
/// Rationale: one machine owns cursor movement, the explicit frame stack, and
/// the zero-consumption key set, so no transition can bypass the global progress
/// invariant. Its small iterative dispatcher first guards the current state,
/// then performs exactly one transition: it commits pending enum dispatch;
/// advances Pratt NUD dispatch, NUD/LED elements, completed-variant folding, LED
/// dispatch, or final completion; closes an ordinary completed frame; or invokes
/// one typed fixed, attachment, or container transition method. Pratt phases keep
/// frozen dispatch, operand binding power, and accumulated lhs state on the same
/// frame, so extender loops obey the same one-part transition and progress
/// identity as ordinary elements. All transition methods are non-recursive and
/// share the machine-owned cursor, stack, diagnostics, and progress proof.
#[doc(hidden)]
pub fn recover_document<'input>(
    input: &mut Input<'input>,
    failure: ParseFailure<'input>,
    plan: &'static RecoveryPlanData,
) -> Result<RecoveryDocument<'input>, ExplorationError<'input>> {
    input.restore(0);
    recover_document_with_root_follow(input, failure, plan, plan.root_follow)
}

/// Interprets recovery from the current cursor with a generated framing root FOLLOW.
pub(crate) fn recover_document_with_root_follow<'input>(
    input: &mut Input<'input>,
    failure: ParseFailure<'input>,
    plan: &'static RecoveryPlanData,
    root_follow: &'static FollowSet,
) -> Result<RecoveryDocument<'input>, ExplorationError<'input>> {
    if failure.diagnostics.is_empty() {
        let position = input.byte_position();
        let span = Span::new(position, position).unwrap_or_else(|_| Span::new(0, 0).unwrap());
        return Err(recovery_fatal(
            input.source(),
            span,
            ParseDiagnosticKind::InvariantViolation,
            "public recovery requires a nonempty strict failure",
        ));
    }
    let projection =
        recover_document_from_seed_with_root_follow(input, failure.diagnostics, plan, root_follow)?;
    if projection.diagnostics.is_empty() {
        let position = input.byte_position();
        let span = Span::new(position, position).unwrap_or_else(|_| Span::new(0, 0).unwrap());
        return Err(recovery_fatal(
            input.source(),
            span,
            ParseDiagnosticKind::InvariantViolation,
            "public recovery completed without its strict diagnostic",
        ));
    }
    Ok(RecoveryDocument {
        source: projection.source,
        root: projection.root,
        diagnostics: projection.diagnostics,
        progress: projection.progress,
    })
}

/// Interprets framing recovery from an internal seed that may be empty.
pub(crate) fn recover_document_from_seed_with_root_follow<'input>(
    input: &mut Input<'input>,
    diagnostics: Box<[ParseDiagnostic<'input>]>,
    plan: &'static RecoveryPlanData,
    root_follow: &'static FollowSet,
) -> Result<RecoveryProjection<'input>, ExplorationError<'input>> {
    let source = input.source();
    if !std::ptr::eq(input.grammar(), plan.grammar) {
        let position = input.byte_position();
        let span = Span::new(position, position).unwrap_or_else(|_| Span::new(0, 0).unwrap());
        return Err(recovery_fatal(
            source,
            span,
            ParseDiagnosticKind::GrammarMismatch,
            "recovery input belongs to a different grammar",
        ));
    }
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            ParseDiagnosticKind::InvalidTokenSpan | ParseDiagnosticKind::GrammarMismatch
        )
    }) {
        return Err(ExplorationError {
            source,
            diagnostics,
        });
    }

    let mut progress = RecoveryProgress::new(input.record_count());
    let root_start = input.byte_position();
    let root_id = progress.enter_frame(plan.root);
    let root = RecoveryFrame::new(
        root_id,
        plan.root,
        root_follow.clone(),
        root_start,
        None,
        RecoveryBindingPower(0),
    );
    RecoveryMachine {
        input,
        plan,
        source,
        diagnostics: diagnostics.into_vec(),
        frames: vec![root],
        progress,
    }
    .run()
}

impl<'machine, 'input> RecoveryMachine<'machine, 'input> {
    /// Enters one selected content or nested-Node occurrence through a shared stack boundary.
    fn enter_semantic_occurrence(
        &mut self,
        context: RecoveryElementContext,
        terminal: RecoveryTerminalPlanData,
        child_follow: FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        match terminal {
            RecoveryTerminalPlanData::Content(schema) => recovery_push_content(
                self.input,
                context.element,
                schema,
                &mut self.frames[context.frame.get()],
                "recovery content span is not a checked UTF-8 source range",
            ),
            RecoveryTerminalPlanData::Node(node) => {
                let minimum_binding_power = self.frames[context.frame.get()]
                    .pratt
                    .as_ref()
                    .and_then(|pratt| {
                        pratt
                            .active_operands
                            .iter()
                            .find(|operand| operand.element_index == context.element_index)
                    })
                    .map_or(0, |operand| operand.minimum_binding_power);
                let start = self.input.byte_position();
                let child_id = self.progress.enter_frame(node);
                self.frames.push(RecoveryFrame::new(
                    child_id,
                    node,
                    child_follow,
                    start,
                    Some(context.element),
                    RecoveryBindingPower(minimum_binding_power),
                ));
                Ok(())
            }
        }
    }

    /// Consumes one selected repetition separator into its owning frame.
    fn consume_repetition_separator(
        &mut self,
        context: RecoveryElementContext,
        selector: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let record = self
            .input
            .advance()
            .expect("selected repetition separator exists");
        if self.source.get(record.span.range()).is_none() {
            return Err(recovery_fatal(
                self.source,
                record.span,
                ParseDiagnosticKind::InvalidTokenSpan,
                "repetition separator is not a UTF-8 source range",
            ));
        }
        debug_assert!(selector.contains(record.kind));
        let Some(schema) = self
            .plan
            .tokens
            .get(usize::from(record.kind))
            .copied()
            .flatten()
        else {
            return Err(recovery_fatal(
                self.source,
                record.span,
                ParseDiagnosticKind::InvariantViolation,
                "repetition separator lacks a static token schema",
            ));
        };
        let frame = &mut self.frames[context.frame.get()];
        frame.push_part(RecoveryPart::Separator(RecoverySeparator {
            schema,
            span: record.span,
        }));
        frame
            .repeated
            .as_mut()
            .expect("separator retains repeated state")
            .phase = RecoveryRepeatedPhase::AfterSeparator;
        Ok(())
    }

    /// Executes one typed repeated-container phase transition.
    fn repeated_container_transition(
        &mut self,
        context: RecoveryElementContext,
        terminal: RecoveryTerminalPlanData,
        recipe: RecoveryRepeatedPlan,
        caller_follow: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let frame = &mut self.frames[context.frame.get()];
        if frame
            .repeated
            .is_none_or(|state| state.element != context.element_index)
        {
            frame.repeated = Some(RecoveryRepeatedState {
                element: context.element_index,
                count: RecoveryOccurrenceCount(0),
                phase: RecoveryRepeatedPhase::NeedElement,
            });
        }
        let state = frame
            .repeated
            .expect("repeated element initializes its state");
        let stop = recipe.stop.evaluate(caller_follow).map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvariantViolation,
                "repetition stop width differs from caller FOLLOW",
            )
        })?;
        let starts = self
            .input
            .peek(0)
            .is_some_and(|record| recipe.element.contains(record.kind));

        if state.phase == RecoveryRepeatedPhase::NeedElement && starts {
            let repeated = self.frames[context.frame.get()]
                .repeated
                .as_mut()
                .expect("repeated state remains active");
            repeated.count.increment();
            repeated.phase = RecoveryRepeatedPhase::NeedContinuation;
            let child_follow = recipe.child_follow.evaluate(caller_follow).map_err(|()| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "repeated child FOLLOW width differs from caller FOLLOW",
                )
            })?;
            return self.enter_semantic_occurrence(context, terminal, child_follow);
        }

        match state.phase {
            RecoveryRepeatedPhase::NeedElement => {
                self.repeated_need_element(context, recipe, caller_follow, &stop)
            }
            RecoveryRepeatedPhase::NeedContinuation => {
                self.repeated_need_continuation(context, recipe, caller_follow, &stop)
            }
            RecoveryRepeatedPhase::AfterSeparator => {
                self.repeated_after_separator(context, recipe, caller_follow, &stop)
            }
        }
    }

    /// Stops, diagnoses, or synchronizes a repeated container awaiting an occurrence.
    fn repeated_need_element(
        &mut self,
        context: RecoveryElementContext,
        recipe: RecoveryRepeatedPlan,
        caller_follow: &FollowSet,
        stop: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let stops = self.input.peek(0).is_none()
            || self
                .input
                .peek(0)
                .is_some_and(|record| stop.contains(record.kind));
        if stops {
            let state = self.frames[context.frame.get()]
                .repeated
                .expect("repeated state remains active");
            let missing_required = recipe.minimum == RecoveryMinimumData::One
                && state.count.get() == 0
                && recipe.presence == RecoveryRepetitionPresenceData::Required;
            self.frames[context.frame.get()].finish_repeated_value();
            if missing_required {
                let expected = match recipe.minimum_failure {
                    Some(failure) => failure.evaluate(caller_follow).map_err(|()| {
                        recovery_fatal(
                            self.source,
                            Span::new(0, 0).unwrap(),
                            ParseDiagnosticKind::InvariantViolation,
                            "minimum failure width differs from caller FOLLOW",
                        )
                    })?,
                    None => recipe.element.clone(),
                };
                recovery_push_missing(
                    self.input,
                    self.plan,
                    context.owner,
                    context.element,
                    &expected,
                    &mut self.frames[context.frame.get()],
                    &mut self.diagnostics,
                    "missing repetition position exceeds checked span range",
                )?;
            }
            return Ok(());
        }
        recovery_push_skipped(
            self.input,
            self.plan,
            context.owner,
            Some(context.element.schema),
            recipe.element,
            stop,
            &mut self.frames[context.frame.get()],
            &mut self.diagnostics,
            "repetition start synchronization is not a checked UTF-8 range",
        )
    }

    /// Continues a repeated container through a separator or another occurrence.
    fn repeated_need_continuation(
        &mut self,
        context: RecoveryElementContext,
        recipe: RecoveryRepeatedPlan,
        caller_follow: &FollowSet,
        stop: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let starts_element = self
            .input
            .peek(0)
            .is_some_and(|record| recipe.element.contains(record.kind));
        let stops = self.input.peek(0).is_none()
            || self
                .input
                .peek(0)
                .is_some_and(|record| stop.contains(record.kind));
        let Some(separator) = recipe.separator else {
            if starts_element {
                self.frames[context.frame.get()]
                    .repeated
                    .as_mut()
                    .expect("unseparated repetition retains state")
                    .phase = RecoveryRepeatedPhase::NeedElement;
            } else if stops {
                self.frames[context.frame.get()].finish_repeated_value();
            } else {
                recovery_push_skipped(
                    self.input,
                    self.plan,
                    context.owner,
                    Some(context.element.schema),
                    recipe.element,
                    stop,
                    &mut self.frames[context.frame.get()],
                    &mut self.diagnostics,
                    "unseparated repetition synchronization is not a checked UTF-8 range",
                )?;
            }
            return Ok(());
        };

        let starts_separator = self
            .input
            .peek(0)
            .is_some_and(|record| separator.contains(record.kind));
        if starts_separator {
            self.consume_repetition_separator(context, separator)?;
        } else if stops {
            self.frames[context.frame.get()].finish_repeated_value();
        } else if starts_element {
            let expected = recipe
                .continuation_failure
                .evaluate(caller_follow)
                .map_err(|()| {
                    recovery_fatal(
                        self.source,
                        Span::new(0, 0).unwrap(),
                        ParseDiagnosticKind::InvariantViolation,
                        "continuation failure width differs from caller FOLLOW",
                    )
                })?;
            recovery_push_missing(
                self.input,
                self.plan,
                context.owner,
                context.element,
                &expected,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
                "missing separator position exceeds checked span range",
            )?;
            self.frames[context.frame.get()]
                .repeated
                .as_mut()
                .expect("missing separator retains repeated state")
                .phase = RecoveryRepeatedPhase::NeedElement;
        } else {
            let restart = recipe.element.union(separator).map_err(|_| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "repetition selector widths differ",
                )
            })?;
            recovery_push_skipped(
                self.input,
                self.plan,
                context.owner,
                Some(context.element.schema),
                &restart,
                stop,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
                "repetition continuation synchronization is not a checked UTF-8 range",
            )?;
        }
        Ok(())
    }

    /// Closes or recovers after one consumed repetition separator.
    fn repeated_after_separator(
        &mut self,
        context: RecoveryElementContext,
        recipe: RecoveryRepeatedPlan,
        caller_follow: &FollowSet,
        stop: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let starts = self
            .input
            .peek(0)
            .is_some_and(|record| recipe.element.contains(record.kind));
        if starts {
            self.frames[context.frame.get()]
                .repeated
                .as_mut()
                .expect("separator retains repeated state")
                .phase = RecoveryRepeatedPhase::NeedElement;
            return Ok(());
        }
        let stops = self.input.peek(0).is_none()
            || self
                .input
                .peek(0)
                .is_some_and(|record| stop.contains(record.kind));
        if !stops {
            return recovery_push_skipped(
                self.input,
                self.plan,
                context.owner,
                Some(context.element.schema),
                recipe.element,
                stop,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
                "post-separator synchronization is not a checked UTF-8 range",
            );
        }
        if recipe.trailing == RecoveryTrailingSeparatorData::Forbidden {
            let expected = match recipe.after_separator_failure {
                Some(failure) => failure.evaluate(caller_follow).map_err(|()| {
                    recovery_fatal(
                        self.source,
                        Span::new(0, 0).unwrap(),
                        ParseDiagnosticKind::InvariantViolation,
                        "after-separator failure width differs from caller FOLLOW",
                    )
                })?,
                None => recipe.element.clone(),
            };
            recovery_push_missing(
                self.input,
                self.plan,
                context.owner,
                context.element,
                &expected,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
                "missing post-separator position exceeds checked span range",
            )?;
        }
        self.frames[context.frame.get()].finish_repeated_value();
        Ok(())
    }

    /// Executes one direct or optional semantic-container transition.
    fn single_semantic_container_transition(
        &mut self,
        context: RecoveryElementContext,
        terminal: RecoveryTerminalPlanData,
        container: &'static RecoveryContainerPlanData,
        caller_follow: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let (restart, child_follow, required) = match container {
            RecoveryContainerPlanData::Direct {
                restart,
                child_follow,
            } => (
                restart.evaluate(caller_follow).map_err(|()| {
                    recovery_fatal(
                        self.source,
                        Span::new(0, 0).unwrap(),
                        ParseDiagnosticKind::InvariantViolation,
                        "recovery FIRST width differs from caller FOLLOW",
                    )
                })?,
                *child_follow,
                RecoveryRequirement::Required,
            ),
            RecoveryContainerPlanData::Optional {
                restart,
                child_follow,
            } => (
                (*restart).clone(),
                *child_follow,
                RecoveryRequirement::Optional,
            ),
            RecoveryContainerPlanData::Repeated { .. } => {
                unreachable!("repeated recovery is handled before direct and optional containers")
            }
        };
        let stop = child_follow.evaluate(caller_follow).map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvariantViolation,
                "semantic child FOLLOW width differs from caller FOLLOW",
            )
        })?;
        let starts = self
            .input
            .peek(0)
            .is_some_and(|record| restart.contains(record.kind));
        if !starts {
            let stopped = self.input.peek(0).is_none()
                || self
                    .input
                    .peek(0)
                    .is_some_and(|record| stop.contains(record.kind));
            if stopped {
                self.frames[context.frame.get()].finish_semantic_value();
                if required == RecoveryRequirement::Required {
                    recovery_push_missing(
                        self.input,
                        self.plan,
                        context.owner,
                        context.element,
                        &restart,
                        &mut self.frames[context.frame.get()],
                        &mut self.diagnostics,
                        "missing recovery position exceeds checked span range",
                    )?;
                }
                return Ok(());
            }
            return recovery_push_skipped(
                self.input,
                self.plan,
                context.owner,
                Some(context.element.schema),
                &restart,
                &stop,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
                "skipped recovery input is not a checked UTF-8 range",
            );
        }

        self.frames[context.frame.get()].finish_semantic_value();
        self.enter_semantic_occurrence(context, terminal, stop)
    }

    /// Starts one permanently committed Pratt variant body.
    fn begin_pratt_variant(
        &mut self,
        frame_index: RecoveryFrameIndex,
        variant: RecoveryVariantPlanData,
        operands: &'static [RecoveryPrattOperandPlanData],
        phase: RecoveryPrattPhase,
    ) {
        let frame = &mut self.frames[frame_index.get()];
        frame.elements = variant.elements;
        frame.committed_variant = Some(variant.schema);
        frame.next_element = RecoveryElementIndexData(0);
        frame.semantic = None;
        frame.repeated = None;
        frame.presence = None;
        frame.optional_fixed = None;
        let pratt = frame
            .pratt
            .as_mut()
            .expect("Pratt variant belongs to a Pratt frame");
        pratt.active_operands = operands;
        pratt.phase = phase;
    }

    /// Seals one completed Pratt variant and retains it as the next extender lhs.
    fn finish_pratt_variant(
        &mut self,
        frame_index: RecoveryFrameIndex,
    ) -> Result<(), ExplorationError<'input>> {
        let frame = &mut self.frames[frame_index.get()];
        let extent = Span::new(frame.start.get(), frame.end.get()).map_err(|_| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvalidTokenSpan,
                "Pratt recovery variant extent is not a checked UTF-8 range",
            )
        })?;
        let node = RecoveryNode {
            schema: frame.plan.schema,
            committed_variant: frame.committed_variant,
            extent,
            parts: std::mem::take(&mut frame.parts).into_boxed_slice(),
        };
        frame.elements = &[];
        frame.committed_variant = None;
        frame.next_element = RecoveryElementIndexData(0);
        let pratt = frame
            .pratt
            .as_mut()
            .expect("completed Pratt variant belongs to a Pratt frame");
        pratt.active_operands = &[];
        pratt.lhs = Some(node);
        pratt.phase = RecoveryPrattPhase::LedDispatch;
        Ok(())
    }

    /// Materializes the final accumulated lhs into the ordinary frame close path.
    fn complete_pratt_expression(
        &mut self,
        frame_index: RecoveryFrameIndex,
    ) -> Result<(), ExplorationError<'input>> {
        let frame = &mut self.frames[frame_index.get()];
        let pratt = frame
            .pratt
            .as_mut()
            .expect("completed expression belongs to a Pratt frame");
        let Some(lhs) = pratt.lhs.take() else {
            return Err(recovery_fatal(
                self.source,
                Span::new(frame.start.get(), frame.start.get())
                    .unwrap_or_else(|_| Span::new(0, 0).unwrap()),
                ParseDiagnosticKind::InvariantViolation,
                "Pratt recovery completed without a null denotation",
            ));
        };
        frame.start = RecoveryByteOffset(lhs.extent.start() as usize);
        frame.end = RecoveryByteOffset(lhs.extent.end() as usize);
        frame.committed_variant = lhs.committed_variant;
        frame.parts = lhs.parts.into_vec();
        frame.elements = &[];
        frame.next_element = RecoveryElementIndexData(0);
        pratt.phase = RecoveryPrattPhase::Complete;
        Ok(())
    }

    /// Closes an uncommitted Pratt nud at its owner FOLLOW without guessing.
    fn close_unresolved_pratt_nud(
        &mut self,
        frame_index: RecoveryFrameIndex,
        expected: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let owner = self.frames[frame_index.get()].plan.schema;
        let unresolved_at = recovery_unresolved(
            self.input,
            self.plan,
            owner,
            expected,
            "no Pratt null denotation committed before recovery stopped",
            &mut self.diagnostics,
        )
        .map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvalidTokenSpan,
                "unresolved Pratt recovery position exceeds checked span range",
            )
        })?;
        let frame = &mut self.frames[frame_index.get()];
        frame.end.include(unresolved_at.end());
        frame
            .pratt
            .as_mut()
            .expect("unresolved nud belongs to a Pratt frame")
            .phase = RecoveryPrattPhase::Complete;
        Ok(())
    }

    /// Selects, synchronizes, or closes one Pratt null denotation.
    fn pratt_nud_transition(
        &mut self,
        frame_index: RecoveryFrameIndex,
    ) -> Result<(), ExplorationError<'input>> {
        let frame = &self.frames[frame_index.get()];
        let pratt = frame
            .pratt
            .as_ref()
            .expect("nud transition has Pratt state");
        let plan = pratt.plan;
        let caller_follow = frame.caller_follow.clone();
        let expected = plan.nud_failure.evaluate(&caller_follow).map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvariantViolation,
                "Pratt nud failure width differs from caller FOLLOW",
            )
        })?;
        let selection_follow = plan
            .nud_selection_follow
            .union(&caller_follow)
            .map_err(|_| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "Pratt nud selection FOLLOW width differs from caller FOLLOW",
                )
            })?;
        let selected =
            recovery_dispatch_select(self.input, self.plan, plan.nud_dispatch, &selection_follow)
                .map_err(|()| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "Pratt nud lookahead is not a UTF-8 source range",
                )
            })?;
        if let Some(selected) = selected {
            let Some(variant) = plan.nud.get(selected.variant().get()) else {
                return Err(recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "Pratt nud dispatch selected an unknown variant",
                ));
            };
            self.begin_pratt_variant(
                frame_index,
                variant.variant,
                variant.operands,
                RecoveryPrattPhase::NudElements,
            );
            return Ok(());
        }

        let overlap = self.input.peek(0).is_some_and(|record| {
            plan.nud_restart.contains(record.kind) && caller_follow.contains(record.kind)
        });
        if overlap {
            return self.close_unresolved_pratt_nud(frame_index, &expected);
        }

        let stops = self.input.peek(0).is_none()
            || self
                .input
                .peek(0)
                .is_some_and(|record| caller_follow.contains(record.kind));
        let restarts = self
            .input
            .peek(0)
            .is_some_and(|record| plan.nud_restart.contains(record.kind));
        if stops || restarts {
            return self.close_unresolved_pratt_nud(frame_index, &expected);
        }
        recovery_push_skipped(
            self.input,
            self.plan,
            frame.plan.schema,
            None,
            plan.nud_restart,
            &caller_follow,
            &mut self.frames[frame_index.get()],
            &mut self.diagnostics,
            "Pratt nud synchronization input is not a checked UTF-8 range",
        )
    }

    /// Returns the exact frozen caller-FOLLOW guard active at this minimum power.
    fn active_pratt_guard(
        plan: &'static RecoveryPrattPlanData,
        minimum: RecoveryBindingPower,
    ) -> Result<Option<FollowSet>, ()> {
        let mut active: Option<FollowSet> = None;
        for branch in plan
            .caller_follow_guard
            .iter()
            .filter(|branch| branch.left_binding_power >= minimum.0)
        {
            active = Some(match active {
                Some(expected) => expected.union(branch.expected).map_err(|_| ())?,
                None => branch.expected.clone(),
            });
        }
        Ok(active)
    }

    /// Retains one Pratt ambiguity while reconciling its exact strict seed.
    fn retain_pratt_ambiguity(
        &mut self,
        owner: &'static NodeSchemaData,
        expected: &FollowSet,
    ) -> Result<(), ExplorationError<'input>> {
        let record = self.input.peek(0).expect("selected Pratt extender exists");
        let text = self.source.get(record.span.range()).ok_or_else(|| {
            recovery_fatal(
                self.source,
                record.span,
                ParseDiagnosticKind::InvalidTokenSpan,
                "Pratt ambiguity span is not a UTF-8 source range",
            )
        })?;
        let found = self.plan.grammar.kind_name(record.kind);
        let expected = recovery_expected_names(self.plan.grammar, expected);
        if let Some(seed) = self.diagnostics.iter_mut().find(|diagnostic| {
            diagnostic.kind == ParseDiagnosticKind::PrattAmbiguity
                && diagnostic.span == record.span
                && diagnostic.found == found
                && diagnostic.expected == expected
                && diagnostic.action.is_none()
                && diagnostic.owner.is_none()
                && diagnostic.site.is_none()
        }) {
            seed.owner = Some(owner);
        }
        self.diagnostics.push(ParseDiagnostic {
            kind: ParseDiagnosticKind::PrattAmbiguity,
            span: record.span,
            text,
            found,
            expected,
            message: "Pratt extender overlaps caller FOLLOW".into(),
            action: None,
            owner: Some(owner),
            site: None,
        });
        Ok(())
    }

    /// Selects one qualifying extender or permanently closes the Pratt frame.
    fn pratt_led_transition(
        &mut self,
        frame_index: RecoveryFrameIndex,
    ) -> Result<(), ExplorationError<'input>> {
        let frame = &self.frames[frame_index.get()];
        let pratt = frame
            .pratt
            .as_ref()
            .expect("led transition has Pratt state");
        let plan = pratt.plan;
        let minimum = pratt.minimum_binding_power;
        let caller_follow = frame.caller_follow.clone();
        let Some(dispatch) = plan.led_dispatch else {
            return self.complete_pratt_expression(frame_index);
        };
        let selected = recovery_dispatch_select(self.input, self.plan, dispatch, &caller_follow)
            .map_err(|()| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvalidTokenSpan,
                    "Pratt led lookahead is not a UTF-8 source range",
                )
            })?;
        let Some(selected) = selected else {
            return self.complete_pratt_expression(frame_index);
        };
        let Some(variant) = plan.led.get(selected.variant().get()) else {
            return Err(recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvariantViolation,
                "Pratt led dispatch selected an unknown variant",
            ));
        };
        let left_binding_power = variant.position.left_binding_power();
        if left_binding_power < minimum.0 {
            return self.complete_pratt_expression(frame_index);
        }

        if self
            .input
            .peek(0)
            .is_some_and(|record| caller_follow.contains(record.kind))
        {
            if matches!(selected, RecoveryDispatchSelection::RecoveryCommit(_)) {
                return self.complete_pratt_expression(frame_index);
            }
            let Some(guard) = Self::active_pratt_guard(plan, minimum).map_err(|()| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "Pratt caller-FOLLOW guard widths disagree",
                )
            })?
            else {
                return Err(recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "qualifying Pratt extender lacks its frozen caller-FOLLOW guard",
                ));
            };
            let record = self.input.peek(0).expect("selected Pratt extender exists");
            if !guard.contains(record.kind) {
                return Err(recovery_fatal(
                    self.source,
                    record.span,
                    ParseDiagnosticKind::InvariantViolation,
                    "selected Pratt extender disagrees with its frozen caller-FOLLOW guard",
                ));
            }
            self.retain_pratt_ambiguity(frame.plan.schema, &guard)?;
            return self.complete_pratt_expression(frame_index);
        }

        let frame = &mut self.frames[frame_index.get()];
        let lhs = frame
            .pratt
            .as_mut()
            .expect("selected extender belongs to a Pratt frame")
            .lhs
            .take()
            .expect("selected extender retains a recovered lhs");
        let lhs_span = lhs.extent;
        frame.start = RecoveryByteOffset(lhs_span.start() as usize);
        frame.end = RecoveryByteOffset(lhs_span.end() as usize);
        frame.parts = vec![RecoveryPart::Element(RecoveryElement {
            schema: variant.lhs,
            span: lhs_span,
            value: RecoveryValue::Node(Box::new(lhs)),
        })];
        self.begin_pratt_variant(
            frame_index,
            variant.variant,
            variant.operands,
            RecoveryPrattPhase::LedElements,
        );
        Ok(())
    }

    /// Executes one pending enum-dispatch transition without guessing on restart/FOLLOW overlap.
    fn dispatch_transition(
        &mut self,
        frame_index: RecoveryFrameIndex,
    ) -> Result<(), ExplorationError<'input>> {
        let frame = &self.frames[frame_index.get()];
        let pending = frame
            .pending_dispatch
            .expect("dispatch transition requires a pending enum");
        let caller_follow = frame.caller_follow.clone();
        let overlap = self.input.peek(0).is_some_and(|record| {
            pending.restart.contains(record.kind) && caller_follow.contains(record.kind)
        });
        if overlap {
            return self.close_unresolved_dispatch(
                frame_index,
                pending.restart,
                "ambiguous recovery position exceeds checked span range",
            );
        }

        let selected =
            recovery_dispatch_select(self.input, self.plan, pending.dispatch, &caller_follow)
                .map_err(|()| {
                    recovery_fatal(
                        self.source,
                        Span::new(0, 0).unwrap(),
                        ParseDiagnosticKind::InvalidTokenSpan,
                        "predictive recovery lookahead is not a UTF-8 source range",
                    )
                })?;
        if let Some(selection) = selected {
            let variant = selection.variant();
            let Some(variant) = pending.variants.get(variant.get()) else {
                return Err(recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "predictive recovery selected an unknown enum variant",
                ));
            };
            let frame = &mut self.frames[frame_index.get()];
            frame.elements = variant.elements;
            frame.committed_variant = Some(variant.schema);
            frame.pending_dispatch = None;
            return Ok(());
        }

        let stops = self.input.peek(0).is_none()
            || self
                .input
                .peek(0)
                .is_some_and(|record| caller_follow.contains(record.kind));
        let restarts = self
            .input
            .peek(0)
            .is_some_and(|record| pending.restart.contains(record.kind));
        if stops || restarts {
            return self.close_unresolved_dispatch(
                frame_index,
                pending.restart,
                "unresolved recovery position exceeds checked span range",
            );
        }

        recovery_push_skipped(
            self.input,
            self.plan,
            frame.plan.schema,
            None,
            pending.restart,
            &caller_follow,
            &mut self.frames[frame_index.get()],
            &mut self.diagnostics,
            "enum synchronization input is not a checked UTF-8 range",
        )
    }

    /// Permanently closes one uncommitted dispatch with an explicit diagnostic.
    fn close_unresolved_dispatch(
        &mut self,
        frame_index: RecoveryFrameIndex,
        restart: &FollowSet,
        invalid_message: &'static str,
    ) -> Result<(), ExplorationError<'input>> {
        let owner = self.frames[frame_index.get()].plan.schema;
        let unresolved_at = recovery_unresolved(
            self.input,
            self.plan,
            owner,
            restart,
            "no enum alternative committed before recovery stopped",
            &mut self.diagnostics,
        )
        .map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvalidTokenSpan,
                invalid_message,
            )
        })?;
        let frame = &mut self.frames[frame_index.get()];
        frame.end.include(unresolved_at.end());
        frame.pending_dispatch = None;
        Ok(())
    }

    /// Closes one completed frame or consumes trailing root input before retrying.
    fn close_frame_transition(
        &mut self,
        frame_index: RecoveryFrameIndex,
    ) -> Result<RecoveryFrameClose, ExplorationError<'input>> {
        let frame = &self.frames[frame_index.get()];
        let root_stops = self
            .input
            .peek(0)
            .is_some_and(|record| frame.caller_follow.contains(record.kind));
        if frame.return_element.is_none() && !self.input.is_eof() && !root_stops {
            let root_follow = frame.caller_follow.clone();
            recovery_push_skipped(
                self.input,
                self.plan,
                frame.plan.schema,
                None,
                &root_follow,
                &root_follow,
                &mut self.frames[frame_index.get()],
                &mut self.diagnostics,
                "skipped recovery input is not a checked UTF-8 range",
            )?;
            return Ok(RecoveryFrameClose::Continue);
        }

        let finished = self.frames.pop().expect("checked recovery frame");
        self.progress.close_frame();
        let return_element = finished.return_element;
        let node = finished.finish().map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvalidTokenSpan,
                "recovery Node extent is not a checked UTF-8 range",
            )
        })?;
        if let Some(parent) = self.frames.last_mut() {
            let element = return_element.expect("nested frame retains its parent site");
            parent.push_part(RecoveryPart::Element(RecoveryElement {
                schema: element.schema,
                span: node.extent,
                value: RecoveryValue::Node(Box::new(node)),
            }));
            return Ok(RecoveryFrameClose::Continue);
        }
        Ok(RecoveryFrameClose::Root(node))
    }

    /// Dispatches exactly one fixed, presence, or semantic element transition.
    fn element_transition(
        &mut self,
        context: RecoveryElementContext,
    ) -> Result<(), ExplorationError<'input>> {
        let caller_follow = self.frames[context.frame.get()].caller_follow.clone();
        let stop = context
            .element
            .stop
            .evaluate(&caller_follow)
            .map_err(|()| {
                recovery_fatal(
                    self.source,
                    Span::new(0, 0).unwrap(),
                    ParseDiagnosticKind::InvariantViolation,
                    "recovery FOLLOW widths differ",
                )
            })?;
        match &context.element.kind {
            RecoveryElementKindData::Fixed { expected } => recovery_fixed_element_transition(
                self.input,
                self.plan,
                context.owner,
                context.element,
                expected,
                &stop,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
            ),
            RecoveryElementKindData::Semantic {
                terminal,
                container,
                before,
                after,
            } => {
                if recovery_semantic_attachment_transition(
                    self.input,
                    self.plan,
                    context.owner,
                    context.element,
                    context.element_index,
                    before,
                    after,
                    &mut self.frames[context.frame.get()],
                    &mut self.diagnostics,
                )? {
                    return Ok(());
                }
                if let Some(recipe) = RecoveryRepeatedPlan::from_container(container) {
                    self.repeated_container_transition(context, *terminal, recipe, &caller_follow)
                } else {
                    self.single_semantic_container_transition(
                        context,
                        *terminal,
                        container,
                        &caller_follow,
                    )
                }
            }
            RecoveryElementKindData::OptionalFixed { tokens } => {
                recovery_optional_fixed_transition(
                    self.input,
                    self.plan,
                    context.element,
                    tokens,
                    &mut self.frames[context.frame.get()],
                )
            }
            RecoveryElementKindData::FixedPresence {
                before,
                sequence,
                after,
            } => recovery_fixed_presence_transition(
                self.input,
                self.plan,
                context.owner,
                context.element,
                before,
                sequence,
                after,
                &mut self.frames[context.frame.get()],
                &mut self.diagnostics,
            ),
        }
    }

    /// Freezes one closed root and the public proof of bounded machine progress.
    fn finish_document(
        self,
        node: RecoveryNode,
    ) -> Result<RecoveryProjection<'input>, ExplorationError<'input>> {
        let progress = self.progress.finish().map_err(|()| {
            recovery_fatal(
                self.source,
                Span::new(0, 0).unwrap(),
                ParseDiagnosticKind::InvariantViolation,
                "recovery progress exceeded its explicit finite bound",
            )
        })?;
        Ok(RecoveryProjection {
            source: self.source,
            root: node,
            diagnostics: recovery_finalize_diagnostics(self.diagnostics),
            progress,
        })
    }

    /// Executes one transition at a time under the global progress guard.
    fn run(mut self) -> Result<RecoveryProjection<'input>, ExplorationError<'input>> {
        loop {
            let Some(raw_frame_index) = self.frames.len().checked_sub(1) else {
                let span = Span::new(0, 0).unwrap();
                return Err(recovery_fatal(
                    self.source,
                    span,
                    ParseDiagnosticKind::InvariantViolation,
                    "recovery stack became empty without a root",
                ));
            };
            let frame_index = RecoveryFrameIndex(raw_frame_index);
            let frame = &self.frames[frame_index.get()];
            self.progress.guard(self.input, frame)?;

            if frame.pending_dispatch.is_some() {
                self.dispatch_transition(frame_index)?;
                continue;
            }

            if let Some(pratt) = &frame.pratt {
                match pratt.phase {
                    RecoveryPrattPhase::NudDispatch => {
                        self.pratt_nud_transition(frame_index)?;
                    }
                    RecoveryPrattPhase::NudElements | RecoveryPrattPhase::LedElements => {
                        if frame.next_element.get() == frame.elements.len() {
                            self.finish_pratt_variant(frame_index)?;
                        } else {
                            let context = RecoveryElementContext {
                                frame: frame_index,
                                element_index: frame.next_element,
                                owner: frame.plan.schema,
                                element: &frame.elements[frame.next_element.get()],
                            };
                            self.element_transition(context)?;
                        }
                    }
                    RecoveryPrattPhase::LedDispatch => {
                        self.pratt_led_transition(frame_index)?;
                    }
                    RecoveryPrattPhase::Complete => {
                        let close = self.close_frame_transition(frame_index)?;
                        let RecoveryFrameClose::Root(node) = close else {
                            continue;
                        };
                        return self.finish_document(node);
                    }
                }
                continue;
            }

            if frame.next_element.get() == frame.elements.len() {
                let close = self.close_frame_transition(frame_index)?;
                let RecoveryFrameClose::Root(node) = close else {
                    continue;
                };
                return self.finish_document(node);
            }

            let frame = &self.frames[frame_index.get()];
            let context = RecoveryElementContext {
                frame: frame_index,
                element_index: frame.next_element,
                owner: frame.plan.schema,
                element: &frame.elements[frame.next_element.get()],
            };
            self.element_transition(context)?;
        }
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

/// Runs one generated root parser as a strict grammar-erased transaction.
///
/// Lexical errors take precedence and are all retained. A successful root
/// parse must also consume every token before its typed value is erased.
#[doc(hidden)]
pub fn parse_document<'input, T, A>(
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
            action: None,
            owner: None,
            site: None,
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
        let found = input.grammar().kind_name(record.kind);
        let message = found.map_or_else(
            || "unexpected trailing input".to_owned(),
            |name| format!("unexpected trailing {name}"),
        );
        return Err(ParseFailure {
            source,
            diagnostics: vec![ParseDiagnostic {
                kind: ParseDiagnosticKind::TrailingInput,
                span: record.span,
                text: source.get(record.span.range()).unwrap_or(""),
                found,
                expected: vec!["end of input"].into_boxed_slice(),
                message: message.into(),
                action: None,
                owner: None,
                site: None,
            }]
            .into_boxed_slice(),
        });
    }

    Ok(ExplorerDocument::from_parsed(parsed))
}

/// Constructs one generated grammar-erased root adapter.
///
/// # Safety
/// The schema, lexer, capability flags, and sample function must belong to the
/// same Generation and materialized root application.
#[doc(hidden)]
pub const unsafe fn construct_adapter(
    schema: &'static GrammarSchemaData,
    pretty: bool,
    arbitrary: bool,
    lex: for<'input> fn(&'input str) -> LexResult<'input>,
    parse: ParseFn,
    explore: Option<ExploreFn>,
    sample: Option<SampleFn>,
) -> Adapter {
    Adapter {
        schema,
        capabilities: Capabilities {
            pretty,
            arbitrary,
            recovery: explore.is_some(),
        },
        lex,
        parse,
        explore,
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

    #[test]
    fn adapters_report_unsupported_sampling_as_none() {
        fn lex(source: &str) -> LexResult<'_> {
            lex_result(source, [], [])
        }
        fn parse(source: &str) -> Result<ExplorerDocument<'_>, ParseFailure<'_>> {
            Err(ParseFailure {
                source,
                diagnostics: vec![ParseDiagnostic {
                    kind: ParseDiagnosticKind::UnexpectedEof,
                    span: Span::new(0, 0).unwrap(),
                    text: "",
                    found: None,
                    expected: Box::new([]),
                    message: "unexpected end of input".into(),
                    action: None,
                    owner: None,
                    site: None,
                }]
                .into_boxed_slice(),
            })
        }

        let adapter = unsafe { construct_adapter(&GRAMMAR, false, false, lex, parse, None, None) };
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

    #[test]
    fn recovery_progress_rejects_an_exact_duplicate_key() {
        let key = RecoveryProgressKey {
            input: RecoveryInputIndex(0),
            frame: RecoveryFrameId(0),
            state: RecoveryFrameProgress::Element(
                RecoveryElementIndexData(0),
                RecoveryElementProgress::Ready,
            ),
        };
        let mut progress = RecoveryProgress::new(0);
        assert_eq!(progress.retain_guarded(key), Ok(()));
        assert_eq!(progress.retain_guarded(key), Err(()));
    }
}
