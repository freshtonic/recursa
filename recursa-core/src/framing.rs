//! Exact-source framing of repeated typed islands and grammar-erased regions.
//!
//! A matching generated declaration supplies the only plan constructor. The
//! interpreter parses every island strictly and transactionally. The first
//! ordinary syntax, ownership, or payload failure rejects the whole document
//! with the strict diagnostics of the failing island; no authored value and no
//! partial partition survive a rejection.

use crate::explorer::{
    ExplorerRoot, NodeSchemaData, ParseDiagnostic, VariantSchemaData, parse_failure_from_error,
    root_contains_variant, trailing_input_diagnostic,
};
use crate::input::{Input, LexResult, TokenRecord, TriviaRecord};
use crate::lex::LexDiagnostic;
use crate::parsed::{GeneratedParse, Parsed, parse_spanned_with_root_follow};
use crate::{FollowSet, GrammarInfo, Span};

/// Generated lexical function used only behind a framing declaration.
type LexFn = for<'input> fn(&'input str) -> LexResult<'input>;

#[doc(hidden)]
/// Immutable generated plan for one document-framing declaration.
pub struct PlanData {
    /// Matching grammar metadata shared with strict parsing.
    grammar: &'static GrammarInfo,
    /// Static Explorer schema of the framing root application.
    root: &'static NodeSchemaData,
    /// Required fixed token separating adjacent typed islands.
    boundary_kind: u16,
    /// Closed physical-line token kinds owned atomically.
    line_kinds: &'static [u16],
    /// Closed delimited token kinds owned atomically.
    delimited_kinds: &'static [u16],
    /// Optional transition entered only after one complete strict variant.
    payload: Option<PayloadPlanData>,
    /// Generated root FOLLOW containing the framing boundary.
    boundary_follow: &'static FollowSet,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
/// Generated identity and standalone terminator for one payload transition.
pub struct PayloadPlanData {
    /// Exact static variant selected by a complete strict island.
    after: &'static VariantSchemaData,
    /// Nonempty text matched as one complete physical line.
    terminator: &'static str,
}

#[doc(hidden)]
/// Constructs one generated payload transition.
pub const fn payload_plan(
    after: &'static VariantSchemaData,
    terminator: &'static str,
) -> PayloadPlanData {
    PayloadPlanData { after, terminator }
}

#[doc(hidden)]
/// Constructs one immutable generated framing plan.
///
/// The generator validates kind disjointness, closed matcher classes, and
/// variant reachability before emitting this call.
pub const fn plan(
    grammar: &'static GrammarInfo,
    root: &'static NodeSchemaData,
    boundary_kind: u16,
    line_kinds: &'static [u16],
    delimited_kinds: &'static [u16],
    payload: Option<PayloadPlanData>,
    boundary_follow: &'static FollowSet,
) -> PlanData {
    PlanData {
        grammar,
        root,
        boundary_kind,
        line_kinds,
        delimited_kinds,
        payload,
        boundary_follow,
    }
}

/// A fully strict exact-source document partition.
pub struct CompleteFrame<'input, T> {
    /// Complete borrowed source.
    source: &'input str,
    /// Ordered, non-overlapping, exhaustive source ownership.
    parts: Box<[CompletePart<'input, T>]>,
}

impl<'input, T> CompleteFrame<'input, T> {
    /// Returns the complete source.
    pub const fn source(&self) -> &'input str {
        self.source
    }

    /// Returns exact original source without normalization.
    pub const fn render_exact(&self) -> &'input str {
        self.source
    }

    /// Iterates exact source parts in ownership order.
    pub fn parts(&self) -> impl ExactSizeIterator<Item = &CompletePart<'input, T>> {
        self.parts.iter()
    }

    /// Iterates all owned spans in source order.
    pub fn spans(&self) -> impl ExactSizeIterator<Item = Span> + '_ {
        self.parts.iter().map(CompletePart::span)
    }

    /// Iterates only complete parsed authored islands.
    pub fn typed_islands(&self) -> impl Iterator<Item = &Parsed<'input, T>> {
        self.parts.iter().filter_map(|part| match part {
            CompletePart::Island(island) => Some(&island.parsed),
            CompletePart::Gap(_)
            | CompletePart::Line(_)
            | CompletePart::Delimited(_)
            | CompletePart::Payload(_) => None,
        })
    }
}

