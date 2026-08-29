#![allow(
    dead_code,
    reason = "private source analysis lexical-planning seam is consumed by later pipeline tickets"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Grammar-wide lexical planning for source analysis.
//!
//! This module merges token definitions, validates lexical languages, proves
//! keyword-carrier relationships, and assigns deterministic rule priority.
//! It publishes typed token-kind meaning rather than backend-specific lexer
//! state, leaving generation free to render the accepted plan.

use crate::{KindId, KindSet, TextEquivalence};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

use regex_automata::dfa::{Automaton as _, StartKind, dense};
use regex_automata::{Anchored, Input, MatchKind};

use crate::source_model::{
    AuthoredFixedTokenClass, ClosedMatcher, SourceModel, SourceModeledGrammar, SourceSite,
    TokenDefinitionKind, canonical_fixed_spelling,
};

#[derive(Clone, Debug, Eq, PartialEq)]
/// Reports either a validated lexical plan or the diagnostics that prevented one.
pub(crate) struct LexicalPlanReport {
    /// Diagnostics accumulated in deterministic source order.
    pub(crate) diagnostics: Vec<LexicalDiagnostic>,
    /// Complete plan, present only when no blocking diagnostic was emitted.
    pub(crate) plan: Option<LexicalPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Backend-independent description of every token recognized by the grammar.
pub(crate) struct LexicalPlan {
    /// Equivalence policy applied to fixed spellings and keyword carriers.
    pub(crate) text_equivalence: TextEquivalence,
    /// Tie-breaking contract that generated lexers must implement.
    pub(crate) selection_order: LexicalSelectionOrder,
    /// Stable kind table indexed by [`KindId`].
    pub(crate) token_kinds: Vec<TokenKindPlan>,
    /// Merged content-token definitions that produce the planned kinds.
    pub(crate) content_bases: Vec<ContentBasePlan>,
    /// Fixed keywords and the carrier relationships proved for them.
    pub(crate) keywords: Vec<KeywordPlan>,
    /// Parser-facing content-token targets and the kinds each accepts.
    pub(crate) parser_targets: Vec<ParserTargetPlan>,
    /// Token-emitting matcher rules.
    pub(crate) rules: Vec<LexicalRule>,
    /// Non-emitting matcher rules such as trivia.
    pub(crate) non_token_rules: Vec<NonTokenLexicalRule>,
    /// Correlation from source definitions to their assigned kind.
    pub(crate) lowering_map: LexicalLoweringMap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Defines the precedence contract shared by validation and generated lexers.
pub(crate) enum LexicalSelectionOrder {
    /// Prefer length, then fixed rules, then authored pattern priority.
    LongestPrefixThenFixedThenAuthoredPatternPriority,
}

impl LexicalPlan {
    /// Renders lexical projection in canonical order.
    pub(crate) fn canonical_lexical_projection(&self) -> String {
        // Render identity-bearing lexical facts in canonical order so source permutations do not perturb review artifacts.
        let mut rendered = String::new();
        writeln!(rendered, "equivalence {:?}", self.text_equivalence)
            .expect("writing to String cannot fail");
        writeln!(rendered, "selection {:?}", self.selection_order)
            .expect("writing to String cannot fail");
        let mut kind_labels = self
            .token_kinds
            .iter()
            .map(|kind| kind_label(&kind.identity))
            .collect::<Vec<_>>();
        kind_labels.sort();
        for kind in kind_labels {
            writeln!(rendered, "kind {kind}").expect("writing to String cannot fail");
        }
        let mut bases = self
            .content_bases
            .iter()
            .map(|base| {
                (
                    base,
                    kind_label(&self.token_kinds[base.kind.index()].identity),
                )
            })
            .collect::<Vec<_>>();
        bases.sort_by(|(left, left_kind), (right, right_kind)| {
            (
                &left.pattern,
                &left.closed_matcher,
                left.target_names.as_slice(),
                left_kind,
                left.authored_priority,
            )
                .cmp(&(
                    &right.pattern,
                    &right.closed_matcher,
                    right.target_names.as_slice(),
                    right_kind,
                    right.authored_priority,
                ))
        });
        for (base, kind) in bases {
            let closed = base
                .closed_matcher
                .as_ref()
                .map_or_else(String::new, |matcher| format!(" closed={matcher:?}"));
            writeln!(
                rendered,
                "base {} pattern={:?}{} targets=[{}] priority={}",
                kind,
                base.pattern,
                closed,
                base.target_names.as_slice().join(","),
                base.authored_priority.unwrap_or(0),
            )
            .expect("writing to String cannot fail");
        }
        for keyword in &self.keywords {
            let recognition = match &keyword.recognition {
                KeywordRecognition::Standalone => "standalone".to_owned(),
                KeywordRecognition::CombinedCarriers {
                    carrier,
                    additional_carriers,
                } => {
                    let mut carriers = std::iter::once(carrier)
                        .chain(additional_carriers)
                        .map(|kind| kind_label(&self.token_kinds[kind.index()].identity))
                        .collect::<Vec<_>>();
                    carriers.sort();
                    format!("carriers[{}]", carriers.join(","))
                }
            };
            writeln!(
                rendered,
                "keyword {} recognition={recognition}",
                keyword.canonical_spelling,
            )
            .expect("writing to String cannot fail");
        }
        let mut parser_targets = self.parser_targets.iter().collect::<Vec<_>>();
        parser_targets.sort_by(|left, right| left.name.cmp(&right.name));
        for target in parser_targets {
            let base_label = kind_label(&self.token_kinds[target.base_kind.index()].identity);
            let mut admitted = target
                .accepted_kinds()
                .iter()
                .filter(|kind| *kind != target.base_kind)
                .map(|kind| kind_label(&self.token_kinds[kind.index()].identity))
                .collect::<Vec<_>>();
            admitted.sort();
            let mut accepted = vec![base_label.clone()];
            accepted.extend(admitted);
            writeln!(
                rendered,
                "target {} base={} accepts=[{}]",
                target.name,
                base_label,
                accepted.join(","),
            )
            .expect("writing to String cannot fail");
        }
        let mut rules = self
            .rules
            .iter()
            .map(|rule| {
                (
                    rule,
                    kind_label(&self.token_kinds[rule.kind.index()].identity),
                )
            })
            .collect::<Vec<_>>();
        rules.sort_by(|(left, left_kind), (right, right_kind)| {
            (
                &left.matcher,
                &left.closed_matcher,
                left_kind,
                left.role,
                lexical_priority_projection_key(left.priority),
            )
                .cmp(&(
                    &right.matcher,
                    &right.closed_matcher,
                    right_kind,
                    right.role,
                    lexical_priority_projection_key(right.priority),
                ))
        });
        for (rule, kind) in rules {
            let closed = rule
                .closed_matcher
                .as_ref()
                .map_or_else(String::new, |matcher| format!(" closed={matcher:?}"));
            writeln!(
                rendered,
                "rule {:?}{} kind={} priority={:?}",
                rule.matcher, closed, kind, rule.priority,
            )
            .expect("writing to String cannot fail");
        }
        let mut non_token_rules = self.non_token_rules.iter().collect::<Vec<_>>();
        non_token_rules.sort_by(|left, right| {
            (
                &left.matcher,
                &left.closed_matcher,
                lexical_priority_projection_key(left.priority),
                left.outcome,
            )
                .cmp(&(
                    &right.matcher,
                    &right.closed_matcher,
                    lexical_priority_projection_key(right.priority),
                    right.outcome,
                ))
        });
        for rule in non_token_rules {
            let closed = rule
                .closed_matcher
                .as_ref()
                .map_or_else(String::new, |matcher| format!(" closed={matcher:?}"));
            let priority = match rule.priority {
                LexicalPriority::Pattern { authored: None } => String::new(),
                priority => format!(" priority={priority:?}"),
            };
            writeln!(
                rendered,
                "non-token {:?}{}{} outcome={:?}",
                rule.matcher, closed, priority, rule.outcome
            )
            .expect("writing to String cannot fail");
        }
        rendered
    }
}

/// Projects rule priority into the ordering used by canonical output.
const fn lexical_priority_projection_key(priority: LexicalPriority) -> (u8, Option<u32>) {
    match priority {
        LexicalPriority::Fixed => (0, None),
        LexicalPriority::Pattern { authored } => (1, authored),
    }
}

/// Renders an identity-bearing, stable label for a token kind.
pub(crate) fn kind_label(identity: &TokenKindIdentity) -> String {
    match identity {
        TokenKindIdentity::Fixed(spelling) => format!("fixed:{spelling}"),
        TokenKindIdentity::ContentBase(base) => {
            let closed = base
                .closed_matcher
                .as_ref()
                .map_or_else(String::new, |matcher| format!(":closed={matcher:?}"));
            format!(
                "content:{:?}{closed}:priority={:?}",
                base.pattern, base.priority,
            )
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Associates one dense kind ID with its grammar-derived identity.
pub(crate) struct TokenKindPlan {
    /// Dense index used by parser and lexer tables.
    pub(crate) id: KindId,
    /// Meaning that remains stable across source-order permutations.
    pub(crate) identity: TokenKindIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Identifies a kind by the lexical definition that produces it.
pub(crate) enum TokenKindIdentity {
    /// A fixed spelling.
    Fixed(String),
    /// A merged content-token matcher.
    ContentBase(ContentBaseIdentity),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Identity-bearing matcher metadata shared by equivalent content definitions.
pub(crate) struct ContentBaseIdentity {
    /// Preserves the authored matcher pattern for validation and generation.
    pub(crate) pattern: String,
    /// Preserves the closed scanning mechanism, when centrally declared.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Controls deterministic selection among competing lexical matchers.
    pub(crate) priority: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Merged content-token definition published to lexer generation.
pub(crate) struct ContentBasePlan {
    /// Identifies the token kind produced by this lexical rule.
    pub(crate) kind: KindId,
    /// Lists parser targets that admit this token classification.
    pub(crate) target_names: Vec<String>,
    /// Preserves the authored matcher pattern for validation and generation.
    pub(crate) pattern: String,
    /// Preserves the closed scanning mechanism, when centrally declared.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Preserves an explicitly authored pattern priority, when present.
    pub(crate) authored_priority: Option<u32>,
}

/// Equivalent authored definitions merged under one content identity.
struct ContentBaseGroup<'source> {
    /// Shared identity established from normalized definition metadata.
    identity: ContentBaseIdentity,
    /// Non-empty set of source definitions represented by the group.
    definitions: NonEmpty<&'source crate::source_model::TokenDefinition>,
}

/// Normalized matcher participating in overlap and priority analysis.
struct PatternContender {
    /// Preserves the authored matcher pattern for validation and generation.
    pattern: String,
    /// Lists parser targets that admit this token classification.
    target_names: Vec<String>,
    /// Preserves an explicitly authored pattern priority, when present.
    authored_priority: Option<u32>,
    /// Whether authors may resolve an overlap by assigning a priority.
    priority_authorable: bool,
    /// Whether the starter alone determines the match's final extent.
    final_extent: PatternFinalExtent,
    /// Observable effect when this matcher wins.
    outcome: LexicalPatternOutcome,
    /// Retains source sites in deterministic diagnostic order.
    sites: Vec<SourceSite>,
}

/// Describes how much lexical planning knows about a contender's final extent.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PatternFinalExtent {
    /// The anchored regular-expression extent is final.
    Exact,
    /// A closed scanner computes the final extent after its starter matches.
    Closed,
}

/// Equivalent non-emitting lexer records merged for planning.
struct NonTokenRuleGroup {
    /// Preserves the authored matcher pattern for validation and generation.
    pattern: String,
    /// Preserves the closed scanning mechanism, when centrally declared.
    closed_matcher: Option<ClosedMatcher>,
    /// Preserves explicit matcher arbitration priority.
    authored_priority: Option<u32>,
    /// Effect shared by all records in the group.
    outcome: crate::source_model::NonTokenLexerOutcome,
    /// Retains source sites in deterministic diagnostic order.
    sites: NonEmpty<SourceSite>,
}

/// Fixed matcher that remains standalone after carrier analysis.
struct RetainedFixedMatcher {
    /// Canonical authored spelling.
    spelling: String,
    /// Whether ASCII case variants are equivalent matches.
    ascii_insensitive: bool,
}

/// Normalized fixed spelling considered during carrier proof.
struct FixedMatcher {
    /// Canonical authored spelling.
    spelling: String,
    /// Whether the spelling consists solely of word characters.
    word_shaped: bool,
    /// Whether ASCII case variants are equivalent matches.
    ascii_insensitive: bool,
}

/// Proof that one or more content kinds recognize a fixed keyword spelling.
struct SymbolicCarrierProof {
    /// Canonical carrier used for the keyword classification.
    carrier: ContentBaseIdentity,
    /// Other content identities that recognize the same spelling at equal precedence.
    additional_carriers: Vec<ContentBaseIdentity>,
}

impl SymbolicCarrierProof {
    /// Iterates over the canonical carrier followed by any additional carriers.
    fn identities(&self) -> impl Iterator<Item = &ContentBaseIdentity> {
        std::iter::once(&self.carrier).chain(&self.additional_carriers)
    }
}

/// Memoizes keyword-carrier proofs by normalized fixed spelling.
struct CarrierProofCache {
    /// Successful and failed proofs already evaluated.
    proofs: BTreeMap<String, Option<SymbolicCarrierProof>>,
    /// Number of combined matchers constructed, exposed to invariant tests.
    matcher_constructions: usize,
    /// Number of uncached spellings evaluated, exposed to invariant tests.
    proof_evaluations: usize,
}

/// Records how a pattern that accepts a keyword affects token emission.
enum CarrierCandidate {
    /// Produces a content kind.
    Emitting {
        /// Index of the corresponding content-base group.
        group: usize,
        /// Whether a closed matcher prevents the starter from proving final recognition.
        starter_not_final: bool,
        /// Controls deterministic selection among competing lexical matchers.
        priority: u32,
    },
    /// Consumes input without producing a token.
    NonEmitting {
        /// Controls deterministic selection among competing lexical matchers.
        priority: u32,
    },
}

impl CarrierCandidate {
    /// Returns the candidate's normalized precedence.
    const fn priority(&self) -> u32 {
        match self {
            Self::Emitting { priority, .. } | Self::NonEmitting { priority } => *priority,
        }
    }
}

/// Combined DFA and metadata used to prove keyword-carrier relationships.
struct CarrierProver {
    /// DFA whose pattern IDs correspond to `candidates`, or `None` when compilation failed.
    dfa: Option<dense::DFA<Vec<u32>>>,
    /// Candidate metadata in the DFA's pattern order.
    candidates: Vec<CarrierCandidate>,
    /// Number of DFA constructions represented by this prover.
    matcher_constructions: usize,
}

impl ContentBaseGroup<'_> {
    /// Collects every definition site represented by this group.
    fn sites(&self) -> Vec<SourceSite> {
        self.definitions
            .iter()
            .flat_map(|definition| definition.definition_sites.iter().cloned())
            .collect()
    }

    /// Reports whether every grouped definition can author a matcher priority.
    fn priority_authorable(&self) -> bool {
        self.definitions
            .iter()
            .all(|definition| matches!(definition.kind, TokenDefinitionKind::Content(_)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the keyword plan used to plan deterministic token recognition.
pub(crate) struct KeywordPlan {
    /// Supplies the canonical spelling used to validate or execute the lexical plan.
    pub(crate) canonical_spelling: String,
    /// Identifies the token kind produced by this lexical rule.
    pub(crate) kind: KindId,
    /// Supplies the recognition used to validate or execute the lexical plan.
    pub(crate) recognition: KeywordRecognition,
    /// Supplies the definition sites used to validate or execute the lexical plan.
    pub(crate) definition_sites: Vec<SourceSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the keyword recognition alternatives recognized by lexical planning.
pub(crate) enum KeywordRecognition {
    /// Selects the standalone lexical alternative.
    Standalone,
    /// Selects the combined carriers lexical alternative.
    CombinedCarriers {
        /// Supplies the carrier used to validate or execute the lexical plan.
        carrier: KindId,
        /// Supplies the additional carriers used to validate or execute the lexical plan.
        additional_carriers: Vec<KindId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the parser target plan used to plan deterministic token recognition.
pub(crate) struct ParserTargetPlan {
    /// Supplies the definition used to validate or execute the lexical plan.
    pub(crate) definition: crate::lowering::TokenDefinitionId,
    /// Supplies the semantic used to validate or execute the lexical plan.
    pub(crate) semantic: crate::resolution::ContentTokenId,
    /// Supplies the name used to validate or execute the lexical plan.
    pub(crate) name: String,
    /// Supplies the base kind used to validate or execute the lexical plan.
    pub(crate) base_kind: KindId,
    /// Supplies the admitted kinds used to validate or execute the lexical plan.
    pub(crate) admitted_kinds: KindSet,
}

impl ParserTargetPlan {
    /// Derives the accepted kinds under the grammar keyword and lexical-selection policies.
    pub(crate) fn accepted_kinds(&self) -> KindSet {
        let mut accepted = self.admitted_kinds.clone();
        accepted
            .insert(self.base_kind)
            .expect("a parser target base kind uses the grammar's exact width");
        accepted
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the lexical rule used to plan deterministic token recognition.
pub(crate) struct LexicalRule {
    /// Supplies the matcher used to validate or execute the lexical plan.
    pub(crate) matcher: LexicalMatcher,
    /// Preserves the closed scanning mechanism, when centrally declared.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Identifies the token kind produced by this lexical rule.
    pub(crate) kind: KindId,
    /// Supplies the role used to validate or execute the lexical plan.
    pub(crate) role: LexicalRuleRole,
    /// Controls deterministic selection among competing lexical matchers.
    pub(crate) priority: LexicalPriority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the non token lexical rule used to plan deterministic token recognition.
pub(crate) struct NonTokenLexicalRule {
    /// Supplies the matcher used to validate or execute the lexical plan.
    pub(crate) matcher: LexicalMatcher,
    /// Preserves the closed scanning mechanism, when centrally declared.
    pub(crate) closed_matcher: Option<ClosedMatcher>,
    /// Controls deterministic selection among competing lexical matchers.
    pub(crate) priority: LexicalPriority,
    /// Supplies the outcome used to validate or execute the lexical plan.
    pub(crate) outcome: NonTokenLexicalOutcome,
    /// Retains source sites in deterministic diagnostic order.
    pub(crate) sites: NonEmpty<SourceSite>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the non token lexical outcome alternatives recognized by lexical planning.
pub(crate) enum NonTokenLexicalOutcome {
    /// Selects the non emitting lexical alternative.
    NonEmitting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the lexical lowering map used to plan deterministic token recognition.
pub(crate) struct LexicalLoweringMap {
    /// Supplies the token definitions used to validate or execute the lexical plan.
    pub(crate) token_definitions: Vec<TokenDefinitionKindCorrelation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the token definition kind correlation used to plan deterministic token recognition.
pub(crate) struct TokenDefinitionKindCorrelation {
    /// Supplies the source used to validate or execute the lexical plan.
    pub(crate) source: crate::lowering::TokenDefinitionId,
    /// Supplies the lowered used to validate or execute the lexical plan.
    pub(crate) lowered: KindId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the non empty used to plan deterministic token recognition.
pub(crate) struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    /// Starts a non-empty collection with its mandatory first value.
    fn new(first: T) -> Self {
        Self(vec![first])
    }

    /// Rejects an empty vector and preserves every value otherwise.
    fn from_vec(values: Vec<T>) -> Option<Self> {
        if values.is_empty() {
            None
        } else {
            Some(Self(values))
        }
    }

    /// Appends one value while preserving the non-empty invariant.
    fn push(&mut self, value: T) {
        self.0.push(value);
    }

    /// Iterates over values in insertion order.
    fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }

    /// Borrows all values without exposing mutable access to the backing vector.
    fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Reorders values with the supplied deterministic comparator.
    fn sort_by(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.0.sort_by(compare);
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the lexical matcher alternatives recognized by lexical planning.
pub(crate) enum LexicalMatcher {
    /// Selects the fixed lexical alternative.
    Fixed(String),
    /// Selects the regex lexical alternative.
    Regex(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the lexical priority alternatives recognized by lexical planning.
pub(crate) enum LexicalPriority {
    /// Selects the fixed lexical alternative.
    Fixed,
    /// Selects the pattern lexical alternative.
    Pattern {
        #[doc = "Optional priority explicitly supplied by the grammar author."]
        authored: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the lexical rule role alternatives recognized by lexical planning.
pub(crate) enum LexicalRuleRole {
    /// Selects the standalone keyword lexical alternative.
    StandaloneKeyword,
    /// Selects the standalone fixed lexical alternative.
    StandaloneFixed,
    /// Selects the content pattern lexical alternative.
    ContentPattern,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the lexical diagnostic used to plan deterministic token recognition.
pub(crate) struct LexicalDiagnostic {
    /// Supplies the code used to validate or execute the lexical plan.
    pub(crate) code: LexicalDiagnosticCode,
    /// Retains source sites in deterministic diagnostic order.
    pub(crate) sites: Vec<SourceSite>,
    /// Supplies the evidence used to validate or execute the lexical plan.
    pub(crate) evidence: LexicalDiagnosticEvidence,
    /// Supplies the remedies used to validate or execute the lexical plan.
    pub(crate) remedies: Vec<LexicalRemedy>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the lexical diagnostic code alternatives recognized by lexical planning.
pub(crate) enum LexicalDiagnosticCode {
    /// Selects the rca3001 lexical alternative.
    Rca3001,
    /// Selects the rca3002 lexical alternative.
    Rca3002,
    /// Selects the rca3003 lexical alternative.
    Rca3003,
    /// Selects the rca3004 lexical alternative.
    Rca3004,
    /// Selects the rca3005 lexical alternative.
    Rca3005,
    /// Selects the rca3006 lexical alternative.
    Rca3006,
}

impl LexicalDiagnosticCode {
    /// Complete diagnostic-code catalog in stable numeric order.
    pub(crate) const ALL: [Self; 6] = [
        Self::Rca3001,
        Self::Rca3002,
        Self::Rca3003,
        Self::Rca3004,
        Self::Rca3005,
        Self::Rca3006,
    ];

    /// Returns the stable diagnostic code used in rendered source-analysis output.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Rca3001 => "RCA3001",
            Self::Rca3002 => "RCA3002",
            Self::Rca3003 => "RCA3003",
            Self::Rca3004 => "RCA3004",
            Self::Rca3005 => "RCA3005",
            Self::Rca3006 => "RCA3006",
        }
    }

    /// Returns the registry entry defining this code's condition, evidence, and remedy.
    pub(crate) const fn spec(self) -> LexicalDiagnosticSpec {
        match self {
            Self::Rca3001 => LexicalDiagnosticSpec {
                code: self,
                condition: LexicalDiagnosticCondition::InvalidKeywordMatchingPolicy,
                evidence: LexicalEvidenceKind::KeywordMatchingPolicy,
                remedy: LexicalRemedyKind::ChooseSupportedKeywordMatchingPolicy,
            },
            Self::Rca3002 => LexicalDiagnosticSpec {
                code: self,
                condition: LexicalDiagnosticCondition::FixedTokenClassificationMismatch,
                evidence: LexicalEvidenceKind::FixedTokenClassification,
                remedy: LexicalRemedyKind::UseMechanicalFixedTokenClass,
            },
            Self::Rca3003 => LexicalDiagnosticSpec {
                code: self,
                condition: LexicalDiagnosticCondition::ConflictingTokenDefinition,
                evidence: LexicalEvidenceKind::ConflictingDefinitionMetadata,
                remedy: LexicalRemedyKind::MakeDefinitionMetadataEquivalent,
            },
            Self::Rca3004 => LexicalDiagnosticSpec {
                code: self,
                condition: LexicalDiagnosticCondition::UnsupportedLexicalPattern,
                evidence: LexicalEvidenceKind::InvalidPattern,
                remedy: LexicalRemedyKind::UseSupportedNonEmptyPattern,
            },
            Self::Rca3005 => LexicalDiagnosticSpec {
                code: self,
                condition: LexicalDiagnosticCondition::UnresolvedLexicalOverlap,
                evidence: LexicalEvidenceKind::PatternOverlap,
                remedy: LexicalRemedyKind::AddPriorityOrSeparatePatterns,
            },
            Self::Rca3006 => LexicalDiagnosticSpec {
                code: self,
                condition: LexicalDiagnosticCondition::TiedLexicalPriority,
                evidence: LexicalEvidenceKind::PriorityTie,
                remedy: LexicalRemedyKind::AssignDistinctPriority,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Carries the lexical diagnostic spec used to plan deterministic token recognition.
pub(crate) struct LexicalDiagnosticSpec {
    /// Supplies the code used to validate or execute the lexical plan.
    pub(crate) code: LexicalDiagnosticCode,
    /// Supplies the condition used to validate or execute the lexical plan.
    pub(crate) condition: LexicalDiagnosticCondition,
    /// Supplies the evidence used to validate or execute the lexical plan.
    pub(crate) evidence: LexicalEvidenceKind,
    /// Supplies the remedy used to validate or execute the lexical plan.
    pub(crate) remedy: LexicalRemedyKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the lexical diagnostic condition alternatives recognized by lexical planning.
pub(crate) enum LexicalDiagnosticCondition {
    /// Selects the invalid keyword matching policy lexical alternative.
    InvalidKeywordMatchingPolicy,
    /// Selects the fixed token classification mismatch lexical alternative.
    FixedTokenClassificationMismatch,
    /// Selects the conflicting token definition lexical alternative.
    ConflictingTokenDefinition,
    /// Selects the unsupported lexical pattern lexical alternative.
    UnsupportedLexicalPattern,
    /// Selects the unresolved lexical overlap lexical alternative.
    UnresolvedLexicalOverlap,
    /// Selects the tied lexical priority lexical alternative.
    TiedLexicalPriority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the lexical evidence kind alternatives recognized by lexical planning.
pub(crate) enum LexicalEvidenceKind {
    /// Selects the keyword matching policy lexical alternative.
    KeywordMatchingPolicy,
    /// Selects the fixed token classification lexical alternative.
    FixedTokenClassification,
    /// Selects the conflicting definition metadata lexical alternative.
    ConflictingDefinitionMetadata,
    /// Selects the invalid pattern lexical alternative.
    InvalidPattern,
    /// Selects the pattern overlap lexical alternative.
    PatternOverlap,
    /// Selects the priority tie lexical alternative.
    PriorityTie,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the lexical remedy kind alternatives recognized by lexical planning.
pub(crate) enum LexicalRemedyKind {
    /// Selects the choose supported keyword matching policy lexical alternative.
    ChooseSupportedKeywordMatchingPolicy,
    /// Selects the use mechanical fixed token class lexical alternative.
    UseMechanicalFixedTokenClass,
    /// Selects the make definition metadata equivalent lexical alternative.
    MakeDefinitionMetadataEquivalent,
    /// Selects the use supported non empty pattern lexical alternative.
    UseSupportedNonEmptyPattern,
    /// Selects the add priority or separate patterns lexical alternative.
    AddPriorityOrSeparatePatterns,
    /// Selects the assign distinct priority lexical alternative.
    AssignDistinctPriority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the fixed token class alternatives recognized by lexical planning.
pub(crate) enum FixedTokenClass {
    /// Selects the keyword lexical alternative.
    Keyword,
    /// Selects the punctuation lexical alternative.
    Punctuation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the lexical diagnostic evidence alternatives recognized by lexical planning.
pub(crate) enum LexicalDiagnosticEvidence {
    /// Selects the invalid keyword matching policy lexical alternative.
    InvalidKeywordMatchingPolicy {
        /// Supplies the authored used to validate or execute the lexical plan.
        authored: String,
    },
    /// Selects the fixed token classification mismatch lexical alternative.
    FixedTokenClassificationMismatch {
        /// Supplies the spelling used to validate or execute the lexical plan.
        spelling: String,
        /// Supplies the authored used to validate or execute the lexical plan.
        authored: FixedTokenClass,
        /// Supplies the mechanical used to validate or execute the lexical plan.
        mechanical: FixedTokenClass,
    },
    /// Selects the conflicting token definition lexical alternative.
    ConflictingTokenDefinition {
        /// Supplies the identity used to validate or execute the lexical plan.
        identity: String,
        /// Supplies the normalized metadata used to validate or execute the lexical plan.
        normalized_metadata: Vec<LexicalDefinitionMetadata>,
    },
    /// Selects the invalid pattern lexical alternative.
    InvalidPattern {
        /// Preserves the authored matcher pattern for validation and generation.
        pattern: String,
        /// Supplies the reason used to validate or execute the lexical plan.
        reason: UnsupportedPatternReason,
    },
    /// Selects the pattern overlap lexical alternative.
    PatternOverlap {
        /// Supplies the contenders used to validate or execute the lexical plan.
        contenders: Vec<LexicalPatternEvidence>,
        /// Supplies the witness used to validate or execute the lexical plan.
        witness: String,
    },
    /// Selects the priority tie lexical alternative.
    PriorityTie {
        /// Supplies the contenders used to validate or execute the lexical plan.
        contenders: Vec<LexicalPatternEvidence>,
        /// Controls deterministic selection among competing lexical matchers.
        priority: u32,
        /// Supplies the witness used to validate or execute the lexical plan.
        witness: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the unsupported pattern reason alternatives recognized by lexical planning.
pub(crate) enum UnsupportedPatternReason {
    /// Selects the invalid or unsupported syntax lexical alternative.
    InvalidOrUnsupportedSyntax,
    /// Selects the word boundary lexical alternative.
    WordBoundary,
    /// Selects the anchor lexical alternative.
    Anchor,
    /// Selects the empty match lexical alternative.
    EmptyMatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the lexical definition metadata alternatives recognized by lexical planning.
pub(crate) enum LexicalDefinitionMetadata {
    /// Selects the fixed lexical alternative.
    Fixed {
        /// Supplies the class used to validate or execute the lexical plan.
        class: FixedTokenClass,
        /// Supplies the canonical spelling used to validate or execute the lexical plan.
        canonical_spelling: String,
    },
    /// Selects the content lexical alternative.
    Content {
        /// Preserves the authored matcher pattern for validation and generation.
        pattern: String,
        /// Preserves the closed scanning mechanism, when centrally declared.
        closed_matcher: Option<ClosedMatcher>,
        /// Preserves an explicitly authored pattern priority, when present.
        authored_priority: Option<u32>,
        /// Supplies the admitted fixed tokens used to validate or execute the lexical plan.
        admitted_fixed_tokens: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the lexical pattern evidence used to plan deterministic token recognition.
pub(crate) struct LexicalPatternEvidence {
    /// Preserves the authored matcher pattern for validation and generation.
    pub(crate) pattern: String,
    /// Lists parser targets that admit this token classification.
    pub(crate) target_names: Vec<String>,
    /// Preserves an explicitly authored pattern priority, when present.
    pub(crate) authored_priority: Option<u32>,
    /// Supplies the outcome used to validate or execute the lexical plan.
    pub(crate) outcome: LexicalPatternOutcome,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the lexical pattern outcome alternatives recognized by lexical planning.
pub(crate) enum LexicalPatternOutcome {
    /// Selects the emits token kind lexical alternative.
    EmitsTokenKind,
    /// Selects the non emitting lexical alternative.
    NonEmitting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Classifies the lexical remedy alternatives recognized by lexical planning.
pub(crate) enum LexicalRemedy {
    /// Selects the choose supported keyword matching policy lexical alternative.
    ChooseSupportedKeywordMatchingPolicy,
    /// Selects the use mechanical fixed token class lexical alternative.
    UseMechanicalFixedTokenClass {
        /// Supplies the class used to validate or execute the lexical plan.
        class: FixedTokenClass,
    },
    /// Selects the make definition metadata equivalent lexical alternative.
    MakeDefinitionMetadataEquivalent {
        /// Supplies the identity used to validate or execute the lexical plan.
        identity: String,
    },
    /// Selects the use supported non empty pattern lexical alternative.
    UseSupportedNonEmptyPattern,
    /// Selects the assign distinct priority lexical alternative.
    AssignDistinctPriority {
        /// Supplies the contenders used to validate or execute the lexical plan.
        contenders: Vec<LexicalPatternEvidence>,
    },
    /// Selects the make patterns disjoint lexical alternative.
    MakePatternsDisjoint {
        /// Supplies the contenders used to validate or execute the lexical plan.
        contenders: Vec<LexicalPatternEvidence>,
    },
}

/// Derives the non token rule groups under the grammar keyword and lexical-selection policies.
fn non_token_rule_groups(source: &SourceModeledGrammar) -> Vec<NonTokenRuleGroup> {
    let mut groups = Vec::<NonTokenRuleGroup>::new();
    for record in &source.source_model().lexer_records {
        if let Some(group) = groups.iter_mut().find(|group| {
            group.pattern == record.pattern
                && group.closed_matcher == record.closed_matcher
                && group.authored_priority == record.priority
                && group.outcome == record.outcome
        }) {
            group.sites.push(record.site.clone());
        } else {
            groups.push(NonTokenRuleGroup {
                pattern: record.pattern.clone(),
                closed_matcher: record.closed_matcher.clone(),
                authored_priority: record.priority,
                outcome: record.outcome,
                sites: NonEmpty::new(record.site.clone()),
            });
        }
    }
    for group in &mut groups {
        group.sites.sort_by(|left, right| {
            source_site_key(source, left).cmp(&source_site_key(source, right))
        });
    }
    groups.sort_by(|left, right| {
        (
            &left.pattern,
            &left.closed_matcher,
            left.authored_priority,
            non_token_source_outcome_key(left.outcome),
        )
            .cmp(&(
                &right.pattern,
                &right.closed_matcher,
                right.authored_priority,
                non_token_source_outcome_key(right.outcome),
            ))
    });
    groups
}

/// Derives the non token source outcome key under the grammar keyword and lexical-selection policies.
const fn non_token_source_outcome_key(outcome: crate::source_model::NonTokenLexerOutcome) -> u8 {
    match outcome {
        crate::source_model::NonTokenLexerOutcome::NonEmitting => 0,
    }
}

/// Plans non token outcome.
const fn plan_non_token_outcome(
    outcome: crate::source_model::NonTokenLexerOutcome,
) -> NonTokenLexicalOutcome {
    match outcome {
        crate::source_model::NonTokenLexerOutcome::NonEmitting => {
            NonTokenLexicalOutcome::NonEmitting
        }
    }
}

/// Derives the fixed matchers under the grammar keyword and lexical-selection policies.
fn fixed_matchers(model: &SourceModel, text_equivalence: TextEquivalence) -> Vec<FixedMatcher> {
    let mut fixed = BTreeMap::<String, (bool, bool)>::new();
    for definition in &model.token_definitions {
        let TokenDefinitionKind::Fixed(fixed_definition) = &definition.kind else {
            continue;
        };
        let word_shaped = is_word_shaped(&fixed_definition.spelling);
        let spelling = canonical_fixed_spelling(&fixed_definition.spelling, text_equivalence);
        fixed.entry(spelling).or_insert((
            word_shaped,
            word_shaped && text_equivalence == TextEquivalence::AsciiInsensitive,
        ));
    }
    fixed
        .into_iter()
        .map(
            |(spelling, (word_shaped, ascii_insensitive))| FixedMatcher {
                spelling,
                word_shaped,
                ascii_insensitive,
            },
        )
        .collect()
}

impl CarrierProofCache {
    /// Indexes carrier proofs under the grammar's text-equivalence policy.
    fn new(
        text_equivalence: TextEquivalence,
        content_groups: &[ContentBaseGroup<'_>],
        non_token_records: &[crate::source_model::NonTokenLexerRecord],
        fixed_matchers: &[FixedMatcher],
    ) -> Self {
        let spellings = fixed_matchers
            .iter()
            .filter(|matcher| matcher.word_shaped)
            .map(|matcher| matcher.spelling.clone())
            .collect::<Vec<_>>();
        if spellings.is_empty() {
            return Self {
                proofs: BTreeMap::new(),
                matcher_constructions: 0,
                proof_evaluations: 0,
            };
        }
        let prover = CarrierProver::new(content_groups, non_token_records);
        let matcher_constructions = prover.matcher_constructions;
        let proofs = spellings
            .into_iter()
            .map(|spelling| {
                let proof = prover
                    .prove(text_equivalence, &spelling)
                    .and_then(|groups| {
                        let (carrier, additional_carriers) = groups.split_first()?;
                        Some(SymbolicCarrierProof {
                            carrier: content_groups[*carrier].identity.clone(),
                            additional_carriers: additional_carriers
                                .iter()
                                .map(|&group| content_groups[group].identity.clone())
                                .collect(),
                        })
                    });
                (spelling, proof)
            })
            .collect::<BTreeMap<_, _>>();
        Self {
            proof_evaluations: proofs.len(),
            proofs,
            matcher_constructions,
        }
    }

    /// Derives the proof under the grammar keyword and lexical-selection policies.
    fn proof(&self, spelling: &str) -> Option<&SymbolicCarrierProof> {
        self.proofs.get(spelling).and_then(Option::as_ref)
    }
}

/// Derives the retained fixed matchers under the grammar keyword and lexical-selection policies.
fn retained_fixed_matchers(
    fixed_matchers: &[FixedMatcher],
    carrier_proofs: &CarrierProofCache,
) -> Vec<RetainedFixedMatcher> {
    fixed_matchers
        .iter()
        .filter(|matcher| !matcher.word_shaped || carrier_proofs.proof(&matcher.spelling).is_none())
        .map(|matcher| RetainedFixedMatcher {
            spelling: matcher.spelling.clone(),
            ascii_insensitive: matcher.ascii_insensitive,
        })
        .collect()
}

/// Plans lexical.
pub(crate) fn plan_lexical(source: &SourceModeledGrammar) -> LexicalPlanReport {
    // Resolve definitions, validate patterns, prove carriers, and assign priority in order; each phase consumes only validated facts.
    let model = source.source_model();
    let text_equivalence = source.resolved().configuration.syntax.text_equivalence;
    let content_groups = content_base_groups(model);
    let non_token_groups = non_token_rule_groups(source);
    let fixed_matchers = fixed_matchers(model, text_equivalence);
    let carrier_proofs = CarrierProofCache::new(
        text_equivalence,
        &content_groups,
        &model.lexer_records,
        &fixed_matchers,
    );
    let retained_fixed_matchers = retained_fixed_matchers(&fixed_matchers, &carrier_proofs);
    let mut diagnostics = source
        .resolved()
        .configuration
        .syntax
        .invalid_keyword_matching
        .as_ref()
        .map(|invalid| LexicalDiagnostic {
            code: LexicalDiagnosticCode::Rca3001,
            sites: vec![source.grammar_configuration_site(invalid.range.clone())],
            evidence: LexicalDiagnosticEvidence::InvalidKeywordMatchingPolicy {
                authored: invalid.authored.clone(),
            },
            remedies: vec![LexicalRemedy::ChooseSupportedKeywordMatchingPolicy],
        })
        .into_iter()
        .collect::<Vec<_>>();
    diagnostics.extend(fixed_classification_diagnostics(source));
    diagnostics.extend(definition_conflict_diagnostics(source, text_equivalence));
    diagnostics.extend(lexical_diagnostics(
        source,
        &content_groups,
        &non_token_groups,
        &retained_fixed_matchers,
    ));
    diagnostics.sort_by_key(|diagnostic| diagnostic_key(source, diagnostic));
    if !diagnostics.is_empty() {
        return LexicalPlanReport {
            diagnostics,
            plan: None,
        };
    }
    let mut identity_sites = Vec::<(TokenKindIdentity, crate::source_model::SourceOrderKey)>::new();
    for definition in &model.token_definitions {
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            continue;
        };
        let identity =
            TokenKindIdentity::Fixed(canonical_fixed_spelling(&fixed.spelling, text_equivalence));
        let site = definition
            .definition_sites
            .iter()
            .map(|site| source_site_key(source, site))
            .min()
            .unwrap_or(crate::source_model::SourceOrderKey::LAST);
        if let Some((_, existing)) = identity_sites
            .iter_mut()
            .find(|(candidate, _)| candidate == &identity)
        {
            *existing = (*existing).min(site);
        } else {
            identity_sites.push((identity, site));
        }
    }
    for group in &content_groups {
        let identity = TokenKindIdentity::ContentBase(group.identity.clone());
        let site = group
            .sites()
            .iter()
            .map(|site| source_site_key(source, site))
            .min()
            .unwrap_or(crate::source_model::SourceOrderKey::LAST);
        identity_sites.push((identity, site));
    }
    let mut ordered_identities = identity_sites;
    ordered_identities.sort_by(|(left_identity, left_site), (right_identity, right_site)| {
        (left_site, kind_label(left_identity)).cmp(&(right_site, kind_label(right_identity)))
    });
    let token_kinds = ordered_identities
        .into_iter()
        .map(|(identity, _)| identity)
        .enumerate()
        .map(|(index, identity)| TokenKindPlan {
            id: KindId::new(index.try_into().expect("token kind identity overflow")),
            identity,
        })
        .collect::<Vec<_>>();
    let kind_by_content_base = content_groups
        .iter()
        .map(|group| {
            (
                group.identity.clone(),
                token_kind_by_identity(
                    &token_kinds,
                    &TokenKindIdentity::ContentBase(group.identity.clone()),
                ),
            )
        })
        .collect::<Vec<_>>();
    let definition_kinds = model
        .token_definitions
        .iter()
        .map(|definition| {
            let identity = match &definition.kind {
                TokenDefinitionKind::Fixed(fixed) => TokenKindIdentity::Fixed(
                    canonical_fixed_spelling(&fixed.spelling, text_equivalence),
                ),
                TokenDefinitionKind::Content(content) => {
                    return (
                        definition.id,
                        content_base_kind(&kind_by_content_base, &content_base_identity(content)),
                    );
                }
                TokenDefinitionKind::LexicalBase(base) => {
                    return (
                        definition.id,
                        content_base_kind(
                            &kind_by_content_base,
                            &ContentBaseIdentity {
                                pattern: base.pattern.clone(),
                                closed_matcher: base.closed_matcher.clone(),
                                priority: base.priority,
                            },
                        ),
                    );
                }
            };
            (
                definition.id,
                token_kind_by_identity(&token_kinds, &identity),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut content_bases = content_groups
        .iter()
        .map(|group| {
            let mut target_names = group
                .definitions
                .iter()
                .filter(|definition| matches!(definition.kind, TokenDefinitionKind::Content(_)))
                .map(|definition| definition.name.clone())
                .collect::<Vec<_>>();
            target_names.sort();
            ContentBasePlan {
                kind: content_base_kind(&kind_by_content_base, &group.identity),
                target_names,
                pattern: group.identity.pattern.clone(),
                closed_matcher: group.identity.closed_matcher.clone(),
                authored_priority: group.identity.priority,
            }
        })
        .collect::<Vec<_>>();
    content_bases.sort_by_key(|base| base.kind);
    let mut rules_by_key = BTreeMap::new();
    let mut keywords_by_spelling = BTreeMap::<String, (KindId, Vec<SourceSite>)>::new();
    for definition in &model.token_definitions {
        let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
            continue;
        };
        let keyword = is_word_shaped(&fixed.spelling);
        let canonical_spelling = match (keyword, text_equivalence) {
            (true, TextEquivalence::AsciiInsensitive) => fixed.spelling.to_ascii_uppercase(),
            _ => fixed.spelling.clone(),
        };
        let kind = definition_kinds[&definition.id];
        let role = if keyword {
            let (_, sites) = keywords_by_spelling
                .entry(canonical_spelling.clone())
                .or_insert_with(|| (kind, Vec::new()));
            sites.extend(definition.definition_sites.iter().cloned());
            LexicalRuleRole::StandaloneKeyword
        } else {
            LexicalRuleRole::StandaloneFixed
        };
        let matcher = LexicalMatcher::Fixed(canonical_spelling);
        rules_by_key
            .entry((role, matcher.clone(), kind))
            .or_insert(LexicalRule {
                matcher,
                closed_matcher: None,
                kind,
                role,
                priority: LexicalPriority::Fixed,
            });
    }
    for group in &content_groups {
        let kind = content_base_kind(&kind_by_content_base, &group.identity);
        let role = LexicalRuleRole::ContentPattern;
        let matcher = LexicalMatcher::Regex(group.identity.pattern.clone());
        rules_by_key
            .entry((role, matcher.clone(), kind))
            .or_insert(LexicalRule {
                matcher,
                closed_matcher: group.identity.closed_matcher.clone(),
                kind,
                role,
                priority: LexicalPriority::Pattern {
                    authored: group.identity.priority,
                },
            });
    }
    let mut keywords = keywords_by_spelling
        .into_iter()
        .map(|(canonical_spelling, (kind, mut definition_sites))| {
            definition_sites.sort_by_key(|site| source_site_key(source, site));
            let recognition = carrier_proofs.proof(&canonical_spelling).map_or(
                KeywordRecognition::Standalone,
                |proof| {
                    let mut kinds = proof
                        .identities()
                        .map(|identity| content_base_kind(&kind_by_content_base, identity))
                        .collect::<Vec<_>>();
                    kinds.sort_unstable();
                    kinds.dedup();
                    let (carrier, additional_carriers) = kinds
                        .split_first()
                        .expect("a successful carrier proof is nonempty");
                    KeywordRecognition::CombinedCarriers {
                        carrier: *carrier,
                        additional_carriers: additional_carriers.to_vec(),
                    }
                },
            );
            KeywordPlan {
                canonical_spelling,
                kind,
                recognition,
                definition_sites,
            }
        })
        .collect::<Vec<_>>();
    keywords.sort_by(|left, right| left.canonical_spelling.cmp(&right.canonical_spelling));
    for keyword in &keywords {
        if matches!(
            keyword.recognition,
            KeywordRecognition::CombinedCarriers { .. }
        ) {
            rules_by_key.remove(&(
                LexicalRuleRole::StandaloneKeyword,
                LexicalMatcher::Fixed(keyword.canonical_spelling.clone()),
                keyword.kind,
            ));
        }
    }
    let mut rules = rules_by_key.into_values().collect::<Vec<_>>();
    rules.sort_by_key(|rule| (rule.role, rule.matcher.clone(), rule.kind));
    let parser_targets = model
        .token_definitions
        .iter()
        .filter_map(|definition| {
            let TokenDefinitionKind::Content(content) = &definition.kind else {
                return None;
            };
            let base_kind = definition_kinds[&definition.id];
            let admitted_kinds = KindSet::from_kinds(
                token_kinds.len(),
                content
                    .admission_set
                    .iter()
                    .map(|definition| definition_kinds[definition])
                    .filter(|kind| *kind != base_kind),
            )
            .expect("parser target admissions use planned grammar kinds");
            Some(ParserTargetPlan {
                definition: definition.id,
                semantic: content.semantic,
                name: definition.name.clone(),
                base_kind,
                admitted_kinds,
            })
        })
        .collect::<Vec<_>>();
    let plan = LexicalPlan {
        text_equivalence,
        selection_order: LexicalSelectionOrder::LongestPrefixThenFixedThenAuthoredPatternPriority,
        token_kinds,
        content_bases,
        keywords,
        parser_targets,
        rules,
        non_token_rules: non_token_groups
            .into_iter()
            .map(|group| NonTokenLexicalRule {
                matcher: LexicalMatcher::Regex(group.pattern),
                closed_matcher: group.closed_matcher,
                priority: LexicalPriority::Pattern {
                    authored: group.authored_priority,
                },
                outcome: plan_non_token_outcome(group.outcome),
                sites: group.sites,
            })
            .collect(),
        lowering_map: LexicalLoweringMap {
            token_definitions: definition_kinds
                .into_iter()
                .map(|(source, lowered)| TokenDefinitionKindCorrelation { source, lowered })
                .collect(),
        },
    };
    LexicalPlanReport {
        diagnostics,
        plan: Some(plan),
    }
}

/// Derives the fixed classification diagnostics under the grammar keyword and lexical-selection policies.
fn fixed_classification_diagnostics(source: &SourceModeledGrammar) -> Vec<LexicalDiagnostic> {
    source
        .source_model()
        .token_definitions
        .iter()
        .filter_map(|definition| {
            let TokenDefinitionKind::Fixed(fixed) = &definition.kind else {
                return None;
            };
            let mechanical = if is_word_shaped(&fixed.spelling) {
                FixedTokenClass::Keyword
            } else {
                FixedTokenClass::Punctuation
            };
            let mismatches = fixed
                .authored_classifications
                .iter()
                .filter(|classification| authored_fixed_class(classification.class) != mechanical)
                .collect::<Vec<_>>();
            (!mismatches.is_empty()).then(|| LexicalDiagnostic {
                code: LexicalDiagnosticCode::Rca3002,
                sites: sorted_sites(
                    source,
                    mismatches
                        .iter()
                        .map(|classification| classification.site.clone())
                        .collect(),
                ),
                evidence: LexicalDiagnosticEvidence::FixedTokenClassificationMismatch {
                    spelling: fixed.spelling.clone(),
                    authored: authored_fixed_class(mismatches[0].class),
                    mechanical,
                },
                remedies: vec![LexicalRemedy::UseMechanicalFixedTokenClass { class: mechanical }],
            })
        })
        .collect()
}

/// Derives the authored fixed class under the grammar keyword and lexical-selection policies.
const fn authored_fixed_class(class: AuthoredFixedTokenClass) -> FixedTokenClass {
    match class {
        AuthoredFixedTokenClass::Keyword => FixedTokenClass::Keyword,
        AuthoredFixedTokenClass::Punctuation => FixedTokenClass::Punctuation,
    }
}

/// Derives the definition conflict diagnostics under the grammar keyword and lexical-selection policies.
fn definition_conflict_diagnostics(
    source: &SourceModeledGrammar,
    text_equivalence: TextEquivalence,
) -> Vec<LexicalDiagnostic> {
    let model = source.source_model();
    let mut by_identity =
        BTreeMap::<(bool, String), Vec<&crate::source_model::TokenDefinition>>::new();
    for definition in &model.token_definitions {
        let identity = match &definition.kind {
            TokenDefinitionKind::Fixed(_) => (false, definition.name.clone()),
            TokenDefinitionKind::Content(_) | TokenDefinitionKind::LexicalBase(_) => {
                (true, definition.name.clone())
            }
        };
        by_identity.entry(identity).or_default().push(definition);
    }
    by_identity
        .into_iter()
        .filter_map(|((_, identity), definitions)| {
            let metadata = definitions
                .iter()
                .map(|definition| match &definition.kind {
                    TokenDefinitionKind::Fixed(fixed) => LexicalDefinitionMetadata::Fixed {
                        class: if is_word_shaped(&fixed.spelling) {
                            FixedTokenClass::Keyword
                        } else {
                            FixedTokenClass::Punctuation
                        },
                        canonical_spelling: canonical_fixed_spelling(
                            &fixed.spelling,
                            text_equivalence,
                        ),
                    },
                    TokenDefinitionKind::Content(content) => {
                        let mut admitted_fixed_tokens = content
                            .admission_set
                            .iter()
                            .map(|admission| {
                                let fixed = &model.token_definitions[admission.index()];
                                let TokenDefinitionKind::Fixed(fixed) = &fixed.kind else {
                                    unreachable!("content admissions reference only fixed tokens")
                                };
                                canonical_fixed_spelling(&fixed.spelling, text_equivalence)
                            })
                            .collect::<Vec<_>>();
                        admitted_fixed_tokens.sort();
                        admitted_fixed_tokens.dedup();
                        LexicalDefinitionMetadata::Content {
                            pattern: content.pattern.clone(),
                            closed_matcher: content.closed_matcher.clone(),
                            authored_priority: content.priority,
                            admitted_fixed_tokens,
                        }
                    }
                    TokenDefinitionKind::LexicalBase(base) => LexicalDefinitionMetadata::Content {
                        pattern: base.pattern.clone(),
                        closed_matcher: base.closed_matcher.clone(),
                        authored_priority: base.priority,
                        admitted_fixed_tokens: Vec::new(),
                    },
                })
                .collect::<Vec<_>>();
            let mut normalized_metadata = Vec::new();
            for definition in metadata {
                if !normalized_metadata.contains(&definition) {
                    normalized_metadata.push(definition);
                }
            }
            normalized_metadata.sort_by_key(definition_metadata_key);
            (normalized_metadata.len() > 1).then(|| LexicalDiagnostic {
                code: LexicalDiagnosticCode::Rca3003,
                sites: sorted_sites(
                    source,
                    definitions
                        .iter()
                        .flat_map(|definition| definition.definition_sites.iter().cloned())
                        .collect(),
                ),
                evidence: LexicalDiagnosticEvidence::ConflictingTokenDefinition {
                    identity: identity.clone(),
                    normalized_metadata,
                },
                remedies: vec![LexicalRemedy::MakeDefinitionMetadataEquivalent { identity }],
            })
        })
        .collect()
}

/// Derives the definition metadata key under the grammar keyword and lexical-selection policies.
fn definition_metadata_key(metadata: &LexicalDefinitionMetadata) -> String {
    match metadata {
        LexicalDefinitionMetadata::Fixed {
            class,
            canonical_spelling,
        } => format!("fixed:{class:?}:{canonical_spelling:?}"),
        LexicalDefinitionMetadata::Content {
            pattern,
            closed_matcher,
            authored_priority,
            admitted_fixed_tokens,
        } => {
            let closed = closed_matcher
                .as_ref()
                .map_or_else(String::new, |matcher| format!(":closed={matcher:?}"));
            format!(
                "content:{pattern:?}{closed}:priority={authored_priority:?}:admits={admitted_fixed_tokens:?}",
            )
        }
    }
}

/// Derives the content base identity under the grammar keyword and lexical-selection policies.
fn content_base_identity(
    content: &crate::source_model::ContentTokenDefinition,
) -> ContentBaseIdentity {
    ContentBaseIdentity {
        pattern: content.pattern.clone(),
        closed_matcher: content.closed_matcher.clone(),
        priority: content.priority,
    }
}

/// Derives the content base groups under the grammar keyword and lexical-selection policies.
fn content_base_groups(source: &SourceModel) -> Vec<ContentBaseGroup<'_>> {
    let mut grouped = Vec::<ContentBaseGroup<'_>>::new();
    for definition in &source.token_definitions {
        let Some(identity) = content_base_identity_for_definition(definition) else {
            continue;
        };
        if let Some(group) = grouped.iter_mut().find(|group| group.identity == identity) {
            group.definitions.push(definition);
        } else {
            grouped.push(ContentBaseGroup {
                identity,
                definitions: NonEmpty::new(definition),
            });
        }
    }
    for group in &mut grouped {
        group
            .definitions
            .sort_by(|left, right| left.name.cmp(&right.name));
    }
    grouped.sort_by(|left, right| {
        (
            &left.identity.pattern,
            &left.identity.closed_matcher,
            left.identity.priority,
        )
            .cmp(&(
                &right.identity.pattern,
                &right.identity.closed_matcher,
                right.identity.priority,
            ))
    });
    grouped
}

/// Derives the content base identity for definition under the grammar keyword and lexical-selection policies.
fn content_base_identity_for_definition(
    definition: &crate::source_model::TokenDefinition,
) -> Option<ContentBaseIdentity> {
    match &definition.kind {
        TokenDefinitionKind::Fixed(_) => None,
        TokenDefinitionKind::Content(content) => Some(content_base_identity(content)),
        TokenDefinitionKind::LexicalBase(base) => Some(ContentBaseIdentity {
            pattern: base.pattern.clone(),
            closed_matcher: base.closed_matcher.clone(),
            priority: base.priority,
        }),
    }
}

/// Derives the token kind by identity under the grammar keyword and lexical-selection policies.
fn token_kind_by_identity(kinds: &[TokenKindPlan], identity: &TokenKindIdentity) -> KindId {
    kinds
        .iter()
        .find_map(|kind| (&kind.identity == identity).then_some(kind.id))
        .expect("every token-kind identity receives one kind")
}

/// Derives the content base kind under the grammar keyword and lexical-selection policies.
fn content_base_kind(
    kinds: &[(ContentBaseIdentity, KindId)],
    identity: &ContentBaseIdentity,
) -> KindId {
    kinds
        .iter()
        .find_map(|(candidate, kind)| (candidate == identity).then_some(*kind))
        .expect("every content base receives one token kind")
}

/// Derives the lexical diagnostics under the grammar keyword and lexical-selection policies.
fn lexical_diagnostics(
    source: &SourceModeledGrammar,
    content_groups: &[ContentBaseGroup<'_>],
    non_token_groups: &[NonTokenRuleGroup],
    retained_fixed_matchers: &[RetainedFixedMatcher],
) -> Vec<LexicalDiagnostic> {
    // Adapt all independent lexical findings before sorting, preserving stable codes and authored source sites.
    let mut diagnostics = Vec::new();
    let mut contenders = Vec::new();
    for group in non_token_groups {
        match validate_pattern(&group.pattern) {
            Ok(()) => contenders.push(PatternContender {
                pattern: group.pattern.clone(),
                target_names: Vec::new(),
                authored_priority: group.authored_priority,
                priority_authorable: group.closed_matcher.is_some(),
                final_extent: closed_pattern_final_extent(group.closed_matcher.as_ref()),
                outcome: match group.outcome {
                    crate::source_model::NonTokenLexerOutcome::NonEmitting => {
                        LexicalPatternOutcome::NonEmitting
                    }
                },
                sites: group.sites.iter().cloned().collect(),
            }),
            Err(reason) => diagnostics.push(LexicalDiagnostic {
                code: LexicalDiagnosticCode::Rca3004,
                sites: sorted_sites(source, group.sites.iter().cloned().collect()),
                evidence: LexicalDiagnosticEvidence::InvalidPattern {
                    pattern: group.pattern.clone(),
                    reason,
                },
                remedies: vec![LexicalRemedy::UseSupportedNonEmptyPattern],
            }),
        }
    }
    for group in content_groups {
        match validate_pattern(&group.identity.pattern) {
            Ok(()) => contenders.push(PatternContender {
                pattern: group.identity.pattern.clone(),
                target_names: group
                    .definitions
                    .iter()
                    .map(|definition| definition.name.clone())
                    .collect(),
                authored_priority: group.identity.priority,
                priority_authorable: group.priority_authorable(),
                final_extent: closed_pattern_final_extent(group.identity.closed_matcher.as_ref()),
                outcome: LexicalPatternOutcome::EmitsTokenKind,
                sites: group.sites(),
            }),
            Err(reason) => diagnostics.push(LexicalDiagnostic {
                code: LexicalDiagnosticCode::Rca3004,
                sites: sorted_sites(source, group.sites()),
                evidence: LexicalDiagnosticEvidence::InvalidPattern {
                    pattern: group.identity.pattern.clone(),
                    reason,
                },
                remedies: vec![LexicalRemedy::UseSupportedNonEmptyPattern],
            }),
        }
    }
    contenders.sort_by(|left, right| {
        (
            &left.pattern,
            left.authored_priority,
            left.outcome,
            &left.target_names,
            left.final_extent,
        )
            .cmp(&(
                &right.pattern,
                right.authored_priority,
                right.outcome,
                &right.target_names,
                right.final_extent,
            ))
    });
    for ambiguity in pattern_ambiguities(&contenders, retained_fixed_matchers) {
        let indices = ambiguity.indices;
        let witness = ambiguity.witness;
        let evidence_contenders = indices
            .iter()
            .map(|&index| pattern_evidence(&contenders[index]))
            .collect::<Vec<_>>();
        let mut sites = indices
            .iter()
            .flat_map(|&index| contenders[index].sites.iter().cloned())
            .collect::<Vec<_>>();
        sites = sorted_sites(source, sites);
        let highest = indices
            .iter()
            .map(|&index| contenders[index].authored_priority.unwrap_or(0))
            .max()
            .expect("an ambiguity has contenders");
        let (code, evidence) = if indices
            .iter()
            .any(|&index| contenders[index].authored_priority.is_none())
        {
            (
                LexicalDiagnosticCode::Rca3005,
                LexicalDiagnosticEvidence::PatternOverlap {
                    contenders: evidence_contenders.clone(),
                    witness,
                },
            )
        } else {
            (
                LexicalDiagnosticCode::Rca3006,
                LexicalDiagnosticEvidence::PriorityTie {
                    contenders: evidence_contenders.clone(),
                    priority: highest,
                    witness,
                },
            )
        };
        let mut remedies = Vec::new();
        if indices
            .iter()
            .any(|&index| contenders[index].priority_authorable)
        {
            remedies.push(LexicalRemedy::AssignDistinctPriority {
                contenders: evidence_contenders.clone(),
            });
        }
        remedies.push(LexicalRemedy::MakePatternsDisjoint {
            contenders: evidence_contenders,
        });
        diagnostics.push(LexicalDiagnostic {
            code,
            sites,
            evidence,
            remedies,
        });
    }
    diagnostics
}

/// Classifies whether a closed matcher changes the extent recognized by its starter pattern.
fn closed_pattern_final_extent(matcher: Option<&ClosedMatcher>) -> PatternFinalExtent {
    match matcher {
        Some(ClosedMatcher::NextExclusion { .. }) | None => PatternFinalExtent::Exact,
        Some(_) => PatternFinalExtent::Closed,
    }
}

/// Validates pattern.
fn validate_pattern(pattern: &str) -> Result<(), UnsupportedPatternReason> {
    let hir = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|_| UnsupportedPatternReason::InvalidOrUnsupportedSyntax)?;
    let look = hir.properties().look_set();
    if look.contains_word() {
        return Err(UnsupportedPatternReason::WordBoundary);
    }
    if look.contains_anchor() {
        return Err(UnsupportedPatternReason::Anchor);
    }
    let dfa = dense::DFA::new(pattern)
        .map_err(|_| UnsupportedPatternReason::InvalidOrUnsupportedSyntax)?;
    if accepts_empty(&dfa) {
        return Err(UnsupportedPatternReason::EmptyMatch);
    }
    Ok(())
}

/// Derives the pattern evidence under the grammar keyword and lexical-selection policies.
fn pattern_evidence(contender: &PatternContender) -> LexicalPatternEvidence {
    LexicalPatternEvidence {
        pattern: contender.pattern.clone(),
        target_names: contender.target_names.clone(),
        authored_priority: contender.authored_priority,
        outcome: contender.outcome,
    }
}

/// Derives the accepts empty under the grammar keyword and lexical-selection policies.
fn accepts_empty<A: regex_automata::dfa::Automaton>(dfa: &A) -> bool {
    let input = Input::new(b"").anchored(Anchored::Yes);
    let Ok(state) = dfa.start_state_forward(&input) else {
        return false;
    };
    dfa.is_match_state(dfa.next_eoi_state(state))
}

/// One witnessed ambiguity that source analysis must reject before runtime rule selection.
struct PatternAmbiguity {
    /// Indices of the simultaneously applicable contenders.
    indices: Vec<usize>,
    /// Shortest known source prefix on which the contenders are simultaneously applicable.
    witness: String,
}

/// Derives the pattern ambiguities under the grammar keyword and lexical-selection policies.
fn pattern_ambiguities(
    contenders: &[PatternContender],
    retained_fixed_matchers: &[RetainedFixedMatcher],
) -> Vec<PatternAmbiguity> {
    if contenders.len() < 2 {
        return Vec::new();
    }
    let patterns = contenders
        .iter()
        .map(|contender| contender.pattern.as_str())
        .collect::<Vec<_>>();
    let Some(dfa) = all_matches_dfa(&patterns) else {
        return Vec::new();
    };
    let input = Input::new(b"").anchored(Anchored::Yes);
    let Ok(start) = dfa.start_state_forward(&input) else {
        return Vec::new();
    };
    let fixed_progress = retained_fixed_matchers
        .iter()
        .enumerate()
        .map(|(index, _)| (index, 0_usize))
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::from([(start, fixed_progress.clone())]);
    let mut frontier = VecDeque::from([(start, Vec::<u8>::new(), fixed_progress)]);
    let mut ambiguities = BTreeMap::<Vec<usize>, String>::new();
    while let Some((state, witness, fixed_progress)) = frontier.pop_front() {
        let end = dfa.next_eoi_state(state);
        let fixed_wins = fixed_progress
            .iter()
            .any(|&(index, consumed)| consumed == retained_fixed_matchers[index].spelling.len());
        if !fixed_wins && !witness.is_empty() && dfa.is_match_state(end) {
            let matches = (0..dfa.match_len(end))
                .map(|index| dfa.match_pattern(end, index).as_usize())
                .collect::<Vec<_>>();
            let highest = matches
                .iter()
                .map(|&index| contenders[index].authored_priority.unwrap_or(0))
                .max()
                .unwrap_or(0);
            let contenders = matches
                .into_iter()
                .filter(|&index| contenders[index].authored_priority.unwrap_or(0) == highest)
                .collect::<Vec<_>>();
            if contenders.len() > 1
                && let Ok(witness) = String::from_utf8(witness.clone())
            {
                ambiguities.entry(contenders).or_insert(witness);
            }
        }
        for byte in u8::MIN..=u8::MAX {
            let next = dfa.next_state(state, byte);
            if dfa.is_dead_state(next) || dfa.is_quit_state(next) {
                continue;
            }
            let next_fixed_progress = fixed_progress
                .iter()
                .filter_map(|&(index, consumed)| {
                    let matcher = &retained_fixed_matchers[index];
                    let expected = *matcher.spelling.as_bytes().get(consumed)?;
                    (expected == byte
                        || (matcher.ascii_insensitive && expected.eq_ignore_ascii_case(&byte)))
                    .then_some((index, consumed + 1))
                })
                .collect::<Vec<_>>();
            if !seen.insert((next, next_fixed_progress.clone())) {
                continue;
            }
            let mut next_witness = witness.clone();
            next_witness.push(byte);
            frontier.push_back((next, next_witness, next_fixed_progress));
        }
    }
    let mut ambiguities = ambiguities
        .into_iter()
        .map(|(indices, witness)| PatternAmbiguity { indices, witness })
        .collect::<Vec<_>>();

    for left in 0..contenders.len() {
        for right in left + 1..contenders.len() {
            let left_extent = contenders[left].final_extent;
            let right_extent = contenders[right].final_extent;
            let finalizing = left_extent == PatternFinalExtent::Closed
                || right_extent == PatternFinalExtent::Closed;
            if !finalizing
                || contenders[left].authored_priority.unwrap_or(0)
                    != contenders[right].authored_priority.unwrap_or(0)
            {
                continue;
            }
            if ambiguities.iter().any(|ambiguity| {
                ambiguity.indices.contains(&left) && ambiguity.indices.contains(&right)
            }) {
                continue;
            }
            let Some(witness) =
                prefix_overlap_witness(&contenders[left].pattern, &contenders[right].pattern)
            else {
                continue;
            };
            ambiguities.push(PatternAmbiguity {
                indices: vec![left, right],
                witness,
            });
        }
    }
    ambiguities.sort_by(|left, right| {
        (&left.indices, &left.witness).cmp(&(&right.indices, &right.witness))
    });
    ambiguities
}

/// Finds a valid UTF-8 prefix on which both anchored patterns have accepted, at possibly different ends.
fn prefix_overlap_witness(left: &str, right: &str) -> Option<String> {
    type State = (
        Option<regex_automata::util::primitives::StateID>,
        Option<regex_automata::util::primitives::StateID>,
        bool,
        bool,
    );

    let (Ok(left_dfa), Ok(right_dfa)) = (dense::DFA::new(left), dense::DFA::new(right)) else {
        return None;
    };
    let input = Input::new(b"").anchored(Anchored::Yes);
    let (Ok(left_start), Ok(right_start)) = (
        left_dfa.start_state_forward(&input),
        right_dfa.start_state_forward(&input),
    ) else {
        return None;
    };
    let start = (Some(left_start), Some(right_start), false, false);
    let mut seen = BTreeSet::from([start]);
    let mut frontier = VecDeque::from([(start, Vec::<u8>::new())]);
    while let Some(((left_state, right_state, left_matched, right_matched), witness)) =
        frontier.pop_front()
    {
        let left_matched =
            left_matched || left_state.is_some_and(|state| dfa_accepts(&left_dfa, state));
        let right_matched =
            right_matched || right_state.is_some_and(|state| dfa_accepts(&right_dfa, state));
        if left_matched
            && right_matched
            && !witness.is_empty()
            && let Ok(witness) = String::from_utf8(witness.clone())
        {
            return Some(witness);
        }
        for byte in u8::MIN..=u8::MAX {
            let advance =
                |dfa: &dense::DFA<Vec<u32>>,
                 state: Option<regex_automata::util::primitives::StateID>| {
                    state.and_then(|state| {
                        let next = dfa.next_state(state, byte);
                        (!dfa.is_dead_state(next) && !dfa.is_quit_state(next)).then_some(next)
                    })
                };
            let next_left = advance(&left_dfa, left_state);
            let next_right = advance(&right_dfa, right_state);
            if (next_left.is_none() && !left_matched) || (next_right.is_none() && !right_matched) {
                continue;
            }
            let next = (next_left, next_right, left_matched, right_matched);
            if !seen.insert(next) {
                continue;
            }
            let mut next_witness = witness.clone();
            next_witness.push(byte);
            frontier.push_back((next, next_witness));
        }
    }
    None
}

/// Reports whether one anchored DFA state accepts at end of input.
fn dfa_accepts(
    dfa: &dense::DFA<Vec<u32>>,
    state: regex_automata::util::primitives::StateID,
) -> bool {
    dfa.is_match_state(dfa.next_eoi_state(state))
}

/// Builds the shared all-matches DFA used by anchored lexical-policy analysis.
fn all_matches_dfa(patterns: &[&str]) -> Option<dense::DFA<Vec<u32>>> {
    let mut builder = dense::Builder::new();
    builder.configure(
        dense::DFA::config()
            .match_kind(MatchKind::All)
            .start_kind(StartKind::Anchored),
    );
    builder.build_many(patterns).ok()
}

/// Derives the sorted sites under the grammar keyword and lexical-selection policies.
fn sorted_sites(source: &SourceModeledGrammar, mut sites: Vec<SourceSite>) -> Vec<SourceSite> {
    sites.sort_by_key(|site| source_site_key(source, site));
    sites
}

/// Derives the diagnostic key under the grammar keyword and lexical-selection policies.
fn diagnostic_key(
    source: &SourceModeledGrammar,
    diagnostic: &LexicalDiagnostic,
) -> (crate::source_model::SourceOrderKey, LexicalDiagnosticCode) {
    let Some(site) = diagnostic.sites.first() else {
        return (crate::source_model::SourceOrderKey::LAST, diagnostic.code);
    };
    (source_site_key(source, site), diagnostic.code)
}

/// Derives the source site key under the grammar keyword and lexical-selection policies.
fn source_site_key(
    source: &SourceModeledGrammar,
    site: &SourceSite,
) -> crate::source_model::SourceOrderKey {
    source.source_order_key(site)
}

impl CarrierProver {
    /// Prepares reusable matcher state for proving keyword-carrier relationships.
    fn new(
        content_groups: &[ContentBaseGroup<'_>],
        non_token_records: &[crate::source_model::NonTokenLexerRecord],
    ) -> Self {
        let mut patterns = content_groups
            .iter()
            .map(|group| group.identity.pattern.as_str())
            .collect::<Vec<_>>();
        let mut candidates = content_groups
            .iter()
            .enumerate()
            .map(|(group, definition)| CarrierCandidate::Emitting {
                group,
                starter_not_final: definition.identity.closed_matcher.is_some(),
                priority: definition.identity.priority.unwrap_or(0),
            })
            .collect::<Vec<_>>();
        patterns.extend(
            non_token_records
                .iter()
                .map(|record| record.pattern.as_str()),
        );
        candidates.extend(
            non_token_records
                .iter()
                .map(|record| CarrierCandidate::NonEmitting {
                    priority: record.priority.unwrap_or(0),
                }),
        );
        let (dfa, matcher_constructions) = if patterns.is_empty() {
            (None, 0)
        } else {
            (all_matches_dfa(&patterns), 1)
        };
        Self {
            dfa,
            candidates,
            matcher_constructions,
        }
    }

    /// Returns matching candidate indexes when the combined DFA accepts the spelling.
    fn prove(&self, text_equivalence: TextEquivalence, spelling: &str) -> Option<Vec<usize>> {
        let dfa = self.dfa.as_ref()?;
        let input = Input::new(b"").anchored(Anchored::Yes);
        let start = dfa.start_state_forward(&input).ok()?;
        let mut states = BTreeSet::from([start]);
        for character in spelling.chars() {
            let choices = if text_equivalence == TextEquivalence::AsciiInsensitive
                && character.is_ascii_alphabetic()
            {
                vec![
                    character.to_ascii_lowercase(),
                    character.to_ascii_uppercase(),
                ]
            } else {
                vec![character]
            };
            let mut next = BTreeSet::new();
            for state in states {
                for choice in &choices {
                    let mut encoded = [0_u8; 4];
                    let mut candidate = state;
                    for &byte in choice.encode_utf8(&mut encoded).as_bytes() {
                        candidate = dfa.next_state(candidate, byte);
                    }
                    next.insert(candidate);
                }
            }
            states = next;
        }
        let mut groups = BTreeSet::new();
        for state in states {
            let state = dfa.next_eoi_state(state);
            if !dfa.is_match_state(state) {
                return None;
            }
            let winning_priority = (0..dfa.match_len(state))
                .map(|index| self.candidates[dfa.match_pattern(state, index).as_usize()].priority())
                .max()?;
            let mut winning_group = None;
            for index in 0..dfa.match_len(state) {
                let candidate = &self.candidates[dfa.match_pattern(state, index).as_usize()];
                if candidate.priority() != winning_priority {
                    continue;
                }
                match candidate {
                    CarrierCandidate::Emitting {
                        group,
                        starter_not_final: false,
                        ..
                    } => {
                        if winning_group.is_some_and(|winner| winner != *group) {
                            return None;
                        }
                        winning_group = Some(*group);
                    }
                    CarrierCandidate::Emitting {
                        starter_not_final: true,
                        ..
                    }
                    | CarrierCandidate::NonEmitting { .. } => return None,
                }
            }
            groups.insert(winning_group?);
        }
        Some(groups.into_iter().collect())
    }
}

/// Reports whether this value is word shaped.
fn is_word_shaped(spelling: &str) -> bool {
    let mut characters = spelling.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || unicode_ident::is_xid_start(first))
        && characters.all(unicode_ident::is_xid_continue)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{LexicalRuleRole, plan_lexical};
    use crate::TextEquivalence;
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::resolution::{ResolutionInput, resolve_local_semantics};
    use crate::source_model::{SourceModelInput, lower_source_model};
    use crate::source_syntax::CargoCfgSnapshot;

    fn source_model(source: &str) -> crate::source_model::SourceModeledGrammar {
        let package = TempDir::new().unwrap();
        let root = package.path().join("grammar.rs");
        fs::write(&root, source).unwrap();
        source_model_from_package(&package, &CargoCfgSnapshot::default())
    }

    fn source_model_with_files(
        root_source: &str,
        files: &[(&str, &str)],
    ) -> crate::source_model::SourceModeledGrammar {
        let package = TempDir::new().unwrap();
        fs::write(package.path().join("grammar.rs"), root_source).unwrap();
        for (path, source) in files {
            fs::write(package.path().join(path), source).unwrap();
        }
        source_model_from_package(&package, &CargoCfgSnapshot::default())
    }

    fn source_model_from_package(
        package: &TempDir,
        cfg: &CargoCfgSnapshot,
    ) -> crate::source_model::SourceModeledGrammar {
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("grammar.rs"),
            edition: RustEdition::Rust2024,
            cfg,
        });
        assert!(
            discovery.diagnostics.is_empty(),
            "{:?}",
            discovery.diagnostics
        );
        let resolution = resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg,
        });
        assert!(
            resolution.diagnostics.is_empty(),
            "{:?}",
            resolution.diagnostics
        );
        let source = lower_source_model(SourceModelInput {
            resolved: resolution.resolved.unwrap(),
            cfg,
        });
        assert!(source.diagnostics.is_empty(), "{:?}", source.diagnostics);
        source.source_modeled.unwrap()
    }

    fn plan(source: &str) -> super::LexicalPlanReport {
        plan_lexical(&source_model(source))
    }

    fn content_base_identity(pattern: &str) -> super::TokenKindIdentity {
        super::TokenKindIdentity::ContentBase(super::ContentBaseIdentity {
            pattern: pattern.to_owned(),
            closed_matcher: None,
            priority: None,
        })
    }

    #[test]
    fn plans_sensitive_keywords_with_unicode_xid_classification() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("select", "éclair", "e\u{301}clair", "->")]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.expect("valid lexical plan");
        assert_eq!(plan.text_equivalence, TextEquivalence::Sensitive);
        assert_eq!(
            plan.rules
                .iter()
                .map(|rule| match &rule.matcher {
                    super::LexicalMatcher::Fixed(spelling) => (spelling.as_str(), rule.role),
                    super::LexicalMatcher::Regex(_) => unreachable!(),
                })
                .collect::<Vec<_>>(),
            [
                ("e\u{301}clair", LexicalRuleRole::StandaloneKeyword),
                ("select", LexicalRuleRole::StandaloneKeyword),
                ("éclair", LexicalRuleRole::StandaloneKeyword),
                ("->", LexicalRuleRole::StandaloneFixed),
            ],
        );
    }

    #[test]
    fn ascii_insensitive_keywords_have_canonical_spelling_and_duplicate_metadata() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("select", "SeLeCt", "é", "É", "e\u{301}")]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.unwrap();
        assert_eq!(plan.text_equivalence, TextEquivalence::AsciiInsensitive);
        assert_eq!(
            plan.keywords
                .iter()
                .map(|keyword| (
                    keyword.canonical_spelling.as_str(),
                    keyword.definition_sites.len(),
                ))
                .collect::<Vec<_>>(),
            [("E\u{301}", 1), ("SELECT", 2), ("É", 1), ("é", 1)],
        );
        assert!(plan.keywords.iter().all(|keyword| {
            keyword.definition_sites.windows(2).all(|sites| {
                sites[0].path < sites[1].path
                    || (sites[0].path == sites[1].path
                        && sites[0].range.start <= sites[1].range.start)
            })
        }));
    }

    #[test]
    fn ascii_insensitive_spelling_does_not_fold_symbolic_fixed_names() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    keywords {
        lower => r"a",
        LOWER => r"b",
    }
}

#[derive(recursa::Node)]
#[tok(lower, LOWER)]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report
            .plan
            .expect("case-sensitive symbolic names are distinct");
        assert_eq!(
            plan.token_kinds
                .iter()
                .map(|kind| &kind.identity)
                .collect::<Vec<_>>(),
            [
                &super::TokenKindIdentity::Fixed("A".to_owned()),
                &super::TokenKindIdentity::Fixed("B".to_owned()),
            ],
        );
    }

    #[test]
    fn projects_reclaimable_keywords_to_concrete_kinds_and_parser_target_sets() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root { name: Word }

#[derive(recursa::Node)]
struct Definition {
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(IF))]
    name: Word,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.unwrap();
        assert_eq!(
            plan.token_kinds
                .iter()
                .map(|kind| &kind.identity)
                .collect::<Vec<_>>(),
            [
                &super::TokenKindIdentity::Fixed("IF".to_owned()),
                &content_base_identity(r"[A-Za-z_][A-Za-z0-9_]*"),
            ],
        );
        let target = plan
            .parser_targets
            .iter()
            .find(|target| target.name == "Word")
            .expect("Word parser target");
        assert_eq!(
            plan.token_kinds[target.base_kind.index()].identity,
            content_base_identity(r"[A-Za-z_][A-Za-z0-9_]*"),
        );
        assert_eq!(
            target
                .accepted_kinds()
                .iter()
                .map(|kind| &plan.token_kinds[kind.index()].identity)
                .collect::<Vec<_>>(),
            [
                &super::TokenKindIdentity::Fixed("IF".to_owned()),
                &content_base_identity(r"[A-Za-z_][A-Za-z0-9_]*"),
            ],
        );
        assert_eq!(plan.text_equivalence, TextEquivalence::AsciiInsensitive);
    }

    #[test]
    fn derives_explicit_lexer_priorities_independently_of_declaration_order() {
        let first = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[a-z]+")]
    word: Word,
    #[lex(pattern = r"[0-9]+")]
    number: Number,
}
"#,
        )
        .plan
        .unwrap();
        let second = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[0-9]+")]
    number: Number,
    #[lex(pattern = r"[a-z]+")]
    word: Word,
}
"#,
        )
        .plan
        .unwrap();

        let projection = |plan: &super::LexicalPlan| {
            plan.rules
                .iter()
                .map(|rule| (rule.matcher.clone(), rule.priority))
                .collect::<Vec<_>>()
        };
        assert_eq!(projection(&first), projection(&second));
        assert_eq!(
            projection(&first),
            [
                (
                    super::LexicalMatcher::Regex("[0-9]+".to_owned()),
                    super::LexicalPriority::Pattern { authored: None },
                ),
                (
                    super::LexicalMatcher::Regex("[a-z]+".to_owned()),
                    super::LexicalPriority::Pattern { authored: None },
                ),
            ],
        );
    }

    #[test]
    fn keeps_longest_match_separate_from_equal_span_priority() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("!", "!=")]
struct Fixed;

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"!+")]
    operator: Operator,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.unwrap();
        assert_eq!(
            plan.selection_order,
            super::LexicalSelectionOrder::LongestPrefixThenFixedThenAuthoredPatternPriority,
        );
        let fixed_priorities = plan
            .rules
            .iter()
            .filter(|rule| rule.role == LexicalRuleRole::StandaloneFixed)
            .map(|rule| rule.priority)
            .collect::<Vec<_>>();
        assert_eq!(
            fixed_priorities,
            [super::LexicalPriority::Fixed, super::LexicalPriority::Fixed]
        );
        assert_eq!(
            plan.rules
                .iter()
                .find(|rule| rule.role == LexicalRuleRole::ContentPattern)
                .unwrap()
                .priority,
            super::LexicalPriority::Pattern { authored: None },
        );
    }

    #[test]
    fn eliminates_a_keyword_only_after_whole_record_combined_carrier_proof() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("A", "IF")]
struct Root {
    #[lex(pattern = "A")]
    upper: Upper,
    #[lex(pattern = "a")]
    lower: Lower,
    #[lex(pattern = "I")]
    prefix_only: Prefix,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.unwrap();
        let keyword = |spelling: &str| {
            plan.keywords
                .iter()
                .find(|keyword| keyword.canonical_spelling == spelling)
                .unwrap()
        };
        let super::KeywordRecognition::CombinedCarriers {
            carrier,
            additional_carriers,
        } = &keyword("A").recognition
        else {
            panic!("A/a must be jointly carried")
        };
        assert_eq!(
            std::iter::once(carrier)
                .chain(additional_carriers)
                .map(|kind| &plan.token_kinds[kind.index()].identity)
                .collect::<Vec<_>>(),
            [&content_base_identity("A"), &content_base_identity("a"),],
        );
        assert_eq!(
            keyword("IF").recognition,
            super::KeywordRecognition::Standalone
        );
        assert!(
            !plan
                .rules
                .iter()
                .any(|rule| { rule.matcher == super::LexicalMatcher::Fixed("A".to_owned()) })
        );
        assert!(
            plan.rules
                .iter()
                .any(|rule| { rule.matcher == super::LexicalMatcher::Fixed("IF".to_owned()) })
        );
    }

    #[test]
    fn central_ignore_matching_a_keyword_forces_silent_standalone_fallback() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    ignore = r"if",
}

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"IF")]
    word: Word,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.unwrap();
        assert!(matches!(
            plan.non_token_rules.as_slice(),
            [super::NonTokenLexicalRule {
                matcher: super::LexicalMatcher::Regex(pattern),
                outcome: super::NonTokenLexicalOutcome::NonEmitting,
                ..
            }] if pattern == "if"
        ));
        assert_eq!(
            plan.keywords[0].recognition,
            super::KeywordRecognition::Standalone
        );
        assert!(
            plan.rules
                .iter()
                .any(|rule| { rule.matcher == super::LexicalMatcher::Fixed("IF".to_owned()) })
        );
    }

    #[test]
    fn equal_priority_ignore_and_content_overlap_emits_typed_rca3005() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    ignore = r"[A-Za-z]+",
}

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-F]+")]
    word: Word,
}
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3005);
        assert_eq!(
            crate::source_analysis::lexical_code(diagnostic.code),
            "RCA3005"
        );
        assert_eq!(diagnostic.sites.len(), 2);
        assert!(matches!(
            &diagnostic.evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap {
                contenders,
                witness,
            } if contenders.len() == 2 && witness == "A"
        ));
    }

    #[test]
    fn combined_pattern_dfas_exclude_unused_unanchored_search_states() {
        let dfa = super::all_matches_dfa(&[r"[A-Za-z_][A-Za-z0-9_]*", r"'[^']*'"])
            .expect("valid lexical patterns compile");

        assert_eq!(dfa.start_kind(), regex_automata::dfa::StartKind::Anchored,);
    }

    const POSTGRESQL_PATTERN_FAMILIES: [&str; 14] = [
        r"[ \t\r\n\f]+",
        r"--",
        r"/\*",
        r"\$(?:[A-Za-z_][A-Za-z0-9_]*)?\$",
        r"(?:(?:[0-9](?:_?[0-9])*\.[0-9](?:_?[0-9])*|\.[0-9](?:_?[0-9])*)(?:[eE][+-]?[0-9](?:_?[0-9])*)?|[0-9](?:_?[0-9])*\.[eE][+-]?[0-9](?:_?[0-9])*|[0-9](?:_?[0-9])*[eE][+-]?[0-9](?:_?[0-9])*|[0-9](?:_?[0-9])*\.)",
        r"(?:0[xX](?:_?[0-9a-fA-F])+|0[oO](?:_?[0-7])+|0[bB](?:_?[01])+|[0-9](?:_?[0-9])*)",
        r"\$[0-9]+",
        r"[-+*/<>=~!@#%^&|?]+",
        r#"[Uu]&"[^"]*(?:""[^"]*)*"|"[^"]*(?:""[^"]*)*"|[A-Za-z_][A-Za-z0-9_]*"#,
        r"'[^']*(?:''[^']*)*'",
        r"(?i:U)&'(?:[^']|'')*'",
        r"(?i:E)'(?:[^'\\]|\\.|'')*'",
        r"(?i:B)'[^']*'",
        r"(?i:X)'[^']*'",
    ];

    #[test]
    fn postgresql_pattern_families_build_one_anchored_dfa() {
        let dfa = super::all_matches_dfa(&POSTGRESQL_PATTERN_FAMILIES)
            .expect("PostgreSQL pattern families compile");

        assert_eq!(dfa.start_kind(), regex_automata::dfa::StartKind::Anchored,);
    }

    #[test]
    #[ignore = "manual representative-scale PostgreSQL DFA memory check"]
    fn postgresql_pattern_families_stay_within_the_memory_budget() {
        let dfa = super::all_matches_dfa(&POSTGRESQL_PATTERN_FAMILIES)
            .expect("PostgreSQL pattern families compile");

        assert!(dfa.memory_usage() < 1024 * 1024, "{dfa:?}");
    }

    #[test]
    fn retained_fixed_rule_shields_a_fully_covered_pattern_overlap() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! { ignore = r"IF" }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"IF")]
    word: Word,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report
            .plan
            .expect("the retained fixed winner resolves the overlap");
        assert!(plan.rules.iter().any(|rule| {
            rule.matcher == super::LexicalMatcher::Fixed("IF".to_owned())
                && rule.role == super::LexicalRuleRole::StandaloneKeyword
        }));
    }

    #[test]
    fn fixed_rule_shields_only_its_exact_witness_of_a_broader_overlap() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! { ignore = r"I[FJ]" }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"I[FJ]")]
    word: Word,
}
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert!(matches!(
            &report.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap { witness, .. }
                if witness == "IJ"
        ));
    }

    #[test]
    fn fixed_shield_remains_when_distinct_content_kinds_tie_on_the_keyword() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"IF|A")]
    first: First,
    #[lex(pattern = r"IF|B")]
    second: Second,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report
            .plan
            .expect("the retained fixed rule resolves the only tie");
        assert_eq!(
            plan.keywords[0].recognition,
            super::KeywordRecognition::Standalone,
        );
        assert!(plan.rules.iter().any(|rule| {
            rule.matcher == super::LexicalMatcher::Fixed("IF".to_owned())
                && rule.role == super::LexicalRuleRole::StandaloneKeyword
        }));
    }

