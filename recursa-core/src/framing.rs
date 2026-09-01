//! Exact-source framing of repeated typed islands and grammar-erased regions.
//!
//! A matching generated declaration supplies the only plan constructor. The
//! interpreter first attempts every island transactionally. If any ordinary
//! syntax or payload failure occurs, all authored values are dropped and every
//! island is replayed through the construction-free recovery interpreter.

use crate::explorer::{
    ExplorationError, ExplorerRoot, ParseDiagnostic, RecoveryAction, RecoveryNode, RecoveryPart,
    RecoveryPlanData, RecoveryProgressTrace, RecoveryProjection, RecoveryValue, VariantSchemaData,
    parse_failure_from_error, recover_document_from_seed_with_root_follow,
    recovery_plan_matches_root, recovery_root_restart_matches, root_contains_variant,
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
    /// Matching grammar metadata shared with parsing and recovery.
    grammar: &'static GrammarInfo,
    /// Required fixed token separating adjacent typed islands.
    boundary_kind: u16,
    /// Closed physical-line token kinds owned atomically.
    line_kinds: &'static [u16],
    /// Closed delimited token kinds owned atomically.
    delimited_kinds: &'static [u16],
    /// Optional transition entered only after one complete strict variant.
    payload: Option<PayloadPlanData>,
    /// Matching construction-free root recovery plan.
    recovery: &'static RecoveryPlanData,
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
/// The generator validates kind disjointness, closed matcher classes, variant
/// reachability, and recovery identity before emitting this call.
pub const fn plan(
    grammar: &'static GrammarInfo,
    boundary_kind: u16,
    line_kinds: &'static [u16],
    delimited_kinds: &'static [u16],
    payload: Option<PayloadPlanData>,
    recovery: &'static RecoveryPlanData,
    boundary_follow: &'static FollowSet,
) -> PlanData {
    PlanData {
        grammar,
        boundary_kind,
        line_kinds,
        delimited_kinds,
        payload,
        recovery,
        boundary_follow,
    }
}

/// Mutually exclusive exact-source framing result.
pub enum FrameResult<'input, T> {
    /// Every island parsed strictly and every payload terminated.
    Complete(CompleteFrame<'input, T>),
    /// At least one ordinary failure erased every authored island value.
    Recovered(RecoveredFrame<'input>),
}

impl<'input, T> FrameResult<'input, T> {
    /// Returns the complete source owned exactly once by the partition.
    pub const fn source(&self) -> &'input str {
        match self {
            Self::Complete(frame) => frame.source(),
            Self::Recovered(frame) => frame.source(),
        }
    }

    /// Returns exact original source without structured Pretty rendering.
    pub const fn render_exact(&self) -> &'input str {
        self.source()
    }

    /// Iterates all owned spans in source order.
    pub fn spans(&self) -> Box<dyn Iterator<Item = Span> + '_> {
        match self {
            Self::Complete(frame) => Box::new(frame.parts.iter().map(CompletePart::span)),
            Self::Recovered(frame) => Box::new(frame.parts.iter().map(RecoveredPart::span)),
        }
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

/// A fully grammar-erased exact-source document partition.
pub struct RecoveredFrame<'input> {
    /// Complete borrowed source.
    source: &'input str,
    /// Ordered, non-overlapping, exhaustive source ownership.
    parts: Box<[RecoveredPart<'input>]>,
    /// Framing-level failures independent of grammar recovery diagnostics.
    diagnostics: Box<[FrameDiagnostic]>,
}

impl<'input> RecoveredFrame<'input> {
    /// Returns the complete source.
    pub const fn source(&self) -> &'input str {
        self.source
    }

    /// Returns exact original source without exposing Pretty.
    pub const fn render_exact(&self) -> &'input str {
        self.source
    }

    /// Iterates exact source parts in ownership order.
    pub fn parts(&self) -> impl ExactSizeIterator<Item = &RecoveredPart<'input>> {
        self.parts.iter()
    }

    /// Iterates construction-free recovered islands.
    pub fn islands(&self) -> impl Iterator<Item = &RecoveredIsland<'input>> {
        self.parts.iter().filter_map(|part| match part {
            RecoveredPart::Island(island) => Some(island),
            RecoveredPart::Gap(_)
            | RecoveredPart::Line(_)
            | RecoveredPart::Delimited(_)
            | RecoveredPart::Payload(_)
            | RecoveredPart::Unresolved(_) => None,
        })
    }

    /// Iterates retained payload regions.
    pub fn payloads(&self) -> impl Iterator<Item = &PayloadRegion<'input>> {
        self.parts.iter().filter_map(|part| match part {
            RecoveredPart::Payload(payload) => Some(payload),
            RecoveredPart::Gap(_)
            | RecoveredPart::Island(_)
            | RecoveredPart::Line(_)
            | RecoveredPart::Delimited(_)
            | RecoveredPart::Unresolved(_) => None,
        })
    }

    /// Returns framing-level diagnostics; island diagnostics remain on each island.
    pub fn diagnostics(&self) -> &[FrameDiagnostic] {
        &self.diagnostics
    }
}