/// One source-owned part of a fully strict partition.
pub enum CompletePart<'input, T> {
    /// Beginning, inter-item, or end source not owned by a significant item.
    Gap(SourceRegion<'input>),
    /// One complete strict typed island.
    Island(CompleteIsland<'input, T>),
    /// One atomically matched physical-line region.
    Line(SourceRegion<'input>),
    /// One atomically matched delimited region.
    Delimited(SourceRegion<'input>),
    /// Payload data and its distinct standalone terminator.
    Payload(PayloadRegion<'input>),
}

impl<T> CompletePart<'_, T> {
    /// Returns this part's exact checked ownership span.
    pub const fn span(&self) -> Span {
        match self {
            Self::Gap(region) | Self::Line(region) | Self::Delimited(region) => region.span,
            Self::Island(island) => island.span,
            Self::Payload(payload) => payload.span,
        }
    }
}

/// One complete authored island paired with its exact framing extent.
pub struct CompleteIsland<'input, T> {
    /// Span including the configured framing boundary.
    span: Span,
    /// Strict value and its occurrence provenance.
    parsed: Parsed<'input, T>,
}

impl<'input, T> CompleteIsland<'input, T> {
    /// Returns the exact island extent including its boundary token.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Borrows the complete strict authored value and provenance.
    pub const fn parsed(&self) -> &Parsed<'input, T> {
        &self.parsed
    }
}

/// One exact raw source region with no authored conversion.
#[derive(Clone, Copy)]
pub struct SourceRegion<'input> {
    /// Complete source shared by all region views.
    source: &'input str,
    /// Checked region range.
    span: Span,
}

impl<'input> SourceRegion<'input> {
    /// Returns the exact checked source range.
    pub const fn span(self) -> Span {
        self.span
    }

    /// Returns exact borrowed text.
    pub fn text(self) -> &'input str {
        self.source
            .get(self.span.range())
            .expect("framing stores only checked source spans")
    }
}

/// Payload data kept distinct from its standalone control marker.
pub struct PayloadRegion<'input> {
    /// Complete source shared by both subspans.
    source: &'input str,
    /// Combined exact ownership extent.
    span: Span,
    /// Payload data, including its exact physical line endings.
    data: Span,
    /// Standalone terminator line, including its line ending.
    terminator: Span,
}

impl<'input> PayloadRegion<'input> {
    /// Returns the combined exact payload ownership extent.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the exact payload-data span.
    pub const fn data_span(&self) -> Span {
        self.data
    }

    /// Returns exact payload data without the control line.
    pub fn data_text(&self) -> &'input str {
        self.source
            .get(self.data.range())
            .expect("framing stores only checked payload spans")
    }

    /// Returns the distinct standalone terminator-line span.
    pub const fn terminator_span(&self) -> Span {
        self.terminator
    }
}

/// Framing-level cause attached to a strict document rejection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameDiagnostic {
    /// Stable issue class.
    kind: FrameDiagnosticKind,
    /// Checked source location.
    span: Span,
    /// Deterministic explanation.
    message: Box<str>,
}

impl FrameDiagnostic {
    /// Returns the stable framing classification.
    pub const fn kind(&self) -> FrameDiagnosticKind {
        self.kind
    }

    /// Returns the append-only framing diagnostic code.
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// Returns the checked source location.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the deterministic explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Stable framing-level rejection classifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDiagnosticKind {
    /// A fully completed payload-opening island lacked its control line.
    MissingPayloadTerminator,
    /// A complete island left significant source before its required boundary.
    MissingBoundary,
}

impl FrameDiagnosticKind {
    /// Returns the append-only Recursa diagnostic code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingPayloadTerminator => "RCA5001",
            Self::MissingBoundary => "RCA5002",
        }
    }
}