    #[test]
    fn ambiguity_search_continues_past_a_fixed_witness_in_the_same_dfa_state() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("A", this)]
struct Root {
    #[lex(pattern = r"[A-Z]+")]
    broad: Broad,
    #[lex(pattern = r"[A-F]+")]
    narrow: Narrow,
}
"#,
        );

        assert!(report.plan.is_none());
        assert!(matches!(
            &report.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap { witness, .. }
                if witness == "B"
        ));
    }

    #[test]
    fn fixed_prefix_does_not_shield_a_longer_pattern_overlap() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"IF[A-Z]*")]
    broad: Broad,
    #[lex(pattern = r"IF[A-Y]*")]
    narrow: Narrow,
}
"#,
        );

        assert!(report.plan.is_none());
        assert!(matches!(
            &report.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap { witness, .. }
                if witness == "IFA"
        ));
    }

    #[test]
    fn ascii_insensitive_fixed_coverage_folds_only_ascii_in_word_shaped_tokens() {
        let punctuation = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("a-b", this)]
struct Root {
    #[lex(pattern = r"A-B|X")]
    first: First,
    #[lex(pattern = r"A-B|Y")]
    second: Second,
}
"#,
        );
        let unicode = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("ÉA", this)]
struct Root {
    #[lex(pattern = r"Éa|éA|X")]
    first: First,
    #[lex(pattern = r"Éa|éA|Y")]
    second: Second,
}
"#,
        );

        assert!(matches!(
            &punctuation.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap { witness, .. }
                if witness == "A-B"
        ));
        assert!(matches!(
            &unicode.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap { witness, .. }
                if witness == "éA"
        ));
    }

    #[test]
    fn fixed_precedence_applies_to_authored_priority_ties_without_hiding_other_witnesses() {
        let fully_covered = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"IF|A", priority = 7)]
    first: First,
    #[lex(pattern = r"IF|B", priority = 7)]
    second: Second,
}
"#,
        );
        let partially_covered = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"I[FJ]|A", priority = 7)]
    first: First,
    #[lex(pattern = r"I[FJ]|B", priority = 7)]
    second: Second,
}
"#,
        );

        assert!(
            fully_covered.diagnostics.is_empty(),
            "{:?}",
            fully_covered.diagnostics
        );
        let plan = fully_covered
            .plan
            .expect("fixed precedence resolves the complete priority tie");
        assert_eq!(
            plan.keywords[0].recognition,
            super::KeywordRecognition::Standalone,
        );
        assert!(matches!(
            &partially_covered.diagnostics[0],
            super::LexicalDiagnostic {
                code: super::LexicalDiagnosticCode::Rca3006,
                evidence: super::LexicalDiagnosticEvidence::PriorityTie {
                    priority: 7,
                    witness,
                    ..
                },
                ..
            } if witness == "IJ"
        ));
    }

    #[test]
    fn ignore_overlap_is_deterministic_under_internal_record_permutation() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! { ignore = r"[A-Z]+" }