/// One source-owned part of a recovered partition.
pub enum RecoveredPart<'input> {
    /// Beginning, inter-item, or end source not owned by a significant item.
    Gap(SourceRegion<'input>),
    /// One grammar-erased recovery projection.
    Island(RecoveredIsland<'input>),
    /// One atomically matched physical-line region.
    Line(SourceRegion<'input>),
    /// One atomically matched delimited region.
    Delimited(SourceRegion<'input>),
    /// Payload data and its optional standalone terminator.
    Payload(PayloadRegion<'input>),
    /// Fail-closed suffix whose ownership could not be assigned uniquely.
    Unresolved(SourceRegion<'input>),
}

impl RecoveredPart<'_> {
    /// Returns this part's exact checked ownership span.
    pub const fn span(&self) -> Span {
        match self {
            Self::Gap(region)
            | Self::Line(region)
            | Self::Delimited(region)
            | Self::Unresolved(region) => region.span,
            Self::Island(island) => island.span,
            Self::Payload(payload) => payload.span,
        }
    }
}

/// One recovered island retaining no authored semantic value.
pub struct RecoveredIsland<'input> {
    /// Framing extent including the configured boundary.
    span: Span,
    /// Construction-free grammar projection and progress evidence.
    recovery: RecoveryProjection<'input>,
}

impl RecoveredIsland<'_> {
    /// Returns the exact island extent including its boundary token.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the eager construction-free recovery root.
    pub const fn root(&self) -> &RecoveryNode {
        self.recovery.root()
    }

    /// Returns strict and recovery diagnostics for this island.
    pub fn diagnostics(&self) -> &[ParseDiagnostic<'_>] {
        self.recovery.diagnostics()
    }

    /// Returns finite-machine progress evidence for this island replay.
    pub const fn progress_trace(&self) -> RecoveryProgressTrace {
        self.recovery.progress_trace()
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
    /// Standalone terminator line, absent after a missing-terminator failure.
    terminator: Option<Span>,
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

    /// Returns the distinct standalone terminator-line span when present.
    pub const fn terminator_span(&self) -> Option<Span> {
        self.terminator
    }
}

/// Framing-level recoverable diagnostic.
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

/// Stable framing-level recoverable classifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDiagnosticKind {
    /// A fully completed payload-opening island lacked its control line.
    MissingPayloadTerminator,
    /// No unique source ownership continuation existed.
    AmbiguousOwnership,
}

impl FrameDiagnosticKind {
    /// Returns the append-only Recursa diagnostic code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingPayloadTerminator => "RCA5001",
            Self::AmbiguousOwnership => "RCA5002",
        }
    }
}

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

/// First-pass part retaining typed islands until the transaction outcome is known.
enum DraftPart<'input, T> {
    /// Exact gap.
    Gap(Span),
    /// Strict or recovered island replay recipe.
    Island {
        /// Exact extent including the boundary.
        span: Span,
        /// Strict value present only when the first pass succeeded.
        parsed: Option<Parsed<'input, T>>,
        /// Strict failure seed replayed without construction.
        diagnostics: Box<[ParseDiagnostic<'input>]>,
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
        /// Optional control-line extent.
        terminator: Option<Span>,
    },
    /// Fail-closed source suffix.
    Unresolved(Span),
}

/// Exact ownership assigned to source after a boundary-less island prefix.
#[derive(Clone, Copy)]
enum EofTail {
    /// Recovery owns the whole candidate because no generated restart was proven.
    None,
    /// Only insignificant source remains after a complete prefix.
    Gap,
    /// Generated root restart evidence proves a distinct unassignable suffix.
    Unresolved,
}