/// Strict rejection of one document by its first failing island.
///
/// A rejection is ordinary invalid input, not an invariant violation. It
/// retains the complete source, the extent of the island candidate that failed,
/// the strict parse diagnostics of that candidate, and the framing-level cause
/// when the failure is a missing boundary or payload terminator rather than a
/// syntax error. At least one of the two diagnostic surfaces is present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameRejection<'input> {
    /// Complete retained source.
    source: &'input str,
    /// Extent of the island candidate, or payload region, that was rejected.
    island: Span,
    /// Strict parse diagnostics of the failing island, in source order.
    diagnostics: Box<[ParseDiagnostic<'input>]>,
    /// Framing-level cause when the island itself parsed strictly.
    framing: Option<FrameDiagnostic>,
}

impl<'input> FrameRejection<'input> {
    /// Returns the complete source retained after rejection.
    pub const fn source(&self) -> &'input str {
        self.source
    }

    /// Returns the extent of the rejected island candidate or payload region.
    ///
    /// For a syntax failure the extent runs from the island start to the end
    /// of the first candidate boundary at or after the failure, or to the end
    /// of input when no candidate boundary follows it.
    pub const fn island(&self) -> Span {
        self.island
    }

    /// Returns the strict parse diagnostics of the failing island.
    ///
    /// The slice is empty only when [`FrameRejection::framing`] reports a
    /// missing payload terminator after a complete island.
    pub fn diagnostics(&self) -> &[ParseDiagnostic<'input>] {
        &self.diagnostics
    }

    /// Returns the framing-level cause, when the island itself parsed strictly.
    pub const fn framing(&self) -> Option<&FrameDiagnostic> {
        self.framing.as_ref()
    }

    /// Returns the checked location that best explains the rejection.
    pub fn span(&self) -> Span {
        match (&self.framing, self.diagnostics.first()) {
            (Some(framing), _) => framing.span(),
            (None, Some(diagnostic)) => diagnostic.span(),
            (None, None) => self.island,
        }
    }
}

impl std::fmt::Display for FrameRejection<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.framing, self.diagnostics.first()) {
            (Some(framing), _) => formatter.write_str(framing.message()),
            (None, Some(diagnostic)) => formatter.write_str(diagnostic.message()),
            (None, None) => formatter.write_str("framing island was rejected"),
        }
    }
}

impl std::error::Error for FrameRejection<'_> {}

/// Mutually exclusive failure of one strict framing request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameFailure<'input> {
    /// Ordinary invalid input rejected at its first failing island.
    Rejected(FrameRejection<'input>),
    /// Violated framing-plan, span, partition, or progress invariants.
    Fatal(FrameError<'input>),
}

impl<'input> FrameFailure<'input> {
    /// Returns the complete source retained by either failure.
    pub const fn source(&self) -> &'input str {
        match self {
            Self::Rejected(rejection) => rejection.source(),
            Self::Fatal(fatal) => fatal.source(),
        }
    }
}

impl<'input> From<FrameError<'input>> for FrameFailure<'input> {
    fn from(fatal: FrameError<'input>) -> Self {
        Self::Fatal(fatal)
    }
}

impl std::fmt::Display for FrameFailure<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(rejection) => rejection.fmt(formatter),
            Self::Fatal(fatal) => fatal.fmt(formatter),
        }
    }
}

impl std::error::Error for FrameFailure<'_> {}

/// Fatal framing-plan, span, partition, or progress invariant violation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameError<'input> {
    /// Complete retained source.
    source: &'input str,
    /// Checked invariant location.
    span: Span,
    /// Stable deterministic explanation.
    message: Box<str>,
}

impl<'input> FrameError<'input> {
    /// Returns the complete source retained after fatal failure.
    pub const fn source(&self) -> &'input str {
        self.source
    }

    /// Returns the checked invariant location.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the stable fatal diagnostic code.
    pub const fn code(&self) -> &'static str {
        "RCA5090"
    }

    /// Returns the deterministic invariant explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for FrameError<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FrameError<'_> {}

/// Ordered part retained until the whole partition is validated.
enum DraftPart<'input, T> {
    /// Exact gap.
    Gap(Span),
    /// One strict island and its exact extent.
    Island {
        /// Exact extent including the boundary when one is present.
        span: Span,
        /// Strict value with its occurrence provenance.
        parsed: Parsed<'input, T>,
    },
    /// Atomic physical line.
    Line(Span),
    /// Atomic delimited region.
    Delimited(Span),
    /// Payload region.
    Payload {
        /// Combined exact extent.
        span: Span,
        /// Payload data extent.
        data: Span,
        /// Control-line extent including its line ending.
        terminator: Span,
    },
}