recursa::tokens! { ignore = r"[A-F]+" }

#[derive(recursa::Node)]
struct Root;
"#,
        );
        let expected = plan_lexical(&source);
        let mut permuted = source.clone();
        permuted.source_model_mut().lexer_records.reverse();
        let actual = plan_lexical(&permuted);

        assert_eq!(actual, expected);
        assert!(actual.plan.is_none());
        assert_eq!(actual.diagnostics.len(), 1);
        let diagnostic = &actual.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3005);
        assert_eq!(diagnostic.sites.len(), 2);
        assert!(
            diagnostic.sites.windows(2).all(
                |sites| source.source_order_key(&sites[0]) < source.source_order_key(&sites[1])
            )
        );
        assert!(matches!(
            &diagnostic.evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap {
                contenders,
                witness,
            } if witness == "A"
                && contenders.len() == 2
                && contenders.iter().all(|contender| {
                    contender.outcome == super::LexicalPatternOutcome::NonEmitting
                        && contender.target_names.is_empty()
                })
        ));
        assert!(matches!(
            diagnostic.remedies.as_slice(),
            [super::LexicalRemedy::MakePatternsDisjoint { contenders }]
                if contenders.len() == 2
        ));
    }

    #[test]
    fn equivalent_ignore_records_are_one_semantic_overlap_contender() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! { ignore = r"[A-Z]+" }