/// Parses and partitions exact source through one generated framing plan.
///
/// # Long-function contract
///
/// Rationale: one transaction must own source partitioning, strict island lifetime, payload mode,
/// and the decision to erase every typed value after any ordinary failure. Invariants: every draft
/// span is checked in whole-source coordinates, each strict/recovery input is bounded to its island,
/// a boundary advances the hard progress key even for a missing island, and no typed value survives
/// a recovered outcome. Phases: scan exact framing regions and candidate boundaries, parse each
/// island in a bounded window, freeze payload or fail-closed suffix ownership, validate the complete
/// partition, then either publish strict parts or replay all islands through construction-free
/// recovery.
#[doc(hidden)]
pub fn frame_document<'input, T>(
    source: &'input str,
    lex: LexFn,
    plan: &'static PlanData,
) -> Result<FrameResult<'input, T>, FrameError<'input>>
where
    T: GeneratedParse<'input> + ExplorerRoot + 'input,
{
    validate_plan::<T>(source, plan)?;
    let mut byte = 0_usize;
    let mut drafts = Vec::new();
    let mut framing_diagnostics = Vec::new();
    let mut recovered = false;

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
                return Err(fatal(
                    source,
                    span,
                    "atomic framing region made no exact progress",
                ));
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
        let mut recovery_boundary = None;
        for boundary in &boundaries {
            let mut input = island_input(
                source,
                checked_span(source, island_start, boundary.span.end() as usize)?,
                lex,
                plan,
            )?;
            match parse_spanned_with_root_follow::<T, ()>(&mut input, plan.boundary_follow) {
                Ok(parsed) if input.peek(0).is_some_and(|record| record == *boundary) => {
                    strict_match = Some((parsed, *boundary, consumed_end(&input, island_start)));
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    if recovery_stops_at_boundary(
                        source,
                        checked_span(source, island_start, boundary.span.end() as usize)?,
                        *boundary,
                        lex,
                        plan,
                    )? {
                        recovery_boundary = Some(*boundary);
                        break;
                    }
                }
            }
        }
        let (parsed, seed, boundary, root_end, eof_tail, eof_diagnostic) = if let Some((
            parsed,
            boundary,
            root_end,
        )) = strict_match
        {
            (
                Some(parsed),
                Vec::<ParseDiagnostic<'input>>::new().into_boxed_slice(),
                Some(boundary),
                root_end,
                EofTail::None,
                None,
            )
        } else if let Some(boundary) = recovery_boundary {
            recovered = true;
            let mut input = island_input(
                source,
                checked_span(source, island_start, boundary.span.end() as usize)?,
                lex,
                plan,
            )?;
            let error =
                match parse_spanned_with_root_follow::<T, ()>(&mut input, plan.boundary_follow) {
                    Ok(_) => {
                        return Err(fatal(
                            source,
                            boundary.span,
                            "strict framing candidate selection was inconsistent",
                        ));
                    }
                    Err(error) => error,
                };
            let seed = parse_failure_from_error(source, error)
                .diagnostics()
                .to_vec()
                .into_boxed_slice();
            (
                None,
                seed,
                Some(boundary),
                island_start,
                EofTail::None,
                None,
            )
        } else {
            let mut input = island_input(
                source,
                checked_span(source, island_start, source.len())?,
                lex,
                plan,
            )?;
            match parse_spanned_with_root_follow::<T, ()>(&mut input, plan.boundary_follow) {
                Ok(parsed) => {
                    let root_end = consumed_end(&input, island_start);
                    let (root_end, tail, diagnostic_span) = match input.peek(0) {
                        Some(next) if recovery_root_restart_matches(plan.recovery, next) => {
                            (root_end, EofTail::Unresolved, next.span)
                        }
                        Some(next) => (source.len(), EofTail::None, next.span),
                        None if root_end < source.len() => (
                            root_end,
                            EofTail::Gap,
                            checked_span(source, root_end, root_end)?,
                        ),
                        None => (
                            root_end,
                            EofTail::None,
                            checked_span(source, root_end, root_end)?,
                        ),
                    };
                    (
                        Some(parsed),
                        Vec::<ParseDiagnostic<'input>>::new().into_boxed_slice(),
                        None,
                        root_end,
                        tail,
                        Some(FrameDiagnostic {
                            kind: FrameDiagnosticKind::AmbiguousOwnership,
                            span: diagnostic_span,
                            message: "framing island lacks its required boundary; source ownership failed closed"
                                .into(),
                        }),
                    )
                }
                Err(error) => {
                    recovered = true;
                    let failure = parse_failure_from_error(source, error);
                    let split = failed_prefix_end(
                        &lexed,
                        failure.diagnostics()[0].span(),
                        island_start,
                        plan,
                    );
                    let root_end = split.unwrap_or(source.len());
                    let seed = if split.is_some() {
                        Vec::new().into_boxed_slice()
                    } else {
                        failure.diagnostics().to_vec().into_boxed_slice()
                    };
                    let diagnostic = &failure.diagnostics()[0];
                    (
                        None,
                        seed,
                        None,
                        root_end,
                        if split.is_some() {
                            EofTail::Unresolved
                        } else {
                            EofTail::None
                        },
                        Some(FrameDiagnostic {
                            kind: FrameDiagnosticKind::AmbiguousOwnership,
                            span: diagnostic.span(),
                            message: format!(
                                "{}; framing island lacks its required boundary and source ownership failed closed",
                                diagnostic.message(),
                            )
                            .into_boxed_str(),
                        }),
                    )
                }
            }
        };
        let Some(boundary) = boundary else {
            recovered = true;
            let root_span = checked_span(source, island_start, root_end)?;
            drafts.push(DraftPart::Island {
                span: root_span,
                parsed,
                diagnostics: seed,
            });
            let suffix = checked_span(source, root_end, source.len())?;
            if suffix.start() < suffix.end() {
                match eof_tail {
                    EofTail::None => {}
                    EofTail::Gap => drafts.push(DraftPart::Gap(suffix)),
                    EofTail::Unresolved => drafts.push(DraftPart::Unresolved(suffix)),
                }
            }
            framing_diagnostics.push(eof_diagnostic.ok_or_else(|| {
                fatal(
                    source,
                    root_span,
                    "boundary-less framing candidate omitted its ownership diagnostic",
                )
            })?);
            break;
        };
        let island_end = boundary.span.end() as usize;
        // The framing boundary itself is the hard-progress transition for an
        // otherwise empty or wholly missing island.
        validate_hard_progress(source, island_start, island_end)?;
        let enters_payload = parsed.as_ref().is_some_and(|parsed| {
            plan.payload.is_some_and(|payload| {
                crate::explorer::parsed_active_variant_is(parsed, payload.after)
            })
        });
        drafts.push(DraftPart::Island {
            span: checked_span(source, island_start, island_end)?,
            parsed,
            diagnostics: seed,
        });
        byte = island_end;

        if enters_payload {
            let payload = plan.payload.expect("payload transition was selected");
            match standalone_terminator(source, byte, payload.terminator) {
                Some(terminator) => {
                    let end = terminator.end() as usize;
                    drafts.push(DraftPart::Payload {
                        span: checked_span(source, byte, end)?,
                        data: checked_span(source, byte, terminator.start() as usize)?,
                        terminator: Some(terminator),
                    });
                    byte = end;
                }
                None => {
                    recovered = true;
                    let end = source.len();
                    drafts.push(DraftPart::Payload {
                        span: checked_span(source, byte, end)?,
                        data: checked_span(source, byte, end)?,
                        terminator: None,
                    });
                    framing_diagnostics.push(FrameDiagnostic {
                        kind: FrameDiagnosticKind::MissingPayloadTerminator,
                        span: checked_span(source, end, end)?,
                        message: format!(
                            "payload is missing standalone terminator {:?}",
                            payload.terminator
                        )
                        .into_boxed_str(),
                    });
                    byte = end;
                }
            }
        }
    }

    if source.is_empty() {
        drafts.push(DraftPart::Gap(checked_span(source, 0, 0)?));
    }
    validate_draft_partition(source, &drafts)?;
    if recovered {
        rebuild_recovered(source, lex, plan, drafts, framing_diagnostics)
            .map(FrameResult::Recovered)
    } else {
        complete_from_drafts(source, drafts).map(FrameResult::Complete)
    }
}