/// Parses and partitions exact source through one generated framing plan.
///
/// # Long-function contract
///
/// Rationale: one transaction must own source partitioning, strict island lifetime, payload mode,
/// and the decision to reject the document at its first ordinary failure. Invariants: every draft
/// span is checked in whole-source coordinates, each strict attempt is bounded to its candidate
/// window, a boundary advances the hard progress key, and no typed value survives a rejection. A
/// final island whose strict parse consumes every significant token to end of input completes
/// without a trailing boundary; any other boundary-less candidate is rejected. Phases: scan exact
/// framing regions and candidate boundaries, parse each island in a bounded window, freeze payload
/// ownership, validate the complete partition, then publish the strict parts.
#[doc(hidden)]
pub fn frame_document<'input, T>(
    source: &'input str,
    lex: LexFn,
    plan: &'static PlanData,
) -> Result<CompleteFrame<'input, T>, FrameFailure<'input>>
where
    T: GeneratedParse<'input> + ExplorerRoot + 'input,
{
    validate_plan(source, plan)?;
    let mut byte = 0_usize;
    let mut drafts = Vec::new();

    while byte < source.len() {
        let lexed = lex_rebased(source, byte, lex)?;
        let records = lexed.records();
        let Some(first) = records.first().copied() else {
            drafts.push(DraftPart::Gap(checked_span(source, byte, source.len())?));
            break;
        };
        let first_start = first.span.start() as usize;
        if first_start > byte {
            drafts.push(DraftPart::Gap(checked_span(source, byte, first_start)?));
            byte = first_start;
        }

        if plan.line_kinds.contains(&first.kind) || plan.delimited_kinds.contains(&first.kind) {
            let span = first.span;
            if span.start() as usize != byte || span.end() <= span.start() {
                return Err(
                    fatal(source, span, "atomic framing region made no exact progress").into(),
                );
            }
            if plan.line_kinds.contains(&first.kind) {
                drafts.push(DraftPart::Line(span));
            } else {
                drafts.push(DraftPart::Delimited(span));
            }
            byte = span.end() as usize;
            continue;
        }

        let island_start = byte;
        let boundaries = records
            .iter()
            .copied()
            .filter(|record| record.kind == plan.boundary_kind)
            .collect::<Vec<_>>();
        let mut strict_match = None;
        for boundary in &boundaries {
            let mut input = island_input(
                source,
                checked_span(source, island_start, boundary.span.end() as usize)?,
                lex,
                plan,
            )?;
            if let Ok(parsed) =
                parse_spanned_with_root_follow::<T, ()>(&mut input, plan.boundary_follow)
                && input.peek(0).is_some_and(|record| record == *boundary)
            {
                strict_match = Some((parsed, *boundary));
                break;
            }
        }
        let Some((parsed, boundary)) = strict_match else {
            // No candidate boundary completes a strict island. The strict attempt
            // over the whole remaining source names the exact failure, or proves a
            // complete final island without a trailing boundary.
            let mut input = island_input(
                source,
                checked_span(source, island_start, source.len())?,
                lex,
                plan,
            )?;
            match parse_spanned_with_root_follow::<T, ()>(&mut input, plan.boundary_follow) {
                Ok(parsed) => {
                    let root_end = consumed_end(&input, island_start);
                    if let Some(next) = input.peek(0) {
                        return Err(FrameFailure::Rejected(FrameRejection {
                            source,
                            island: checked_span(
                                source,
                                island_start,
                                rejection_end(source, &boundaries, next.span),
                            )?,
                            diagnostics: vec![trailing_input_diagnostic(
                                source,
                                plan.grammar,
                                next,
                            )]
                            .into_boxed_slice(),
                            framing: Some(FrameDiagnostic {
                                kind: FrameDiagnosticKind::MissingBoundary,
                                span: next.span,
                                message:
                                    "framing island lacks its required boundary before this token"
                                        .into(),
                            }),
                        }));
                    }
                    drafts.push(DraftPart::Island {
                        span: checked_span(source, island_start, root_end)?,
                        parsed,
                    });
                    if root_end < source.len() {
                        drafts.push(DraftPart::Gap(checked_span(
                            source,
                            root_end,
                            source.len(),
                        )?));
                    }
                    break;
                }
                Err(error) => {
                    let failure = parse_failure_from_error(source, error);
                    let failure_span = failure.diagnostics().first().map_or_else(
                        || Span::new(island_start, island_start).unwrap(),
                        ParseDiagnostic::span,
                    );
                    // When the source before the failing token is itself a complete
                    // island, the failure is a missing boundary rather than a syntax
                    // error inside the island.
                    let framing = prefix_is_complete_island::<T>(
                        source,
                        island_start,
                        failure_span.start() as usize,
                        lex,
                        plan,
                    )?
                    .then(|| FrameDiagnostic {
                        kind: FrameDiagnosticKind::MissingBoundary,
                        span: failure_span,
                        message: "framing island lacks its required boundary before this token"
                            .into(),
                    });
                    return Err(FrameFailure::Rejected(FrameRejection {
                        source,
                        island: checked_span(
                            source,
                            island_start,
                            rejection_end(source, &boundaries, failure_span),
                        )?,
                        diagnostics: failure.diagnostics().to_vec().into_boxed_slice(),
                        framing,
                    }));
                }
            }
        };
        let island_end = boundary.span.end() as usize;
        // The framing boundary itself is the hard-progress transition for an
        // otherwise empty island.
        validate_hard_progress(source, island_start, island_end)?;
        let enters_payload = plan.payload.is_some_and(|payload| {
            crate::explorer::parsed_active_variant_is(&parsed, payload.after)
        });
        drafts.push(DraftPart::Island {
            span: checked_span(source, island_start, island_end)?,
            parsed,
        });
        byte = island_end;

        if enters_payload {
            let payload = plan.payload.expect("payload transition was selected");
            let Some(terminator) = standalone_terminator(source, byte, payload.terminator) else {
                let end = source.len();
                return Err(FrameFailure::Rejected(FrameRejection {
                    source,
                    island: checked_span(source, byte, end)?,
                    diagnostics: Box::new([]),
                    framing: Some(FrameDiagnostic {
                        kind: FrameDiagnosticKind::MissingPayloadTerminator,
                        span: checked_span(source, end, end)?,
                        message: format!(
                            "payload is missing standalone terminator {:?}",
                            payload.terminator
                        )
                        .into_boxed_str(),
                    }),
                }));
            };
            let end = terminator.end() as usize;
            drafts.push(DraftPart::Payload {
                span: checked_span(source, byte, end)?,
                data: checked_span(source, byte, terminator.start() as usize)?,
                terminator,
            });
            byte = end;
        }
    }

    if source.is_empty() {
        drafts.push(DraftPart::Gap(checked_span(source, 0, 0)?));
    }
    validate_draft_partition(source, &drafts)?;
    Ok(complete_from_drafts(source, drafts))
}