recursa::tokens! { ignore = r"[A-Z]+" }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-F]+")]
    word: Word,
}
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3005);
        assert_eq!(diagnostic.sites.len(), 3);
        assert!(matches!(
            &diagnostic.evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap {
                contenders,
                witness,
            } if contenders.len() == 2 && witness == "A"
        ));
    }

    #[test]
    fn equivalent_ignore_records_do_not_overlap_each_other() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! { ignore = r"[A-Z]+" }
recursa::tokens! { ignore = r"[A-Z]+" }

#[derive(recursa::Node)]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty());
        assert!(report.plan.is_some());
    }

    #[test]
    fn equivalent_ignore_declarations_project_as_one_semantic_rule() {
        let one = plan(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! { ignore = r"[ ]+" }
#[derive(recursa::Node)]
struct Root;
"#,
        )
        .plan
        .unwrap();
        let two = plan(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! { ignore = r"[ ]+" }
recursa::tokens! { ignore = r"[ ]+" }
#[derive(recursa::Node)]
struct Root;
"#,
        )
        .plan
        .unwrap();

        assert_eq!(two.non_token_rules.len(), 1);
        assert_eq!(two.non_token_rules[0].sites.as_slice().len(), 2);
        assert_eq!(
            one.canonical_lexical_projection(),
            two.canonical_lexical_projection(),
        );
    }

    #[test]
    fn repeated_invalid_ignore_is_one_rca3004_with_module_aware_sites() {
        let source = source_model_with_files(
            r#"
recursa::grammar! { module = crate }

#[path = "shared.rs"]
mod first;
#[path = "shared.rs"]
mod second;

#[derive(recursa::Node)]
struct Root;
"#,
            &[("shared.rs", r#"recursa::tokens! { ignore = r"" }"#)],
        );
        let report = plan_lexical(&source);

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3004);
        assert_eq!(
            crate::source_analysis::lexical_code(diagnostic.code),
            "RCA3004"
        );
        assert_eq!(diagnostic.sites.len(), 2);
        assert_ne!(diagnostic.sites[0].module, diagnostic.sites[1].module);
        assert!(
            source.source_order_key(&diagnostic.sites[0])
                < source.source_order_key(&diagnostic.sites[1])
        );
    }

    #[test]
    fn central_only_overlap_offers_only_an_authored_actionable_remedy() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    literals {
        Broad => r"[A-Z]+",
        Narrow => r"[A-F]+",
    }
}