/// Tests a delimiter candidate against the generated construction-free recovery state.
///
/// A framing delimiter is eligible only when recovery completes the root while leaving that
/// exact token unconsumed as caller FOLLOW. Delimiters consumed by nested elements, repetitions,
/// or other grammar structure therefore cannot become document boundaries merely because a
/// strict attempt failed farther into the candidate window.
fn recovery_stops_at_boundary<'input>(
    source: &'input str,
    bounds: Span,
    boundary: TokenRecord,
    lex: LexFn,
    plan: &'static PlanData,
) -> Result<bool, FrameError<'input>> {
    let mut input = island_input(source, bounds, lex, plan)?;
    let projection = recover_document_from_seed_with_root_follow(
        &mut input,
        Vec::<ParseDiagnostic<'input>>::new().into_boxed_slice(),
        plan.recovery,
        plan.boundary_follow,
    )
    .map_err(|error| fatal_recovery(source, error))?;
    let matches = input.peek(0).is_some_and(|record| record == boundary);
    drop(projection);
    Ok(matches)
}

/// Converts a successful first pass without touching its strict values.
fn complete_from_drafts<'input, T>(
    source: &'input str,
    drafts: Vec<DraftPart<'input, T>>,
) -> Result<CompleteFrame<'input, T>, FrameError<'input>> {
    let parts = drafts
        .into_iter()
        .map(|part| match part {
            DraftPart::Gap(span) => Ok(CompletePart::Gap(region(source, span))),
            DraftPart::Island {
                span,
                parsed: Some(parsed),
                diagnostics,
            } if diagnostics.is_empty() => {
                Ok(CompletePart::Island(CompleteIsland { span, parsed }))
            }
            DraftPart::Line(span) => Ok(CompletePart::Line(region(source, span))),
            DraftPart::Delimited(span) => Ok(CompletePart::Delimited(region(source, span))),
            DraftPart::Payload {
                span,
                data,
                terminator: Some(terminator),
            } => Ok(CompletePart::Payload(PayloadRegion {
                source,
                span,
                data,
                terminator: Some(terminator),
            })),
            DraftPart::Island { span, .. }
            | DraftPart::Payload { span, .. }
            | DraftPart::Unresolved(span) => Err(fatal(
                source,
                span,
                "a failed framing part reached the complete transaction",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CompleteFrame {
        source,
        parts: parts.into_boxed_slice(),
    })
}

/// Replays every island through recovery after dropping all strict values.
fn rebuild_recovered<'input, T>(
    source: &'input str,
    lex: LexFn,
    plan: &'static PlanData,
    drafts: Vec<DraftPart<'input, T>>,
    diagnostics: Vec<FrameDiagnostic>,
) -> Result<RecoveredFrame<'input>, FrameError<'input>>
where
    T: GeneratedParse<'input> + ExplorerRoot + 'input,
{
    let mut parts = Vec::with_capacity(drafts.len());
    for draft in drafts {
        match draft {
            DraftPart::Gap(span) => parts.push(RecoveredPart::Gap(region(source, span))),
            DraftPart::Line(span) => parts.push(RecoveredPart::Line(region(source, span))),
            DraftPart::Delimited(span) => {
                parts.push(RecoveredPart::Delimited(region(source, span)));
            }
            DraftPart::Payload {
                span,
                data,
                terminator,
            } => parts.push(RecoveredPart::Payload(PayloadRegion {
                source,
                span,
                data,
                terminator,
            })),
            DraftPart::Unresolved(span) => {
                parts.push(RecoveredPart::Unresolved(region(source, span)));
            }
            DraftPart::Island {
                span,
                parsed,
                diagnostics,
            } => {
                drop(parsed);
                let start = span.start() as usize;
                let lexed = lex_island(source, start, span, lex, plan)?;
                let mut input = lexed.input_bounded(plan.grammar, span).ok_or_else(|| {
                    fatal(
                        source,
                        span,
                        "recovery island input escaped its checked boundary",
                    )
                })?;
                let recovery = recover_document_from_seed_with_root_follow(
                    &mut input,
                    diagnostics,
                    plan.recovery,
                    plan.boundary_follow,
                )
                .map_err(|error| fatal_recovery(source, error))?;
                validate_island_diagnostics(source, span, &recovery)?;
                let root_end = consumed_end(&input, start);
                if root_end > span.end() as usize {
                    return Err(fatal(
                        source,
                        span,
                        format!(
                            "construction-free framing replay crossed its island ownership ({root_end} > {}, cursor {})",
                            span.end(),
                            input.cursor(),
                        ),
                    ));
                }
                parts.push(RecoveredPart::Island(RecoveredIsland { span, recovery }));
            }
        }
    }
    Ok(RecoveredFrame {
        source,
        parts: parts.into_boxed_slice(),
        diagnostics: diagnostics.into_boxed_slice(),
    })
}

/// Validates that every island-local diagnostic and edit remains locally owned.
fn validate_island_diagnostics<'input>(
    source: &'input str,
    island: Span,
    recovery: &RecoveryProjection<'input>,
) -> Result<(), FrameError<'input>> {
    for diagnostic in recovery.diagnostics() {
        let action_span = match diagnostic.action() {
            Some(RecoveryAction::Missing { at }) => Some(at),
            Some(RecoveryAction::Skipped { span }) => Some(span),
            None => None,
        };
        if !span_contains(island, diagnostic.span())
            || action_span.is_some_and(|span| !span_contains(island, span))
        {
            return Err(fatal(
                source,
                island,
                "recovery diagnostic or action escaped its framing island",
            ));
        }
        if let Some(owner) = diagnostic.owner()
            && !recovery_owner_contains(
                recovery.root(),
                owner.qualified_name(),
                action_span.unwrap_or_else(|| diagnostic.span()),
            )
        {
            return Err(fatal(
                source,
                island,
                "recovery diagnostic owner does not contain its local span",
            ));
        }
    }
    Ok(())
}