/// Reports whether the source before one failing token is a complete strict island.
///
/// The prefix must parse strictly and leave no significant token unconsumed; an
/// empty prefix is never a complete island.
fn prefix_is_complete_island<'input, T>(
    source: &'input str,
    island_start: usize,
    prefix_end: usize,
    lex: LexFn,
    plan: &'static PlanData,
) -> Result<bool, FrameError<'input>>
where
    T: GeneratedParse<'input> + ExplorerRoot + 'input,
{
    if prefix_end <= island_start {
        return Ok(false);
    }
    let mut input = island_input(
        source,
        checked_span(source, island_start, prefix_end)?,
        lex,
        plan,
    )?;
    Ok(
        parse_spanned_with_root_follow::<T, ()>(&mut input, plan.boundary_follow).is_ok()
            && input.peek(0).is_none(),
    )
}

/// Chooses the extent reported for a rejected island candidate.
///
/// The extent ends at the first candidate boundary at or after the failure so
/// a rejection names one statement-sized window rather than the whole suffix.
fn rejection_end(source: &str, boundaries: &[TokenRecord], failure: Span) -> usize {
    boundaries
        .iter()
        .find(|boundary| boundary.span.end() >= failure.end())
        .map_or(source.len(), |boundary| boundary.span.end() as usize)
}