#[derive(recursa::Node)]
struct Root;
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3005);
        assert!(matches!(
            diagnostic.remedies.as_slice(),
            [super::LexicalRemedy::MakePatternsDisjoint { contenders }]
                if contenders.len() == 2
        ));
    }

    #[test]
    fn lower_priority_non_emitting_record_does_not_block_a_proven_emitting_carrier() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    ignore = r"if",
}

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"[A-Za-z]+", priority = 1)]
    word: Word,
}
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(matches!(
            report.plan.unwrap().keywords[0].recognition,
            super::KeywordRecognition::CombinedCarriers { .. }
        ));
    }

    #[test]
    fn invalid_central_non_emitting_record_is_not_silently_discarded() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    ignore = r"",
}

#[derive(recursa::Node)]
struct Root;
"#,
        );

        assert!(report.plan.is_none());
        assert!(matches!(
            report.diagnostics.as_slice(),
            [super::LexicalDiagnostic {
                code: super::LexicalDiagnosticCode::Rca3004,
                evidence: super::LexicalDiagnosticEvidence::InvalidPattern {
                    reason: super::UnsupportedPatternReason::EmptyMatch,
                    ..
                },
                ..
            }]
        ));
    }

    #[test]
    fn carrier_cache_compiles_once_and_proves_each_canonical_keyword_once() {
        let source = source_model(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("if", "IF", "else", this)]
struct Root {
    #[lex(pattern = r"[A-Za-z]+")]
    word: Word,
}
"#,
        );
        let groups = super::content_base_groups(source.source_model());
        let fixed = super::fixed_matchers(
            source.source_model(),
            crate::TextEquivalence::AsciiInsensitive,
        );
        let cache = super::CarrierProofCache::new(
            crate::TextEquivalence::AsciiInsensitive,
            &groups,
            &source.source_model().lexer_records,
            &fixed,
        );

        assert_eq!(cache.matcher_constructions, 1);
        assert_eq!(cache.proof_evaluations, 2);
        assert_eq!(cache.proofs.len(), 2);

        let invalid = source_model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("IF", "ELSE", this)]