/// Finds the exact recovered owner whose extent contains one diagnostic span.
fn recovery_owner_contains(root: &RecoveryNode, owner: &str, span: Span) -> bool {
    if root.schema().qualified_name() == owner && span_contains(root.extent(), span) {
        return true;
    }
    root.parts().any(|part| {
        matches!(
            part,
            RecoveryPart::Element(element)
                if matches!(element.value(), RecoveryValue::Node(child) if recovery_owner_contains(child, owner, span))
        )
    })
}

/// Reports whether one checked span is wholly contained by another.
const fn span_contains(outer: Span, inner: Span) -> bool {
    inner.start() >= outer.start() && inner.end() <= outer.end()
}

/// Re-lexes exactly one already-frozen island extent, including its boundary.
///
/// The first pass established this extent through strict parsing or the grammar-aware recovery
/// machine. The configured boundary remains in the bounded input so recovery can leave it
/// unconsumed as the caller FOLLOW; the framing part still owns that boundary byte range.
/// Restricting construction-free replay to this frozen extent prevents later islands from being
/// reconsidered and constructs no authored value.
fn lex_island<'input>(
    source: &'input str,
    start: usize,
    island: Span,
    lex: LexFn,
    _plan: &PlanData,
) -> Result<LexResult<'input>, FrameError<'input>> {
    if start != island.start() as usize {
        return Err(fatal(
            source,
            island,
            "recovery island start diverges from its checked window",
        ));
    }
    lex_window(source, island, lex)
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

/// Returns the last complete token before one strict failure location.
fn failed_prefix_end(
    lexed: &LexResult<'_>,
    failure: Span,
    fallback: usize,
    plan: &PlanData,
) -> Option<usize> {
    let failing = lexed
        .records()
        .iter()
        .find(|record| record.span.start() == failure.start())?;
    if !recovery_root_restart_matches(plan.recovery, *failing) {
        return None;
    }
    Some(
        lexed
            .records()
            .iter()
            .take_while(|record| record.span.end() <= failure.start())
            .last()
            .map_or(fallback, |record| record.span.end() as usize),
    )
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
fn validate_plan<'input, T: ExplorerRoot>(
    source: &'input str,
    plan: &PlanData,
) -> Result<(), FrameError<'input>> {
    let schema = T::__recursa_explorer_schema();
    if !recovery_plan_matches_root(plan.recovery, plan.grammar, schema)
        || plan
            .payload
            .is_some_and(|payload| !root_contains_variant(schema, payload.after))
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
            "generated framing plan violates recovery or payload identity",
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
            "framing island recovery violated the hard progress key",
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
            | DraftPart::Unresolved(span)
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

/// Converts a recovery invariant failure without reclassifying it as syntax.
fn fatal_recovery<'input>(
    source: &'input str,
    error: ExplorationError<'input>,
) -> FrameError<'input> {
    let diagnostic = error.diagnostics().first();
    FrameError {
        source,
        span: diagnostic.map_or_else(|| Span::new(0, 0).unwrap(), ParseDiagnostic::span),
        message: diagnostic
            .map_or(
                "construction-free recovery failed",
                ParseDiagnostic::message,
            )
            .into(),
    }
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
}
