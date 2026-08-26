#![allow(
    dead_code,
    reason = "private M3 lexical-planning seam is consumed by later pipeline tickets"
)]

use crate::{KindId, KindSet, TextEquivalence};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

use regex_automata::dfa::{Automaton as _, dense};
use regex_automata::{Anchored, Input, MatchKind};

use crate::source_model::{
    AuthoredFixedTokenClass, SourceModel, SourceModeledGrammar, SourceSite, TokenDefinitionKind,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalPlanReport {
    pub(crate) diagnostics: Vec<LexicalDiagnostic>,
    pub(crate) plan: Option<LexicalPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalPlan {
    pub(crate) text_equivalence: TextEquivalence,
    pub(crate) selection_order: LexicalSelectionOrder,
    pub(crate) token_kinds: Vec<TokenKindPlan>,
    pub(crate) content_bases: Vec<ContentBasePlan>,
    pub(crate) keywords: Vec<KeywordPlan>,
    pub(crate) parser_targets: Vec<ParserTargetPlan>,
    pub(crate) rules: Vec<LexicalRule>,
    pub(crate) non_token_rules: Vec<NonTokenLexicalRule>,
    pub(crate) lowering_map: LexicalLoweringMap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LexicalSelectionOrder {
    LongestPrefixThenFixedThenAuthoredPatternPriority,
}

impl LexicalPlan {
    pub(crate) fn canonical_lexical_projection(&self) -> String {
        let mut rendered = String::new();
        writeln!(rendered, "equivalence {:?}", self.text_equivalence)
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
                left.target_names.as_slice(),
                left_kind,
                left.authored_priority,
            )
                .cmp(&(
                    &right.pattern,
                    right.target_names.as_slice(),
                    right_kind,
                    right.authored_priority,
                ))
        });
        for (base, kind) in bases {
            writeln!(
                rendered,
                "base {} pattern={:?} targets=[{}] priority={}",
                kind,
                base.pattern,
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
                left_kind,
                left.role,
                lexical_priority_projection_key(left.priority),
            )
                .cmp(&(
                    &right.matcher,
                    right_kind,
                    right.role,
                    lexical_priority_projection_key(right.priority),
                ))
        });
        for (rule, kind) in rules {
            writeln!(
                rendered,
                "rule {:?} kind={} priority={:?}",
                rule.matcher, kind, rule.priority,
            )
            .expect("writing to String cannot fail");
        }
        let mut non_token_rules = self.non_token_rules.iter().collect::<Vec<_>>();
        non_token_rules.sort_by(|left, right| {
            (&left.matcher, left.outcome).cmp(&(&right.matcher, right.outcome))
        });
        for rule in non_token_rules {
            writeln!(
                rendered,
                "non-token {:?} outcome={:?}",
                rule.matcher, rule.outcome
            )
            .expect("writing to String cannot fail");
        }
        rendered
    }
}

const fn lexical_priority_projection_key(priority: LexicalPriority) -> (u8, Option<u32>) {
    match priority {
        LexicalPriority::Fixed => (0, None),
        LexicalPriority::Pattern { authored } => (1, authored),
    }
}