struct Root {
    #[lex(pattern = r"(")]
    invalid: Invalid,
}
"#,
        );
        let invalid_groups = super::content_base_groups(invalid.source_model());
        let invalid_fixed =
            super::fixed_matchers(invalid.source_model(), crate::TextEquivalence::Sensitive);
        let invalid_cache = super::CarrierProofCache::new(
            crate::TextEquivalence::Sensitive,
            &invalid_groups,
            &invalid.source_model().lexer_records,
            &invalid_fixed,
        );

        assert_eq!(invalid_cache.matcher_constructions, 1);
        assert_eq!(invalid_cache.proof_evaluations, 2);
        assert!(invalid_cache.proofs.values().all(Option::is_none));
    }

    #[test]
    fn reports_unsupported_overlap_and_tie_diagnostics_with_sites_and_remedies() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "(")]
    invalid: Invalid,
    #[lex(pattern = r"[A-Z]+")]
    broad: Broad,
    #[lex(pattern = r"[A-F]+")]
    narrow: Narrow,
    #[lex(pattern = r"[0-9]+", priority = 7)]
    first_digit: FirstDigit,
    #[lex(pattern = r"[0-9][0-9]*", priority = 7)]
    second_digit: SecondDigit,
}
"#,
        );
        let report = plan_lexical(&source);

        assert!(report.plan.is_none());
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| crate::source_analysis::lexical_code(diagnostic.code))
                .collect::<Vec<_>>(),
            ["RCA3004", "RCA3005", "RCA3006"],
        );
        assert!(
            report.diagnostics.iter().all(|diagnostic| {
                !diagnostic.sites.is_empty() && !diagnostic.remedies.is_empty()
            })
        );
        assert!(report.diagnostics.iter().all(|diagnostic| {
            diagnostic.sites.windows(2).all(|sites| {
                sites[0].path < sites[1].path
                    || (sites[0].path == sites[1].path
                        && sites[0].range.start <= sites[1].range.start)
            })
        }));
        let overlap = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == super::LexicalDiagnosticCode::Rca3005)
            .unwrap();
        assert!(matches!(
            &overlap.evidence,
            super::LexicalDiagnosticEvidence::PatternOverlap { witness, .. } if witness == "A"
        ));
    }

    #[test]
    fn rejects_unicode_word_boundaries_before_emission() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"\bword\b")]
    word: Word,
}
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert!(matches!(
            report.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::InvalidPattern {
                reason: super::UnsupportedPatternReason::WordBoundary,
                ..
            }
        ));
    }

    #[test]
    fn rejects_anchors_and_ascii_word_boundaries_as_non_logos_patterns() {
        let anchors = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"^word$")]
    word: Word,
}
"#,
        );
        let ascii_boundary = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"(?-u:\b)word")]
    word: Word,
}
"#,
        );

        assert!(matches!(
            anchors.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::InvalidPattern {
                reason: super::UnsupportedPatternReason::Anchor,
                ..
            }
        ));
        assert!(matches!(
            ascii_boundary.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::InvalidPattern {
                reason: super::UnsupportedPatternReason::WordBoundary,
                ..
            }
        ));
    }

    #[test]
    fn tied_priority_reports_only_the_highest_effective_contenders() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Z]+")]
    broad: Broad,
    #[lex(pattern = r"[A-F]+", priority = 2)]
    medium: Medium,
    #[lex(pattern = r"[A-C]+", priority = 2)]
    narrow: Narrow,
}
"#,
        );

        let report = plan_lexical(&source);

        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3006);
        assert_eq!(
            crate::source_analysis::lexical_code(diagnostic.code),
            "RCA3006"
        );
        let super::LexicalDiagnosticEvidence::PriorityTie { contenders, .. } = &diagnostic.evidence
        else {
            panic!("expected a typed priority tie")
        };
        assert_eq!(
            contenders
                .iter()
                .flat_map(|contender| contender.target_names.iter().map(String::as_str))
                .collect::<Vec<_>>(),
            ["Narrow", "Medium"],
        );
    }

    #[test]
    fn lexical_diagnostic_registry_reserves_rca3001_through_rca3006() {
        assert_eq!(
            super::LexicalDiagnosticCode::ALL.map(super::LexicalDiagnosticCode::as_str),
            [
                "RCA3001", "RCA3002", "RCA3003", "RCA3004", "RCA3005", "RCA3006",
            ],
        );
        assert_eq!(
            super::LexicalDiagnosticCode::ALL.map(|code| code.spec().condition),
            [
                super::LexicalDiagnosticCondition::InvalidKeywordMatchingPolicy,
                super::LexicalDiagnosticCondition::FixedTokenClassificationMismatch,
                super::LexicalDiagnosticCondition::ConflictingTokenDefinition,
                super::LexicalDiagnosticCondition::UnsupportedLexicalPattern,
                super::LexicalDiagnosticCondition::UnresolvedLexicalOverlap,
                super::LexicalDiagnosticCondition::TiedLexicalPriority,
            ],
        );
        assert!(super::LexicalDiagnosticCode::ALL.iter().all(|code| {
            let spec = code.spec();
            spec.code == *code
        }));
    }

    #[test]
    fn lexical_reports_ignore_internal_vector_order_but_keep_selected_source_sites() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"[A-Z]+")]
    broad: Broad,
    #[lex(pattern = r"[A-F]+")]
    narrow: Narrow,
}
"#,
        );
        let expected = plan_lexical(&source);
        let mut permuted = source.clone();
        permuted.source_model_mut().token_definitions.reverse();

        let actual = plan_lexical(&permuted);

        assert_eq!(actual, expected);
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(
            actual.diagnostics[0]
                .sites
                .iter()
                .map(|site| site.range.start)
                .collect::<Vec<_>>(),
            expected.diagnostics[0]
                .sites
                .iter()
                .map(|site| site.range.start)
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn diagnostic_sites_follow_selected_source_order_instead_of_path_sorting() {
        let source = source_model_with_files(
            r#"
recursa::grammar! { module = crate }
mod z;
mod a;
"#,
            &[
                (
                    "z.rs",
                    r#"
#[derive(recursa::Node)]
pub struct Z {
    #[lex(pattern = r"[A-Z]+")]
    value: Broad,
}
"#,
                ),
                (
                    "a.rs",
                    r#"
#[derive(recursa::Node)]
pub struct A {
    #[lex(pattern = r"[A-F]+")]
    value: Narrow,
}
"#,
                ),
            ],
        );

        let report = plan_lexical(&source);

        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0]
                .sites
                .iter()
                .map(|site| site.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["z.rs", "a.rs"],
        );
    }

    #[test]
    fn source_sites_distinguish_repeated_logical_module_inclusions() {
        let source = source_model_with_files(
            r#"
recursa::grammar! { module = crate }

#[path = "shared.rs"]
mod first;
#[path = "shared.rs"]
mod second;
"#,
            &[(
                "shared.rs",
                r#"
#[derive(recursa::Node)]
#[tok("IF")]
pub struct Shared;
"#,
            )],
        );
        let sites = &source.source_model().token_definitions[0].definition_sites;

        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].path, sites[1].path);
        assert_eq!(sites[0].range, sites[1].range);
        assert_ne!(sites[0].module, sites[1].module);
        assert!(source.source_order_key(&sites[0]) < source.source_order_key(&sites[1]));
    }

    #[test]
    fn contextual_keyword_projection_is_stable_under_source_permutations() {
        let first = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("SELECT", "FROM", "AS", ",")]