/// Converts a validated draft partition into the published strict frame.
fn complete_from_drafts<'input, T>(
    source: &'input str,
    drafts: Vec<DraftPart<'input, T>>,
) -> CompleteFrame<'input, T> {
    let parts = drafts
        .into_iter()
        .map(|part| match part {
            DraftPart::Gap(span) => CompletePart::Gap(region(source, span)),
            DraftPart::Island { span, parsed } => {
                CompletePart::Island(CompleteIsland { span, parsed })
            }
            DraftPart::Line(span) => CompletePart::Line(region(source, span)),
            DraftPart::Delimited(span) => CompletePart::Delimited(region(source, span)),
            DraftPart::Payload {
                span,
                data,
                terminator,
            } => CompletePart::Payload(PayloadRegion {
                source,
                span,
                data,
                terminator,
            }),
        })
        .collect::<Vec<_>>();
    CompleteFrame {
        source,
        parts: parts.into_boxed_slice(),
    }
}

/// Projects one already-lexed suffix into an exact absolute island window.
fn island_input<'input>(
    source: &'input str,
    bounds: Span,
    lex: LexFn,
    plan: &PlanData,
) -> Result<Input<'input>, FrameError<'input>> {
    lex_window(source, bounds, lex)?
        .input_bounded(plan.grammar, bounds)
        .ok_or_else(|| fatal(source, bounds, "framing input window is invalid"))
}

/// Returns the end of the last consumed record without absorbing the following gap.
fn consumed_end(input: &Input<'_>, fallback: usize) -> usize {
    input
        .cursor()
        .checked_sub(1)
        .and_then(|index| input.record(index))
        .map_or(fallback, |record| record.span.end() as usize)
}

/// Validates immutable generated identities defensively at the runtime seam.
fn validate_plan<'input>(source: &'input str, plan: &PlanData) -> Result<(), FrameError<'input>> {
    if plan
        .payload
        .is_some_and(|payload| !root_contains_variant(plan.root, payload.after))
        || !plan
            .boundary_follow
            .is_exact_kind(plan.grammar.kind_count(), plan.boundary_kind)
        || plan
            .line_kinds
            .iter()
            .chain(plan.delimited_kinds)
            .any(|kind| usize::from(*kind) >= plan.grammar.kind_count())
    {
        return Err(fatal(
            source,
            Span::new(0, 0).expect("zero span is valid"),
            "generated framing plan violates boundary or payload identity",
        ));
    }
    validate_plan_components(
        source,
        plan.boundary_kind,
        plan.line_kinds,
        plan.delimited_kinds,
        plan.payload.map(|payload| payload.terminator),
    )
}

/// Validates generated kind ownership and physical-line control text.
fn validate_plan_components<'input>(
    source: &'input str,
    boundary_kind: u16,
    line_kinds: &[u16],
    delimited_kinds: &[u16],
    payload_terminator: Option<&str>,
) -> Result<(), FrameError<'input>> {
    let mut kinds = line_kinds
        .iter()
        .chain(delimited_kinds)
        .copied()
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    if kinds.windows(2).any(|pair| pair[0] == pair[1])
        || kinds.contains(&boundary_kind)
        || payload_terminator
            .is_some_and(|terminator| terminator.is_empty() || terminator.contains(['\r', '\n']))
    {
        return Err(fatal(
            source,
            Span::new(0, 0).expect("zero span is valid"),
            "generated framing plan violates kind or payload invariants",
        ));
    }
    Ok(())
}

/// Rejects an island transition that cannot advance the frozen byte cursor.
fn validate_hard_progress<'input>(
    source: &'input str,
    start: usize,
    end: usize,
) -> Result<(), FrameError<'input>> {
    if end <= start {
        return Err(fatal(
            source,
            checked_span(source, start, start)?,
            "framing island violated the hard progress key",
        ));
    }
    Ok(())
}

/// Lexes one suffix and rebases every checked record to complete-source offsets.
fn lex_rebased<'input>(
    source: &'input str,
    base: usize,
    lex: LexFn,
) -> Result<LexResult<'input>, FrameError<'input>> {
    lex_window(source, checked_span(source, base, source.len())?, lex)
}