pub(crate) fn kind_label(identity: &TokenKindIdentity) -> String {
    match identity {
        TokenKindIdentity::Fixed(spelling) => format!("fixed:{spelling}"),
        TokenKindIdentity::ContentBase(base) => {
            let callback = base
                .callback
                .as_ref()
                .map_or_else(|| "-".to_owned(), crate::source_syntax::path_text);
            format!(
                "content:{:?}:callback={callback}:priority={:?}",
                base.pattern, base.priority,
            )
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TokenKindPlan {
    pub(crate) id: KindId,
    pub(crate) identity: TokenKindIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TokenKindIdentity {
    Fixed(String),
    ContentBase(ContentBaseIdentity),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContentBaseIdentity {
    pub(crate) pattern: String,
    pub(crate) callback: Option<syn::Path>,
    pub(crate) priority: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContentBasePlan {
    pub(crate) kind: KindId,
    pub(crate) target_names: Vec<String>,
    pub(crate) pattern: String,
    pub(crate) callback: Option<syn::Path>,
    pub(crate) authored_priority: Option<u32>,
}

struct ContentBaseGroup<'source> {
    identity: ContentBaseIdentity,
    definitions: NonEmpty<&'source crate::source_model::TokenDefinition>,
}

struct PatternContender {
    pattern: String,
    target_names: Vec<String>,
    authored_priority: Option<u32>,
    priority_authorable: bool,
    outcome: LexicalPatternOutcome,
    sites: Vec<SourceSite>,
}

struct NonTokenRuleGroup {
    pattern: String,
    outcome: crate::source_model::NonTokenLexerOutcome,
    sites: NonEmpty<SourceSite>,
}

struct RetainedFixedMatcher {
    spelling: String,
    ascii_insensitive: bool,
}

struct FixedMatcher {
    spelling: String,
    word_shaped: bool,
    ascii_insensitive: bool,
}

struct SymbolicCarrierProof {
    carrier: ContentBaseIdentity,
    additional_carriers: Vec<ContentBaseIdentity>,
}

impl SymbolicCarrierProof {
    fn identities(&self) -> impl Iterator<Item = &ContentBaseIdentity> {
        std::iter::once(&self.carrier).chain(&self.additional_carriers)
    }
}

struct CarrierProofCache {
    proofs: BTreeMap<String, Option<SymbolicCarrierProof>>,
    matcher_constructions: usize,
    proof_evaluations: usize,
}

enum CarrierCandidate {
    Emitting {
        group: usize,
        opaque: bool,
        priority: u32,
    },
    NonEmitting {
        priority: u32,
    },
}

impl CarrierCandidate {
    const fn priority(&self) -> u32 {
        match self {
            Self::Emitting { priority, .. } | Self::NonEmitting { priority } => *priority,
        }
    }
}

struct CarrierProver {
    dfa: Option<dense::DFA<Vec<u32>>>,
    candidates: Vec<CarrierCandidate>,
    matcher_constructions: usize,
}

impl ContentBaseGroup<'_> {
    fn sites(&self) -> Vec<SourceSite> {
        self.definitions
            .iter()
            .flat_map(|definition| definition.definition_sites.iter().cloned())
            .collect()
    }

    fn priority_authorable(&self) -> bool {
        self.definitions
            .iter()
            .all(|definition| matches!(definition.kind, TokenDefinitionKind::Content(_)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeywordPlan {
    pub(crate) canonical_spelling: String,
    pub(crate) kind: KindId,
    pub(crate) recognition: KeywordRecognition,
    pub(crate) definition_sites: Vec<SourceSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum KeywordRecognition {
    Standalone,
    CombinedCarriers {
        carrier: KindId,
        additional_carriers: Vec<KindId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParserTargetPlan {
    pub(crate) definition: crate::lowering::TokenDefinitionId,
    pub(crate) semantic: crate::resolution::ContentTokenId,
    pub(crate) name: String,
    pub(crate) base_kind: KindId,
    pub(crate) admitted_kinds: KindSet,
}

impl ParserTargetPlan {
    pub(crate) fn accepted_kinds(&self) -> KindSet {
        let mut accepted = self.admitted_kinds.clone();
        accepted
            .insert(self.base_kind)
            .expect("a parser target base kind uses the grammar's exact width");
        accepted
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalRule {
    pub(crate) matcher: LexicalMatcher,
    pub(crate) kind: KindId,
    pub(crate) role: LexicalRuleRole,
    pub(crate) priority: LexicalPriority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NonTokenLexicalRule {
    pub(crate) matcher: LexicalMatcher,
    pub(crate) outcome: NonTokenLexicalOutcome,
    pub(crate) sites: NonEmpty<SourceSite>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum NonTokenLexicalOutcome {
    NonEmitting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalLoweringMap {
    pub(crate) token_definitions: Vec<TokenDefinitionKindCorrelation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TokenDefinitionKindCorrelation {
    pub(crate) source: crate::lowering::TokenDefinitionId,
    pub(crate) lowered: KindId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    fn new(first: T) -> Self {
        Self(vec![first])
    }

    fn from_vec(values: Vec<T>) -> Option<Self> {
        if values.is_empty() {
            None
        } else {
            Some(Self(values))
        }
    }

    fn push(&mut self, value: T) {
        self.0.push(value);
    }

    fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }

    fn as_slice(&self) -> &[T] {
        &self.0
    }

    fn sort_by(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.0.sort_by(compare);
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LexicalMatcher {
    Fixed(String),
    Regex(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LexicalPriority {
    Fixed,
    Pattern { authored: Option<u32> },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LexicalRuleRole {
    StandaloneKeyword,
    StandaloneFixed,
    ContentPattern,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalDiagnostic {
    pub(crate) code: LexicalDiagnosticCode,
    pub(crate) sites: Vec<SourceSite>,
    pub(crate) evidence: LexicalDiagnosticEvidence,
    pub(crate) remedies: Vec<LexicalRemedy>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LexicalDiagnosticCode {
    Rca3001,
    Rca3002,
    Rca3003,
    Rca3004,
    Rca3005,
    Rca3006,
}

impl LexicalDiagnosticCode {
    pub(crate) const ALL: [Self; 6] = [
        Self::Rca3001,
        Self::Rca3002,
        Self::Rca3003,
        Self::Rca3004,
        Self::Rca3005,
        Self::Rca3006,
    ];

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
pub(crate) struct LexicalDiagnosticSpec {
    pub(crate) code: LexicalDiagnosticCode,
    pub(crate) condition: LexicalDiagnosticCondition,
    pub(crate) evidence: LexicalEvidenceKind,
    pub(crate) remedy: LexicalRemedyKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LexicalDiagnosticCondition {
    InvalidKeywordMatchingPolicy,
    FixedTokenClassificationMismatch,
    ConflictingTokenDefinition,
    UnsupportedLexicalPattern,
    UnresolvedLexicalOverlap,
    TiedLexicalPriority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LexicalEvidenceKind {
    KeywordMatchingPolicy,
    FixedTokenClassification,
    ConflictingDefinitionMetadata,
    InvalidPattern,
    PatternOverlap,
    PriorityTie,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LexicalRemedyKind {
    ChooseSupportedKeywordMatchingPolicy,
    UseMechanicalFixedTokenClass,
    MakeDefinitionMetadataEquivalent,
    UseSupportedNonEmptyPattern,
    AddPriorityOrSeparatePatterns,
    AssignDistinctPriority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FixedTokenClass {
    Keyword,
    Punctuation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LexicalDiagnosticEvidence {
    InvalidKeywordMatchingPolicy {
        authored: String,
    },
    FixedTokenClassificationMismatch {
        spelling: String,
        authored: FixedTokenClass,
        mechanical: FixedTokenClass,
    },
    ConflictingTokenDefinition {
        identity: String,
        normalized_metadata: Vec<LexicalDefinitionMetadata>,
    },
    InvalidPattern {
        pattern: String,
        reason: UnsupportedPatternReason,
    },
    PatternOverlap {
        contenders: Vec<LexicalPatternEvidence>,
        witness: String,
    },
    PriorityTie {
        contenders: Vec<LexicalPatternEvidence>,
        priority: u32,
        witness: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnsupportedPatternReason {
    InvalidOrUnsupportedSyntax,
    WordBoundary,
    Anchor,
    EmptyMatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LexicalDefinitionMetadata {
    Fixed {
        class: FixedTokenClass,
        canonical_spelling: String,
    },
    Content {
        pattern: String,
        callback: Option<syn::Path>,
        authored_priority: Option<u32>,
        admitted_fixed_tokens: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalPatternEvidence {
    pub(crate) pattern: String,
    pub(crate) target_names: Vec<String>,
    pub(crate) authored_priority: Option<u32>,
    pub(crate) outcome: LexicalPatternOutcome,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LexicalPatternOutcome {
    EmitsTokenKind,
    NonEmitting,
    OpaqueCallback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LexicalRemedy {
    ChooseSupportedKeywordMatchingPolicy,
    UseMechanicalFixedTokenClass {
        class: FixedTokenClass,
    },
    MakeDefinitionMetadataEquivalent {
        identity: String,
    },
    UseSupportedNonEmptyPattern,
    AssignDistinctPriority {
        contenders: Vec<LexicalPatternEvidence>,
    },
    MakePatternsDisjoint {
        contenders: Vec<LexicalPatternEvidence>,
    },
}

fn non_token_rule_groups(source: &SourceModeledGrammar) -> Vec<NonTokenRuleGroup> {
    let mut groups = Vec::<NonTokenRuleGroup>::new();
    for record in &source.source_model().lexer_records {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.pattern == record.pattern && group.outcome == record.outcome)
        {
            group.sites.push(record.site.clone());
        } else {
            groups.push(NonTokenRuleGroup {
                pattern: record.pattern.clone(),
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
        (&left.pattern, non_token_source_outcome_key(left.outcome))
            .cmp(&(&right.pattern, non_token_source_outcome_key(right.outcome)))
    });
    groups
}

const fn non_token_source_outcome_key(outcome: crate::source_model::NonTokenLexerOutcome) -> u8 {
    match outcome {
        crate::source_model::NonTokenLexerOutcome::NonEmitting => 0,
    }
}

const fn plan_non_token_outcome(
    outcome: crate::source_model::NonTokenLexerOutcome,
) -> NonTokenLexicalOutcome {
    match outcome {
        crate::source_model::NonTokenLexerOutcome::NonEmitting => {
            NonTokenLexicalOutcome::NonEmitting
        }
    }
}

fn fixed_matchers(model: &SourceModel, text_equivalence: TextEquivalence) -> Vec<FixedMatcher> {
    let mut fixed = BTreeMap::<String, (bool, bool)>::new();
    for definition in &model.token_definitions {
        let TokenDefinitionKind::Fixed(definition) = &definition.kind else {
            continue;
        };
        let word_shaped = is_word_shaped(&definition.spelling);
        let spelling = canonical_fixed_spelling(&definition.spelling, text_equivalence);
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

    fn proof(&self, spelling: &str) -> Option<&SymbolicCarrierProof> {
        self.proofs.get(spelling).and_then(Option::as_ref)
    }
}

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

pub(crate) fn plan_lexical(source: &SourceModeledGrammar) -> LexicalPlanReport {
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
                                callback: base.callback.clone(),
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
                callback: group.identity.callback.clone(),
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

const fn authored_fixed_class(class: AuthoredFixedTokenClass) -> FixedTokenClass {
    match class {
        AuthoredFixedTokenClass::Keyword => FixedTokenClass::Keyword,
        AuthoredFixedTokenClass::Punctuation => FixedTokenClass::Punctuation,
    }
}

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
                            callback: content.callback.clone(),
                            authored_priority: content.priority,
                            admitted_fixed_tokens,
                        }
                    }
                    TokenDefinitionKind::LexicalBase(base) => LexicalDefinitionMetadata::Content {
                        pattern: base.pattern.clone(),
                        callback: base.callback.clone(),
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

fn definition_metadata_key(metadata: &LexicalDefinitionMetadata) -> String {
    match metadata {
        LexicalDefinitionMetadata::Fixed {
            class,
            canonical_spelling,
        } => format!("fixed:{class:?}:{canonical_spelling:?}"),
        LexicalDefinitionMetadata::Content {
            pattern,
            callback,
            authored_priority,
            admitted_fixed_tokens,
        } => format!(
            "content:{pattern:?}:callback={}:priority={authored_priority:?}:admits={admitted_fixed_tokens:?}",
            callback
                .as_ref()
                .map_or_else(|| "-".to_owned(), crate::source_syntax::path_text),
        ),
    }
}

fn content_base_identity(
    content: &crate::source_model::ContentTokenDefinition,
) -> ContentBaseIdentity {
    ContentBaseIdentity {
        pattern: content.pattern.clone(),
        callback: content.callback.clone(),
        priority: content.priority,
    }
}

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
            left.identity.priority,
            left.identity
                .callback
                .as_ref()
                .map(crate::source_syntax::path_text),
        )
            .cmp(&(
                &right.identity.pattern,
                right.identity.priority,
                right
                    .identity
                    .callback
                    .as_ref()
                    .map(crate::source_syntax::path_text),
            ))
    });
    grouped
}

fn content_base_identity_for_definition(
    definition: &crate::source_model::TokenDefinition,
) -> Option<ContentBaseIdentity> {
    match &definition.kind {
        TokenDefinitionKind::Fixed(_) => None,
        TokenDefinitionKind::Content(content) => Some(content_base_identity(content)),
        TokenDefinitionKind::LexicalBase(base) => Some(ContentBaseIdentity {
            pattern: base.pattern.clone(),
            callback: base.callback.clone(),
            priority: base.priority,
        }),
    }
}

fn token_kind_by_identity(kinds: &[TokenKindPlan], identity: &TokenKindIdentity) -> KindId {
    kinds
        .iter()
        .find_map(|kind| (&kind.identity == identity).then_some(kind.id))
        .expect("every token-kind identity receives one kind")
}

fn content_base_kind(
    kinds: &[(ContentBaseIdentity, KindId)],
    identity: &ContentBaseIdentity,
) -> KindId {
    kinds
        .iter()
        .find_map(|(candidate, kind)| (candidate == identity).then_some(*kind))
        .expect("every content base receives one token kind")
}

fn lexical_diagnostics(
    source: &SourceModeledGrammar,
    content_groups: &[ContentBaseGroup<'_>],
    non_token_groups: &[NonTokenRuleGroup],
    retained_fixed_matchers: &[RetainedFixedMatcher],
) -> Vec<LexicalDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut contenders = Vec::new();
    for group in non_token_groups {
        match validate_pattern(&group.pattern) {
            Ok(()) => contenders.push(PatternContender {
                pattern: group.pattern.clone(),
                target_names: Vec::new(),
                authored_priority: None,
                priority_authorable: false,
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
                outcome: if group.identity.callback.is_some() {
                    LexicalPatternOutcome::OpaqueCallback
                } else {
                    LexicalPatternOutcome::EmitsTokenKind
                },
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
        )
            .cmp(&(
                &right.pattern,
                right.authored_priority,
                right.outcome,
                &right.target_names,
            ))
    });
    for (indices, witness) in pattern_ambiguities(&contenders, retained_fixed_matchers) {
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

fn pattern_evidence(contender: &PatternContender) -> LexicalPatternEvidence {
    LexicalPatternEvidence {
        pattern: contender.pattern.clone(),
        target_names: contender.target_names.clone(),
        authored_priority: contender.authored_priority,
        outcome: contender.outcome,
    }
}

fn accepts_empty<A: regex_automata::dfa::Automaton>(dfa: &A) -> bool {
    let input = Input::new(b"").anchored(Anchored::Yes);
    let Ok(state) = dfa.start_state_forward(&input) else {
        return false;
    };
    dfa.is_match_state(dfa.next_eoi_state(state))
}

fn pattern_ambiguities(
    contenders: &[PatternContender],
    retained_fixed_matchers: &[RetainedFixedMatcher],
) -> Vec<(Vec<usize>, String)> {
    if contenders.len() < 2 {
        return Vec::new();
    }
    let patterns = contenders
        .iter()
        .map(|contender| contender.pattern.as_str())
        .collect::<Vec<_>>();
    let mut builder = dense::Builder::new();
    builder.configure(dense::DFA::config().match_kind(MatchKind::All));
    let Ok(dfa) = builder.build_many(&patterns) else {
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
    ambiguities.into_iter().collect()
}

fn sorted_sites(source: &SourceModeledGrammar, mut sites: Vec<SourceSite>) -> Vec<SourceSite> {
    sites.sort_by_key(|site| source_site_key(source, site));
    sites
}

fn diagnostic_key(
    source: &SourceModeledGrammar,
    diagnostic: &LexicalDiagnostic,
) -> (crate::source_model::SourceOrderKey, LexicalDiagnosticCode) {
    let Some(site) = diagnostic.sites.first() else {
        return (crate::source_model::SourceOrderKey::LAST, diagnostic.code);
    };
    (source_site_key(source, site), diagnostic.code)
}

fn source_site_key(
    source: &SourceModeledGrammar,
    site: &SourceSite,
) -> crate::source_model::SourceOrderKey {
    source.source_order_key(site)
}

fn canonical_fixed_spelling(spelling: &str, equivalence: TextEquivalence) -> String {
    if is_word_shaped(spelling) && equivalence == TextEquivalence::AsciiInsensitive {
        spelling.to_ascii_uppercase()
    } else {
        spelling.to_owned()
    }
}

impl CarrierProver {
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
                opaque: definition.identity.callback.is_some(),
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
                .map(|_| CarrierCandidate::NonEmitting { priority: 0 }),
        );
        let (dfa, matcher_constructions) = if patterns.is_empty() {
            (None, 0)
        } else {
            let mut builder = dense::Builder::new();
            builder.configure(dense::DFA::config().match_kind(MatchKind::All));
            (builder.build_many(&patterns).ok(), 1)
        };
        Self {
            dfa,
            candidates,
            matcher_constructions,
        }
    }

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
                        opaque: false,
                        ..
                    } => {
                        if winning_group.is_some_and(|winner| winner != *group) {
                            return None;
                        }
                        winning_group = Some(*group);
                    }
                    CarrierCandidate::Emitting { opaque: true, .. }
                    | CarrierCandidate::NonEmitting { .. } => return None,
                }
            }
            groups.insert(winning_group?);
        }
        Some(groups.into_iter().collect())
    }
}

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
            callback: None,
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
    fn opaque_callback_silently_keeps_the_standalone_keyword_rule() {
        let source = source_model(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"[A-Za-z]+", callback = crate::scan_word)]
    word: Word,
}
"#,
        );

        let report = plan_lexical(&source);

        assert!(report.diagnostics.is_empty());
        let plan = report.plan.unwrap();
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
    fn central_only_opaque_callback_participates_in_carrier_proof_without_a_target_view() {
        let report = plan(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    lexer_tokens {
        OpaqueWord => r"[A-Za-z]+" with crate::scan_word,
    }
}

#[derive(recursa::Node)]
#[tok("IF")]
struct Root;
"#,
        );

        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let plan = report.plan.unwrap();
        assert_eq!(plan.content_bases.len(), 1);
        assert!(plan.content_bases[0].target_names.is_empty());
        assert_eq!(plan.parser_targets.len(), 0);
        assert_eq!(
            plan.keywords[0].recognition,
            super::KeywordRecognition::Standalone
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
        Broad => r"[A-Z]+" with crate::scan_broad,
        Narrow => r"[A-F]+" with crate::scan_narrow,
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
    fn carrier_proof_accounts_for_the_priority_of_an_opaque_callback_winner() {
        let callback_wins_source = source_model(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"[A-Za-z]+", priority = 1)]
    safe: Safe,
    #[lex(pattern = r"[A-Za-z]+", callback = crate::scan_word, priority = 2)]
    callback: Callback,
}
"#,
        );
        let callback_wins = plan_lexical(&callback_wins_source);
        assert!(callback_wins.diagnostics.is_empty());
        assert_eq!(
            callback_wins.plan.unwrap().keywords[0].recognition,
            super::KeywordRecognition::Standalone,
        );

        let safe_wins_source = source_model(
            r#"
recursa::grammar! {
    module = crate,
    keyword_matching = ascii_insensitive,
}

#[derive(recursa::Node)]
#[tok("IF", this)]
struct Root {
    #[lex(pattern = r"[A-Za-z]+", priority = 1)]
    safe: Safe,
    #[lex(pattern = r"[A-Za-z]+", callback = crate::scan_word, priority = 0)]
    callback: Callback,
}
"#,
        );
        let safe_wins = plan_lexical(&safe_wins_source);
        assert!(safe_wins.diagnostics.is_empty());
        let plan = safe_wins.plan.unwrap();
        let super::KeywordRecognition::CombinedCarriers {
            carrier,
            additional_carriers,
        } = &plan.keywords[0].recognition
        else {
            panic!("the higher-priority callback-free carrier should prove the keyword")
        };
        assert!(additional_carriers.is_empty());
        assert_eq!(
            plan.token_kinds[carrier.index()].identity,
            super::TokenKindIdentity::ContentBase(super::ContentBaseIdentity {
                pattern: r"[A-Za-z]+".to_owned(),
                callback: None,
                priority: Some(1),
            }),
        );
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
                .map(|diagnostic| diagnostic.code.as_str())
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
        A => r"x" with crate::cb_a,
        B => r"x" with crate::cb_b,
    }
}

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"x", priority = 1)]
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
        B => r"x" with crate::cb_b,
        A => r"x" with crate::cb_a,
    }
}

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"x", priority = 1)]
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
    fn inline_callback_and_priority_reach_the_lexical_plan_as_typed_facts() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(
        pattern = r"[A-Za-z_]+",
        callback = crate::scan_word,
        priority = 17,
    )]
    value: Word,
}
"#,
        );

        let content = source
            .source_model()
            .token_definitions
            .iter()
            .find_map(|definition| match &definition.kind {
                crate::source_model::TokenDefinitionKind::Content(content) => Some(content),
                crate::source_model::TokenDefinitionKind::Fixed(_)
                | crate::source_model::TokenDefinitionKind::LexicalBase(_) => None,
            })
            .unwrap();
        assert_eq!(content.callback, Some(syn::parse_quote!(crate::scan_word)));
        assert_eq!(content.priority, Some(17));
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
    fn central_and_inline_callback_definitions_produce_equivalent_typed_source_facts() {
        let source = source_model(
            r#"
recursa::grammar! { module = crate }

recursa::tokens! {
    literals {
        Word => r"[A-Za-z_]+" with crate::scan_word,
    }
}

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z_]+", callback = crate::scan_word)]
    value: Word,
}
"#,
        );
        let definitions = source
            .source_model()
            .token_definitions
            .iter()
            .filter(|definition| definition.name == "Word")
            .collect::<Vec<_>>();

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].definition_sites.len(), 2);
        let crate::source_model::TokenDefinitionKind::Content(content) = &definitions[0].kind
        else {
            panic!("Word must remain a content-token target")
        };
        assert_eq!(content.callback, Some(syn::parse_quote!(crate::scan_word)));
        assert!(plan_lexical(&source).diagnostics.is_empty());
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