struct Fixed;

#[derive(recursa::Node)]
struct Definition {
    #[lex(pattern = r"[A-Za-z_]+")]
    word: Word,
    #[lex(pattern = r"[A-Za-z_]+", admits(AS))]
    column: ColumnName,
    #[lex(pattern = r"[A-Za-z_]+", admits(AS))]
    table: TableName,
    #[lex(pattern = r"[A-Za-z_]+")]
    alias: Alias,
}
"#,
        )
        .plan
        .unwrap();
        let second = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
struct Definition {
    #[lex(pattern = r"[A-Za-z_]+")]
    alias: Alias,
    #[lex(pattern = r"[A-Za-z_]+", admits(AS))]
    table: TableName,
    #[lex(pattern = r"[A-Za-z_]+", admits(AS))]
    column: ColumnName,
    #[lex(pattern = r"[A-Za-z_]+")]
    word: Word,
}

#[derive(recursa::Node)]
#[tok(",", "AS", "FROM", "SELECT")]
struct Fixed;
"#,
        )
        .plan
        .unwrap();

        assert_eq!(
            first.canonical_lexical_projection(),
            second.canonical_lexical_projection(),
        );
        let base_label =
            super::kind_label(&first.token_kinds[first.content_bases[0].kind.index()].identity);
        assert!(first.canonical_lexical_projection().contains(&format!(
            "target ColumnName base={base_label} accepts=[{base_label},fixed:AS]"
        )));
        assert!(first.canonical_lexical_projection().contains(&format!(
            "target TableName base={base_label} accepts=[{base_label},fixed:AS]"
        )));
        assert!(first.canonical_lexical_projection().contains(&format!(
            "target Alias base={base_label} accepts=[{base_label}]"
        )));
        assert_eq!(first.content_bases.len(), 1);
        assert_eq!(
            first.content_bases[0].target_names.as_slice(),
            ["Alias", "ColumnName", "TableName", "Word"]
        );
        assert_eq!(
            first
                .token_kinds
                .iter()
                .map(|kind| &kind.identity)
                .collect::<Vec<_>>(),
            [
                &super::TokenKindIdentity::Fixed("SELECT".to_owned()),
                &super::TokenKindIdentity::Fixed("FROM".to_owned()),
                &super::TokenKindIdentity::Fixed("AS".to_owned()),
                &super::TokenKindIdentity::Fixed(",".to_owned()),
                &content_base_identity(r"[A-Za-z_]+"),
            ],
        );
    }

    #[test]
    fn named_admission_membership_and_emission_are_stable_under_declaration_permutations() {
        let first = plan(
            r#"
recursa::grammar! { module = crate, keyword_matching = ascii_insensitive }
recursa::tokens! {
    categories { UNRESERVED, RESERVED }
    keywords {
        WINDOW => "window" in UNRESERVED,
        OTHER => "other" in UNRESERVED,
        ROWS => "rows" in RESERVED,
    }
    admissions {
        UnreservedNames = UNRESERVED,
        ColId = UnreservedNames + RESERVED,
        WindowRefName = ColId - { ROWS },
    }
}
#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_]+", admits(WindowRefName))]
    name: WindowName,
}
"#,
        )
        .plan
        .unwrap();
        let second = plan(
            r#"
recursa::grammar! { module = crate, keyword_matching = ascii_insensitive }
recursa::tokens! {
    admissions {
        WindowRefName = ColId - { ROWS },
        ColId = UnreservedNames + RESERVED,
        UnreservedNames = UNRESERVED,
    }
    keywords {
        ROWS => "rows" in RESERVED,
        OTHER => "other" in UNRESERVED,
        WINDOW => "window" in UNRESERVED,
    }
    categories { RESERVED, UNRESERVED }
}
#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_]+", admits(WindowRefName))]
    name: WindowName,
}
"#,
        )
        .plan
        .unwrap();

        assert_eq!(
            first.canonical_lexical_projection(),
            second.canonical_lexical_projection(),
        );
        let base = super::kind_label(
            &first.token_kinds[first.parser_targets[0].base_kind.index()].identity,
        );
        assert!(first.canonical_lexical_projection().contains(&format!(
            "target WindowName base={base} accepts=[{base},fixed:OTHER,fixed:WINDOW]"
        )));
    }

    #[test]
    fn lexical_rule_projection_ignores_source_ordered_kind_ids() {
        let first = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"x", priority = 1)]
    first: First,
    #[lex(pattern = r"x", priority = 2)]
    second: Second,
}
"#,
        )
        .plan
        .unwrap();
        let second = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"x", priority = 2)]
    second: Second,
    #[lex(pattern = r"x", priority = 1)]
    first: First,
}
"#,
        )
        .plan
        .unwrap();

        assert_eq!(
            first.canonical_lexical_projection(),
            second.canonical_lexical_projection(),
        );
    }

    #[test]
    fn content_base_projection_ignores_source_ordered_kind_ids() {
        let first = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    literals {
        A => r"a",
        B => r"b",
    }
}

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"w", priority = 1)]
    winner: Winner,
}
"#,
        )
        .plan
        .unwrap();
        let second = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    literals {
        B => r"b",
        A => r"a",
    }
}

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"w", priority = 1)]
    winner: Winner,
}
"#,
        )
        .plan
        .unwrap();

        assert_eq!(
            first.canonical_lexical_projection(),
            second.canonical_lexical_projection(),
        );
    }

    #[test]
    fn content_base_identity_does_not_depend_on_parser_target_names() {
        let first = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_]+")]
    value: LongParserTargetName,
}
"#,
        )
        .plan
        .unwrap();
        let second = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_]+")]
    value: X,
}
"#,
        )
        .plan
        .unwrap();

        let first_identity = &first.token_kinds[first.content_bases[0].kind.index()].identity;
        let second_identity = &second.token_kinds[second.content_bases[0].kind.index()].identity;
        assert!(matches!(
            first_identity,
            super::TokenKindIdentity::ContentBase(_)
        ));
        assert_eq!(first_identity, second_identity);
    }

    #[test]
    fn lexical_rules_preserve_authored_priority_identity() {
        let plan = plan(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_]+", priority = 17)]
    value: Word,
}
"#,
        )
        .plan
        .unwrap();
        let rule = plan
            .rules
            .iter()
            .find(|rule| rule.role == super::LexicalRuleRole::ContentPattern)
            .unwrap();

        assert_eq!(
            rule.priority,
            super::LexicalPriority::Pattern { authored: Some(17) }
        );
    }

    #[test]
    fn invalid_root_keyword_policy_emits_typed_rca3001() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = unicode_insensitive,
}