/// Lexes one exact absolute-coordinate window and validates every shifted record.
fn lex_window<'input>(
    source: &'input str,
    bounds: Span,
    lex: LexFn,
) -> Result<LexResult<'input>, FrameError<'input>> {
    let base = bounds.start() as usize;
    let tail = source.get(bounds.range()).ok_or_else(|| {
        fatal(
            source,
            Span::new(0, 0).unwrap(),
            "framing cursor is not UTF-8",
        )
    })?;
    let relative = lex(tail);
    if relative.source() != tail {
        return Err(fatal(
            source,
            checked_span(source, base, base)?,
            "generated framing lexer returned a different source",
        ));
    }
    let mut previous = base;
    let mut records = Vec::with_capacity(relative.records().len());
    for record in relative.records() {
        let span = shifted_span(source, base, record.span)?;
        if (span.start() as usize) < previous
            || span.end() < span.start()
            || span.end() > bounds.end()
        {
            return Err(fatal(
                source,
                span,
                "framing lexer records overlap or regress",
            ));
        }
        previous = span.end() as usize;
        let diagnostic = record
            .diagnostic
            .map(|diagnostic| {
                Ok(LexDiagnostic {
                    code: diagnostic.code,
                    anchor: shifted_span(source, base, diagnostic.anchor)?,
                })
            })
            .transpose()?;
        records.push(TokenRecord {
            kind: record.kind,
            span,
            diagnostic,
        });
    }
    let trivia = relative
        .trivia()
        .map(|trivia| {
            trivia
                .iter()
                .map(|record| {
                    let span = shifted_span(source, base, record.span)?;
                    if span.start() < bounds.start() || span.end() > bounds.end() {
                        return Err(fatal(
                            source,
                            bounds,
                            "framing lexer trivia escaped its exact window",
                        ));
                    }
                    Ok(TriviaRecord {
                        kind: record.kind,
                        span,
                    })
                })
                .collect::<Result<Vec<_>, FrameError<'input>>>()
        })
        .transpose()?;
    Ok(LexResult::new(source, records, trivia))
}

/// Locates exact standalone terminator text and includes its physical line ending.
fn standalone_terminator(source: &str, start: usize, terminator: &str) -> Option<Span> {
    let suffix = source.get(start..)?;
    let mut line_start = start;
    for line in suffix.split_inclusive('\n') {
        let has_newline = line.ends_with('\n');
        let without_lf = line.strip_suffix('\n').unwrap_or(line);
        let content = without_lf.strip_suffix('\r').unwrap_or(without_lf);
        let line_end = line_start.checked_add(line.len())?;
        let physical_line_start = line_start == 0
            || source
                .as_bytes()
                .get(line_start.checked_sub(1)?)
                .is_some_and(|byte| *byte == b'\n');
        if physical_line_start && has_newline && content == terminator {
            return Span::new(line_start, line_end).ok();
        }
        line_start = line_end;
    }
    None
}

/// Proves that draft ownership is exhaustive, ordered, and non-overlapping.
fn validate_draft_partition<'input, T>(
    source: &'input str,
    drafts: &[DraftPart<'input, T>],
) -> Result<(), FrameError<'input>> {
    let mut cursor = 0_u32;
    for draft in drafts {
        let span = match draft {
            DraftPart::Gap(span)
            | DraftPart::Line(span)
            | DraftPart::Delimited(span)
            | DraftPart::Island { span, .. }
            | DraftPart::Payload { span, .. } => *span,
        };
        if span.start() != cursor || source.get(span.range()).is_none() {
            return Err(fatal(
                source,
                span,
                "framing partition does not own source exactly once",
            ));
        }
        cursor = span.end();
    }
    if cursor as usize != source.len() {
        return Err(fatal(
            source,
            checked_span(source, cursor as usize, cursor as usize)?,
            "framing partition does not reach end of source",
        ));
    }
    Ok(())
}

/// Constructs one raw source view.
const fn region(source: &str, span: Span) -> SourceRegion<'_> {
    SourceRegion { source, span }
}

/// Shifts one suffix-relative range to complete-source coordinates.
fn shifted_span<'input>(
    source: &'input str,
    base: usize,
    span: Span,
) -> Result<Span, FrameError<'input>> {
    let start = base.checked_add(span.start() as usize).ok_or_else(|| {
        fatal(
            source,
            Span::new(0, 0).unwrap(),
            "framing span offset overflow",
        )
    })?;
    let end = base.checked_add(span.end() as usize).ok_or_else(|| {
        fatal(
            source,
            Span::new(0, 0).unwrap(),
            "framing span offset overflow",
        )
    })?;
    checked_span(source, start, end)
}

/// Creates only a source-bounded UTF-8 range.
fn checked_span<'input>(
    source: &'input str,
    start: usize,
    end: usize,
) -> Result<Span, FrameError<'input>> {
    let span = Span::new(start, end).map_err(|_| {
        fatal(
            source,
            Span::new(0, 0).unwrap(),
            "framing span exceeds runtime width",
        )
    })?;
    if source.get(span.range()).is_none() {
        return Err(fatal(
            source,
            span,
            "framing span is not a UTF-8 source range",
        ));
    }
    Ok(span)
}

/// Constructs one fatal framing invariant result.
fn fatal<'input>(
    source: &'input str,
    span: Span,
    message: impl Into<Box<str>>,
) -> FrameError<'input> {
    FrameError {
        source,
        span,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns one deliberately out-of-bounds generated record.
    fn invalid_span_lex(source: &str) -> LexResult<'_> {
        LexResult::new(
            source,
            vec![TokenRecord {
                kind: 0,
                span: Span::new(0, 2).unwrap(),
                diagnostic: None,
            }],
            None,
        )
    }

    /// Pins the public fatal classification and complete source retention.
    fn assert_fatal(error: FrameError<'_>, source: &str) {
        assert_eq!(error.code(), "RCA5090");
        assert_eq!(error.source(), source);
    }

    #[test]
    fn invalid_plan_span_partition_and_progress_invariants_are_fatal() {
        assert_fatal(
            validate_plan_components("plan", 3, &[3], &[], None).unwrap_err(),
            "plan",
        );
        assert_fatal(
            lex_rebased("x", 0, invalid_span_lex).expect_err("invalid span must be fatal"),
            "x",
        );

        let invalid_partition = [DraftPart::<()>::Gap(Span::new(0, 0).unwrap())];
        assert_fatal(
            validate_draft_partition("x", &invalid_partition).unwrap_err(),
            "x",
        );
        assert_fatal(validate_hard_progress("x", 0, 0).unwrap_err(), "x");
    }

    #[test]
    fn a_rejection_names_its_framing_cause_before_its_strict_diagnostics() {
        let source = "copy 'x';\r\ndata";
        let rejection = FrameRejection {
            source,
            island: Span::new(9, source.len()).unwrap(),
            diagnostics: Box::new([]),
            framing: Some(FrameDiagnostic {
                kind: FrameDiagnosticKind::MissingPayloadTerminator,
                span: Span::new(source.len(), source.len()).unwrap(),
                message: "payload is missing standalone terminator".into(),
            }),
        };
        assert_eq!(
            rejection.span(),
            Span::new(source.len(), source.len()).unwrap()
        );
        assert_eq!(rejection.framing().unwrap().code(), "RCA5001");
        assert_eq!(
            rejection.to_string(),
            "payload is missing standalone terminator"
        );
        let failure = FrameFailure::Rejected(rejection);
        assert_eq!(failure.source(), source);
    }

    #[test]
    fn a_rejected_island_ends_at_the_first_candidate_boundary_after_its_failure() {
        let source = "put 1 1; put 2;";
        let boundary = |start: usize| TokenRecord {
            kind: 0,
            span: Span::new(start, start + 1).unwrap(),
            diagnostic: None,
        };
        let boundaries = [boundary(7), boundary(14)];
        assert_eq!(
            rejection_end(source, &boundaries, Span::new(6, 7).unwrap()),
            8
        );
        assert_eq!(
            rejection_end(source, &boundaries, Span::new(9, 12).unwrap()),
            15
        );
        assert_eq!(
            rejection_end(source, &[], Span::new(9, 12).unwrap()),
            source.len()
        );
    }
}