#[derive(recursa::Node)]
struct Root;
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            super::LexicalDiagnosticCode::Rca3001
        );
        assert_eq!(
            crate::source_analysis::lexical_code(report.diagnostics[0].code),
            "RCA3001"
        );
        assert!(matches!(
            &report.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::InvalidKeywordMatchingPolicy { authored }
                if authored == "unicode_insensitive"
        ));
        assert_eq!(
            report.diagnostics[0].remedies,
            [super::LexicalRemedy::ChooseSupportedKeywordMatchingPolicy]
        );
    }

    #[test]
    fn central_fixed_section_mismatch_emits_typed_rca3002() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    punctuation {
        SELECT => r"SELECT",
    }
}

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Root;
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3002);
        assert_eq!(
            crate::source_analysis::lexical_code(diagnostic.code),
            "RCA3002"
        );
        assert!(matches!(
            diagnostic.evidence,
            super::LexicalDiagnosticEvidence::FixedTokenClassificationMismatch {
                authored: super::FixedTokenClass::Punctuation,
                mechanical: super::FixedTokenClass::Keyword,
                ..
            }
        ));
        assert_eq!(
            diagnostic.remedies,
            [super::LexicalRemedy::UseMechanicalFixedTokenClass {
                class: super::FixedTokenClass::Keyword,
            }]
        );
    }

    #[test]
    fn conflicting_content_metadata_emits_typed_rca3003_at_all_sites() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "a")]
    first: Word,
    #[lex(pattern = "b", priority = 3)]
    second: Word,
    #[lex(pattern = "a")]
    third: Word,
}
"#,
        );
        let report = plan_lexical(&source);
        let mut permuted = source;
        permuted.source_model_mut().token_definitions.reverse();

        assert!(report.plan.is_none());
        assert_eq!(plan_lexical(&permuted), report);
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, super::LexicalDiagnosticCode::Rca3003);
        assert_eq!(
            crate::source_analysis::lexical_code(diagnostic.code),
            "RCA3003"
        );
        assert_eq!(diagnostic.sites.len(), 3);
        assert!(matches!(
            &diagnostic.evidence,
            super::LexicalDiagnosticEvidence::ConflictingTokenDefinition {
                identity,
                normalized_metadata,
            } if identity == "Word" && normalized_metadata.len() == 2
        ));
        assert_eq!(
            diagnostic.remedies,
            [super::LexicalRemedy::MakeDefinitionMetadataEquivalent {
                identity: "Word".to_owned()
            }]
        );
    }

    #[test]
    fn conflicting_canonical_fixed_identity_emits_rca3003_at_all_sites() {
        let report = plan(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    keywords {
        SELECT => r"SELECT",
        SELECT => r"SEL",
    }
}

#[derive(recursa::Node)]
struct Root;
"#,
        );

        assert!(report.plan.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            super::LexicalDiagnosticCode::Rca3003
        );
        assert_eq!(report.diagnostics[0].sites.len(), 2);
        assert!(matches!(
            &report.diagnostics[0].evidence,
            super::LexicalDiagnosticEvidence::ConflictingTokenDefinition {
                identity,
                normalized_metadata,
            } if identity == "SELECT" && normalized_metadata.len() == 2
        ));
    }
}
