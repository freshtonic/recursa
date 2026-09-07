#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Lowering from the normalized grammar to LR rules.
//!
//! Terminals are the grammar's token kinds. Every grammar type becomes one nonterminal, except a
//! fixed token of one kind, which is inlined as its terminal so that keywords cost no reduce.
//! Pratt enums become plain alternatives with the implicit left operand prepended to infix and
//! postfix rules, and their binding powers are recorded on the rules for the precedence
//! resolver; a pair with no precedence equivalent is `RCA0402`. A `#[parse(pratt(exclude(...)))]`
//! site names a distinct language, so the Pratt nonterminal is cloned once per distinct
//! exclusion set (`Expr[excl In]`) without the excluded rules, the way gram.y has `b_expr`
//! beside `a_expr`; an operand the Pratt loop parses with a minimum binding power (every
//! operand but a bare last one) is cloned the same way without the LED rules below that
//! minimum (`Expr[min 8]`). Every parse entry type gets a synthetic start rule. A text
//! predicate other than `Any` is `RCA0403`.
//!
//! # Containers
//!
//! Repetitions lower to the left-recursive core `list1 -> e | list1 [sep] e`, whose reduce builds
//! a `Vec1` in order. Every other container is a choice, and a choice is lowered in one of two
//! ways: inlined into the parent rule as one alternative with the content and one without, which
//! is what gram.y writes by hand, or delegated to a shared nullable helper nonterminal, which is
//! bison's `opt_foo`. Inlining removes an epsilon reduce that LALR(1) would have to take before
//! the token that decides the choice; that matters when the token is a keyword an admission set
//! also accepts as content, because then the epsilon reduce competes with the shift of the same
//! keyword. So a fixed-only choice (an erased optional group, a presence, a trailing separator)
//! is always inlined as its terminals, and a valued choice (`Option<T>`, a zero-or-more list) is
//! inlined when its content can begin with an admitted keyword and delegated to a helper
//! otherwise. A rule inlines at most [`INLINE_BUDGET`](crate::lr::lowering::INLINE_BUDGET)
//! alternatives; past that, choices are demoted to helpers from the right, valued choices
//! first, so the expansion stays bounded.
//!
//! Helpers carry on their rules everything a reduce action needs to name the value it builds:
//! the element type and its `Box` layers, or the optional's content.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use super::grammar::{
    ElementValue, FoldKind, FoldPolicy, FoldSite, LrGrammar, NonterminalId, NonterminalOrigin,
    OptionalContent, Rule, RuleAction, RuleFold, RulePrecedence, SiteForm, SiteKind, Symbol,
    TailDecision, TerminalId, UnrepresentableBindingPowers,
};
use super::report::{LrDiagnostic, NO_PRECEDENCE_EQUIVALENT, TEXT_PREDICATE};
use crate::{
    AtomExpression, AtomId, ContainerShape, DeclId, GrammarDescription, KindSet, PrattPosition,
    PrattVariant, RepetitionCardinality, SiteId, TextPredicate, TypeId, TypeShape,
};

/// Expands atom membership into concrete token-kind sets for LR lowering.
pub(crate) fn expand_atoms(grammar: &GrammarDescription) -> Vec<KindSet> {
    fn expand(id: AtomId, grammar: &GrammarDescription, cache: &mut [Option<KindSet>]) -> KindSet {
        if let Some(cached) = &cache[id.index()] {
            return cached.clone();
        }
        let mut result = KindSet::empty(grammar.kind_count);
        match &grammar.atoms[id.index()].expression {
            AtomExpression::Kind(kind) => {
                result.insert(*kind).expect("validated kind ID");
            }
            AtomExpression::Category(members) | AtomExpression::Class(members) => {
                for member in members {
                    result.union_assign(&expand(*member, grammar, cache));
                }
            }
            AtomExpression::Target { base, admits } => {
                result.union_assign(&expand(*base, grammar, cache));
                for admitted in admits {
                    result.union_assign(&expand(*admitted, grammar, cache));
                }
            }
        }
        cache[id.index()] = Some(result.clone());
        result
    }

    let mut cache = vec![None; grammar.atoms.len()];
    (0..grammar.atoms.len())
        .map(|index| expand(AtomId::new(index as u32), grammar, &mut cache))
        .collect()
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// A distinct Pratt language: a Pratt type minus some of its variants, of any position (a
/// `#[parse(pratt(exclude(...)))]` site, carried into every operand) and minus every LED
/// variant below a minimum binding power (an operand that the Pratt loop parses with
/// `parse_bp(min_bp)`, not carried into its operands). Two sites with equal restrictions share
/// one clone.
pub struct PrattRestriction {
    /// The Pratt type restricted.
    pub type_id: TypeId,
    /// The excluded variants, atoms and prefix variants included, as normalized declarations
    /// in ascending order, deduplicated.
    pub excluded: Vec<DeclId>,
    /// The minimum binding power: LED variants whose `lbp` or `bp` is below it are left out.
    pub min_bp: u8,
}

/// The most alternatives one rule's inlined choices may multiply to before choices are demoted
/// to helper nonterminals: four binary choices.
pub const INLINE_BUDGET: usize = 16;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// The facts about normalized types that the normalized grammar alone does not state: which
/// token types carry their token as a value, which sequences are fixed-token-only, which sites
/// select a boolean presence, and which types are parse entry points.
pub struct TypeRoles {
    /// Token types whose value is the matched token; every other token type is fixed.
    pub content_tokens: BTreeSet<TypeId>,
    /// Sequence types made only of fixed tokens; they carry no value.
    pub fixed_sequences: BTreeSet<TypeId>,
    /// Optional sites whose presence is the value of an authored `bool` field.
    pub presence_sites: BTreeSet<SiteId>,
    /// Types that get a start rule: node applications and content tokens.
    pub start_types: Vec<TypeId>,
    /// Sites that restrict their Pratt target with `#[parse(pratt(exclude(...)))]`, atoms and
    /// prefix variants included, keyed by the site the authored field lowers to (the outer site
    /// when fixed tokens are attached); the restriction carries no minimum binding power.
    pub pratt_exclusions: BTreeMap<SiteId, PrattRestriction>,
    /// Node application types, whose rules construct a node value.
    pub applications: BTreeSet<TypeId>,
    /// Repetition attachment units, each with the semantic site whose value it passes through;
    /// a Pratt operand behind one carries its exclusion set through it.
    pub attachment_units: BTreeMap<TypeId, SiteId>,
    /// Fixed-token presence envelopes, each with the index of its optional presence site.
    pub presence_envelopes: BTreeMap<TypeId, usize>,
}

impl TypeRoles {
    /// Reads the roles off a lowered grammar's correlations.
    pub(crate) fn from_lowered(lowered: &crate::materialization::LoweredGrammar) -> Self {
        let map = &lowered.lowering_map;
        let content_tokens: BTreeSet<TypeId> = map
            .content_targets
            .iter()
            .map(|correlation| correlation.lowered)
            .collect();
        let mut fixed_sequences = BTreeSet::new();
        let mut presence_sites = BTreeSet::new();
        let mut presence_envelopes = BTreeMap::new();
        for presence in &map.presences {
            fixed_sequences.insert(presence.sequence);
            presence_sites.insert(if presence.outer_is_optional {
                presence.outer
            } else {
                presence.envelope_sites[presence.presence_index]
            });
            presence_envelopes.insert(presence.envelope, presence.presence_index);
        }
        let attachment_units: BTreeMap<TypeId, SiteId> = map
            .attachments
            .iter()
            .map(|unit| (unit.unit, unit.sites[unit.semantic_index]))
            .collect();
        for group in &map.optional_fixed_groups {
            fixed_sequences.insert(group.sequence);
        }
        // An erased group inside an attachment has no authored element and so no correlation;
        // recognize every fixed-only sequence by its shape as well.
        let applications: BTreeSet<TypeId> = map
            .applications
            .iter()
            .map(|correlation| correlation.lowered)
            .collect();
        let grammar = &lowered.grammar;
        for (index, description) in grammar.types.iter().enumerate() {
            let type_id = TypeId::new(index as u32);
            let TypeShape::Sequence { sites } = &description.shape else {
                continue;
            };
            if applications.contains(&type_id) || sites.is_empty() {
                continue;
            }
            let fixed_only = sites.iter().all(|site| {
                let site = &grammar.sites[site.index()];
                matches!(site.container, ContainerShape::Direct)
                    && matches!(
                        grammar.types[site.target.index()].shape,
                        TypeShape::Token { .. }
                    )
                    && !content_tokens.contains(&site.target)
            });
            if fixed_only {
                fixed_sequences.insert(type_id);
            }
        }
        let mut start_types: Vec<TypeId> = map
            .applications
            .iter()
            .map(|correlation| correlation.lowered)
            .chain(content_tokens.iter().copied())
            .collect();
        start_types.sort_unstable();
        start_types.dedup();
        Self {
            pratt_exclusions: pratt_exclusions(lowered, &attachment_units),
            content_tokens,
            fixed_sequences,
            presence_sites,
            start_types,
            applications,
            attachment_units,
            presence_envelopes,
        }
    }
}

/// Reads every site's resolved `#[parse(pratt(exclude(...)))]` exclusion (materialization
/// records it on the site the field lowers to, the outer site when fixed tokens are attached)
/// as the restriction of the Pratt type the operand belongs to, which is the site's target or
/// the semantic target of the attachment unit it targets.
fn pratt_exclusions(
    lowered: &crate::materialization::LoweredGrammar,
    attachment_units: &BTreeMap<TypeId, SiteId>,
) -> BTreeMap<SiteId, PrattRestriction> {
    let grammar = &lowered.grammar;
    grammar
        .sites
        .iter()
        .enumerate()
        .filter(|(_, site)| !site.pratt_excluded.is_empty())
        .map(|(index, site)| {
            let type_id = attachment_units
                .get(&site.target)
                .map_or(site.target, |semantic| {
                    grammar.sites[semantic.index()].target
                });
            (
                SiteId::new(index as u32),
                PrattRestriction {
                    type_id,
                    excluded: site.pratt_excluded.clone(),
                    min_bp: 0,
                },
            )
        })
        .collect()
}

/// Lowers a normalized grammar to an LR grammar, or reports every text predicate (`RCA0403`)
/// and every Pratt binding power with no precedence equivalent (`RCA0402`) it carries.
pub fn lower(
    grammar: &GrammarDescription,
    roles: &TypeRoles,
) -> Result<LrGrammar, Vec<LrDiagnostic>> {
    let diagnostics = text_predicate_diagnostics(grammar);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut lowering = Lowering::new(grammar, roles);
    lowering.note_operands();
    lowering.lower_types();
    lowering.lower_clones();
    if !lowering.diagnostics.is_empty() {
        return Err(lowering.diagnostics);
    }
    lowering.decide_tails();
    let start_nonterminals = lowering.start_nonterminals();
    super::lookahead::apply(&mut lowering.lr, grammar, &start_nonterminals);
    lowering.lower_starts();
    super::declared_precedence::apply(&mut lowering.lr, grammar);
    Ok(lowering.lr)
}

/// Reports `RCA0403` for every token type refined by a text predicate other than `Any`.
fn text_predicate_diagnostics(grammar: &GrammarDescription) -> Vec<LrDiagnostic> {
    let mut diagnostics = Vec::new();
    for description in &grammar.types {
        let TypeShape::Token {
            text_predicate: Some(predicate),
            ..
        } = &description.shape
        else {
            continue;
        };
        let predicate = &grammar.text_predicates[predicate.index()];
        let rendered = match predicate {
            TextPredicate::Any => continue,
            TextPredicate::Exact(text) => format!("exact text `{text}`"),
            TextPredicate::OneOf(texts) => format!("one of {texts:?}"),
            TextPredicate::NotIn(texts) => format!("none of {texts:?}"),
        };
        diagnostics.push(LrDiagnostic {
            code: TEXT_PREDICATE,
            message: format!(
                "token type `{}` matches {rendered}; text predicates are not supported by the \
                 Recursa parser",
                description.name
            ),
            path: String::new(),
            items: Vec::new(),
            actions: Vec::new(),
            notes: Vec::new(),
        });
    }
    diagnostics
}

/// Names every token kind for reports: its authored attribute spelling when it has one, else
/// the name of the one token type that matches exactly that kind, else its normalized name.
fn terminal_names(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> Vec<String> {
    let mut names: Vec<Option<String>> = grammar
        .kinds
        .iter()
        .map(|kind| kind.authored.clone())
        .collect();
    let mut claimed = vec![false; grammar.kinds.len()];
    for description in &grammar.types {
        let TypeShape::Token { atom, .. } = &description.shape else {
            continue;
        };
        let kinds: Vec<_> = atom_kinds[atom.index()].iter().collect();
        let [kind] = kinds.as_slice() else {
            continue;
        };
        let slot = &mut names[kind.index()];
        if slot.is_none() && !claimed[kind.index()] {
            *slot = Some(description.name.clone());
        } else if slot.as_deref() != Some(description.name.as_str()) && claimed[kind.index()] {
            *slot = None;
        }
        claimed[kind.index()] = true;
    }
    let mut seen = BTreeSet::new();
    names
        .into_iter()
        .zip(&grammar.kinds)
        .map(|(name, kind)| {
            let name = name.unwrap_or_else(|| kind.name.clone());
            if seen.insert(name.clone()) {
                name
            } else {
                kind.name.clone()
            }
        })
        .collect()
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Identity of a shared helper nonterminal.
enum HelperKey {
    /// A nullable optional around the given present alternatives, building `content`.
    Optional {
        /// The right-hand sides of the present alternatives.
        present: Vec<Vec<Symbol>>,
        /// What the helper builds.
        content: OptionalContent,
    },
    /// The core `list1 -> e | list1 [sep] e`.
    NonEmptyList {
        /// The repeated symbol.
        element: Symbol,
        /// `Box` layers around each element, which change the pushed type.
        boxes: u32,
        /// The separator symbol, if any.
        separator: Option<Symbol>,
    },
    /// `list1t -> list1 | list1 sep`.
    Trailing {
        /// The core list.
        list1: NonterminalId,
        /// The separator.
        separator: Symbol,
    },
    /// `list -> %empty | list1 [sep]`.
    ZeroOrMore {
        /// The core list.
        list1: NonterminalId,
        /// The separator, when one trailing separator is accepted.
        trailing: Option<Symbol>,
    },
}

/// A one-or-more list inside an optional: `Option<Vec1<T>>`.
#[derive(Clone, Copy, Debug)]
struct InnerList {
    /// The core list.
    list1: NonterminalId,
    /// The separator, when one trailing separator is accepted.
    trailing: Option<Symbol>,
    /// Whether the trailing choice is inlined rather than delegated to `list1t`.
    trailing_inline: bool,
}

/// What a valued optional wraps.
#[derive(Clone, Copy, Debug)]
enum OptionalInner {
    /// One symbol.
    Element(Symbol),
    /// A one-or-more list.
    List(InnerList),
}

/// How one site of a sequence is lowered, before and after the budget decides which of its
/// choices are inlined.
#[derive(Clone, Debug)]
enum SiteLowering {
    /// A required site: one symbol.
    Required {
        /// The site's symbol.
        symbol: Symbol,
        /// The site's fold: a fixed token's leaf, or a group around any other target.
        kind: SiteKind,
    },
    /// An erased optional group or a presence: inlined as its terminals or a helper.
    FixedOptional {
        /// The fixed-token symbols of the atomic sequence.
        tokens: Vec<Symbol>,
        /// Whether the choice is the value of a `bool` field.
        presence: bool,
        /// Whether the choice is inlined.
        inline: bool,
    },
    /// `Option<T>` or `Option<Vec1<T>>`: inlined as absent-or-present or a helper.
    ValuedOptional {
        /// What the optional wraps.
        inner: OptionalInner,
        /// The value the helper would build.
        content: OptionalContent,
        /// Whether the choice is inlined.
        inline: bool,
        /// Whether this optional must remain inlined to preserve a proven separated-list tail.
        required_inline: bool,
    },
    /// `Vec1<T>`: the core list, with a trailing-separator choice when accepted.
    OneOrMore {
        /// The core list.
        list1: NonterminalId,
        /// The separator, when one trailing separator is accepted.
        trailing: Option<Symbol>,
        /// Whether the trailing choice is inlined.
        trailing_inline: bool,
    },
    /// `Vec<T>`: absent or the core list, inlined or a helper.
    ZeroOrMore {
        /// The core list.
        list1: NonterminalId,
        /// The list element.
        element: ElementValue,
        /// The separator, when one trailing separator is accepted.
        trailing: Option<Symbol>,
        /// Whether the presence choice is inlined.
        inline: bool,
        /// Whether the trailing choice is inlined.
        trailing_inline: bool,
    },
}

impl SiteLowering {
    /// Counts the alternatives the site contributes to the parent rule.
    fn alternative_count(&self) -> usize {
        let list_count = |trailing: Option<Symbol>, inline: bool| {
            if trailing.is_some() && inline { 2 } else { 1 }
        };
        match self {
            Self::Required { .. } => 1,
            Self::FixedOptional { inline, .. } => 1 + usize::from(*inline),
            Self::ValuedOptional { inline: false, .. } => 1,
            Self::ValuedOptional {
                inner: OptionalInner::Element(_),
                ..
            } => 2,
            Self::ValuedOptional {
                inner: OptionalInner::List(list),
                ..
            } => 1 + list_count(list.trailing, list.trailing_inline),
            Self::OneOrMore {
                trailing,
                trailing_inline,
                ..
            } => list_count(*trailing, *trailing_inline),
            Self::ZeroOrMore { inline: false, .. } => 1,
            Self::ZeroOrMore {
                trailing,
                trailing_inline,
                ..
            } => 1 + list_count(*trailing, *trailing_inline),
        }
    }

    /// Demotes the site's valued choice to a helper; reports whether there was one to demote.
    fn demote_valued(&mut self) -> bool {
        match self {
            Self::ValuedOptional {
                inline,
                required_inline: false,
                ..
            }
            | Self::ZeroOrMore { inline, .. }
                if *inline =>
            {
                *inline = false;
                true
            }
            _ => false,
        }
    }

    /// Demotes the site's fixed-only choice to a helper; reports whether there was one.
    fn demote_fixed(&mut self) -> bool {
        match self {
            Self::FixedOptional { inline, .. } if *inline => {
                *inline = false;
                true
            }
            Self::ValuedOptional {
                inner:
                    OptionalInner::List(InnerList {
                        trailing: Some(_),
                        trailing_inline,
                        ..
                    }),
                ..
            }
            | Self::OneOrMore {
                trailing: Some(_),
                trailing_inline,
                ..
            }
            | Self::ZeroOrMore {
                trailing: Some(_),
                trailing_inline,
                ..
            } if *trailing_inline => {
                *trailing_inline = false;
                true
            }
            _ => false,
        }
    }
}

/// One alternative a site contributes to its parent rule.
#[derive(Clone, Debug)]
struct Alternative {
    /// The symbols the site occupies.
    symbols: Vec<Symbol>,
    /// Whether the site's content is present.
    present: bool,
    /// Whether the choice was inlined (see [`SiteForm::inlined`]).
    inlined: bool,
    /// What the site folds to on the occurrence stack.
    kind: SiteKind,
    /// The policy of the site's fold.
    policy: FoldPolicy,
}

/// What the rules of one alternative set build.
#[derive(Clone, Copy, Debug)]
enum ConstructKind {
    /// A grammar type's value, from its explicit sites.
    Construct {
        /// The type.
        type_id: TypeId,
        /// The enum alternative or Pratt variant.
        variant: Option<DeclId>,
    },
    /// A fixed-only sequence: no value.
    Fixed {
        /// The type.
        type_id: TypeId,
    },
    /// An attachment unit: the semantic child's value passes through.
    Attachment {
        /// The type.
        type_id: TypeId,
    },
    /// A presence envelope: a `bool`.
    Envelope {
        /// The type.
        type_id: TypeId,
        /// The index of the presence site among the envelope's sites.
        presence_index: usize,
    },
}

/// The lowering state: the grammar being built and the symbol chosen for every type.
struct Lowering<'a> {
    /// The normalized grammar.
    grammar: &'a GrammarDescription,
    /// The type roles.
    roles: &'a TypeRoles,
    /// The kind set of every atom.
    atom_kinds: Vec<KindSet>,
    /// FIRST kinds of every type.
    first: Vec<KindSet>,
    /// Keyword kinds that a content token also admits; a choice whose content can begin with
    /// one is inlined.
    admitted_keywords: KindSet,
    /// The LR grammar under construction.
    lr: LrGrammar,
    /// The symbol each type lowers to: an inlined terminal or its nonterminal.
    type_symbols: Vec<Symbol>,
    /// Shared helper nonterminals.
    helpers: HashMap<HelperKey, NonterminalId>,
    /// Names already taken, to keep nonterminal names unique.
    names: BTreeSet<String>,
    /// `RCA0402` diagnostics collected while lowering Pratt variants.
    diagnostics: Vec<LrDiagnostic>,
    /// The clone of each distinct Pratt restriction, created on first reference.
    clones: BTreeMap<PrattRestriction, NonterminalId>,
    /// The clone of each attachment sequence around a Pratt operand under an exclusion set.
    attachment_clones: BTreeMap<(TypeId, Vec<DeclId>), NonterminalId>,
    /// Clones whose rules are still to be emitted, in creation order.
    pending_clones: VecDeque<(PrattRestriction, NonterminalId)>,
    /// What every fixed-token attachment around a Pratt operand belongs to, keyed by the
    /// attachment sequence type.
    operand_attachments: HashMap<TypeId, OperandAttachment>,
    /// Optional tail sites that must inline after a preceding separated list.
    separated_tail_sites: BTreeSet<SiteId>,
}

#[derive(Clone, Copy)]
/// A fixed-token attachment around the operand of a prefix, infix or postfix Pratt variant.
struct OperandAttachment {
    /// The Pratt type the operand belongs to.
    pratt: TypeId,
    /// The precedence the attachment's rule inherits from the variant, so that its reduce
    /// against the next operator is decided as the variant's would be.
    precedence: Option<RulePrecedence>,
}

#[derive(Clone, Copy)]
/// The Pratt rule whose operand sites are lowering: the type its recursive operands belong to
/// and the exclusion set they carry.
struct OperandContext<'a> {
    /// The Pratt type of the rule.
    pratt: TypeId,
    /// The excluded variants the rule's operands carry, of any position; empty outside an
    /// exclusion clone.
    excluded: &'a [DeclId],
}

#[derive(Clone, Copy)]
/// How the operand sites of one alternative lower: under which Pratt rule, and whether the
/// alternative's last site is a bare operand that keeps the full language (a Pratt variant's
/// own last operand) rather than one that takes its minimum binding power (every operand of
/// an attachment sequence).
struct Operands<'a> {
    /// The Pratt rule the sites belong to.
    context: OperandContext<'a>,
    /// Whether the last site is the rule's bare last operand.
    bare_last: bool,
}

/// Strips the `Box` layers off a container shape, counting them.
fn strip_boxes(shape: &ContainerShape) -> (u32, &ContainerShape) {
    match shape {
        ContainerShape::Boxed(inner) => {
            let (boxes, inner) = strip_boxes(inner);
            (boxes + 1, inner)
        }
        other => (0, other),
    }
}

/// Computes the FIRST kinds and nullability of every normalized type by fixpoint.
pub(crate) fn first_sets(
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> (Vec<KindSet>, Vec<bool>) {
    let kind_count = grammar.kind_count;
    let mut first = vec![KindSet::empty(kind_count); grammar.types.len()];
    let mut nullable = vec![false; grammar.types.len()];
    let site_first = |site: SiteId, first: &[KindSet], nullable: &[bool]| -> (KindSet, bool) {
        let site = &grammar.sites[site.index()];
        let target = site.target.index();
        let (_, shape) = strip_boxes(&site.container);
        let site_nullable = match shape {
            ContainerShape::Direct | ContainerShape::Boxed(_) => nullable[target],
            ContainerShape::Optional(_) => true,
            ContainerShape::Repeat { cardinality, .. } => {
                nullable[target] || *cardinality == RepetitionCardinality::ZeroOrMore
            }
        };
        (first[target].clone(), site_nullable)
    };
    let sequence_first =
        |sites: &[SiteId], first: &[KindSet], nullable: &[bool]| -> (KindSet, bool) {
            let mut kinds = KindSet::empty(kind_count);
            for site in sites {
                let (site_kinds, site_nullable) = site_first(*site, first, nullable);
                kinds.union_assign(&site_kinds);
                if !site_nullable {
                    return (kinds, false);
                }
            }
            (kinds, true)
        };
    loop {
        let mut changed = false;
        for (index, description) in grammar.types.iter().enumerate() {
            let (kinds, is_nullable) = match &description.shape {
                TypeShape::Token { atom, .. } => (atom_kinds[atom.index()].clone(), false),
                TypeShape::Alias { site } => site_first(*site, &first, &nullable),
                TypeShape::Sequence { sites } => sequence_first(sites, &first, &nullable),
                TypeShape::Enum { alternatives } => {
                    let mut kinds = KindSet::empty(kind_count);
                    let mut is_nullable = false;
                    for alternative in alternatives {
                        let (alternative_kinds, alternative_nullable) =
                            sequence_first(&alternative.sites, &first, &nullable);
                        kinds.union_assign(&alternative_kinds);
                        is_nullable |= alternative_nullable;
                    }
                    (kinds, is_nullable)
                }
                TypeShape::Pratt { variants } => {
                    let mut kinds = KindSet::empty(kind_count);
                    let mut is_nullable = false;
                    for variant in variants {
                        if matches!(
                            variant.position,
                            PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
                        ) {
                            continue;
                        }
                        let (variant_kinds, variant_nullable) =
                            sequence_first(&variant.sites, &first, &nullable);
                        kinds.union_assign(&variant_kinds);
                        is_nullable |= variant_nullable;
                    }
                    (kinds, is_nullable)
                }
            };
            if kinds.words() != first[index].words() || is_nullable != nullable[index] {
                first[index] = kinds;
                nullable[index] = is_nullable;
                changed = true;
            }
        }
        if !changed {
            return (first, nullable);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The facts needed from a language's prefixes of at most two tokens.
///
/// The separated-tail test uses only the union at each position and whether a complete
/// derivation has length zero or one. Keeping those facts symbolically avoids materializing the
/// Cartesian product of token-kind pairs for every normalized type.
struct PrefixSummary {
    /// Whether the language has any productive derivation.
    productive: bool,
    /// Whether the language has an empty derivation.
    nullable: bool,
    /// Whether the language has a complete one-token derivation.
    one_token: bool,
    /// Token kinds that occur in the first position of a productive derivation.
    first: KindSet,
    /// Token kinds that occur in the second position of a productive derivation.
    second: KindSet,
}

impl PrefixSummary {
    /// Creates the empty language for one grammar width.
    fn empty(kind_count: usize) -> Self {
        Self {
            productive: false,
            nullable: false,
            one_token: false,
            first: KindSet::empty(kind_count),
            second: KindSet::empty(kind_count),
        }
    }

    /// Creates the language containing only the empty derivation.
    fn epsilon(kind_count: usize) -> Self {
        Self {
            productive: true,
            nullable: true,
            ..Self::empty(kind_count)
        }
    }

    /// Creates a one-token language from its possible token kinds.
    fn tokens(kinds: &KindSet) -> Self {
        Self {
            productive: !kinds.is_empty(),
            nullable: false,
            one_token: !kinds.is_empty(),
            first: kinds.clone(),
            second: KindSet::empty(kinds.kind_count()),
        }
    }

    /// Adds another alternative of the same grammar width.
    fn union_assign(&mut self, other: &Self) {
        self.productive |= other.productive;
        self.nullable |= other.nullable;
        self.one_token |= other.one_token;
        self.first.union_assign(&other.first);
        self.second.union_assign(&other.second);
    }
}

/// Concatenates two languages while retaining their symbolic two-token prefix facts.
fn concat_summaries(left: &PrefixSummary, right: &PrefixSummary) -> PrefixSummary {
    if !left.productive || !right.productive {
        return PrefixSummary::empty(left.first.kind_count());
    }
    let mut first = left.first.clone();
    let mut second = left.second.clone();
    if left.nullable {
        first.union_assign(&right.first);
        second.union_assign(&right.second);
    }
    if left.one_token {
        second.union_assign(&right.first);
    }
    PrefixSummary {
        productive: true,
        nullable: left.nullable && right.nullable,
        one_token: (left.one_token && right.nullable) || (left.nullable && right.one_token),
        first,
        second,
    }
}

/// Applies one normalized container shape to a target's symbolic prefix summary.
fn container_summary(
    shape: &ContainerShape,
    target: &PrefixSummary,
    types: &[PrefixSummary],
) -> PrefixSummary {
    match shape {
        ContainerShape::Direct => target.clone(),
        ContainerShape::Boxed(inner) => container_summary(inner, target, types),
        ContainerShape::Optional(inner) => {
            let mut result = container_summary(inner, target, types);
            result.union_assign(&PrefixSummary::epsilon(target.first.kind_count()));
            result
        }
        ContainerShape::Repeat {
            element,
            cardinality,
            separator,
        } => {
            let element = container_summary(element, target, types);
            let continuation = if let Some(separator) = separator {
                concat_summaries(&types[separator.type_id.index()], &element)
            } else {
                element.clone()
            };
            let mut result = element.clone();
            loop {
                let expanded = concat_summaries(&result, &continuation);
                let mut next = result.clone();
                next.union_assign(&expanded);
                if next == result {
                    break;
                }
                result = next;
            }
            if *cardinality == RepetitionCardinality::ZeroOrMore {
                result.union_assign(&PrefixSummary::epsilon(target.first.kind_count()));
            }
            result
        }
    }
}

/// Computes the symbolic two-token prefix facts of every normalized type.
fn two_token_summaries(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> Vec<PrefixSummary> {
    fn sites(
        ids: &[SiteId],
        grammar: &GrammarDescription,
        previous: &[PrefixSummary],
    ) -> PrefixSummary {
        let mut result = PrefixSummary::epsilon(grammar.kind_count);
        for id in ids {
            let site = &grammar.sites[id.index()];
            let next = container_summary(&site.container, &previous[site.target.index()], previous);
            result = concat_summaries(&result, &next);
        }
        result
    }

    let mut summaries = vec![PrefixSummary::empty(grammar.kind_count); grammar.types.len()];
    loop {
        let previous = summaries.clone();
        for (index, description) in grammar.types.iter().enumerate() {
            summaries[index] = match &description.shape {
                TypeShape::Token { atom, .. } => PrefixSummary::tokens(&atom_kinds[atom.index()]),
                TypeShape::Alias { site } => sites(std::slice::from_ref(site), grammar, &previous),
                TypeShape::Sequence { sites: sequence } => sites(sequence, grammar, &previous),
                TypeShape::Enum { alternatives } => {
                    let mut summary = PrefixSummary::empty(grammar.kind_count);
                    for alternative in alternatives {
                        summary.union_assign(&sites(&alternative.sites, grammar, &previous));
                    }
                    summary
                }
                TypeShape::Pratt { variants } => {
                    let mut summary = PrefixSummary::empty(grammar.kind_count);
                    for variant in variants.iter().filter(|variant| {
                        matches!(
                            variant.position,
                            PrattPosition::Atom | PrattPosition::Prefix { .. }
                        )
                    }) {
                        summary.union_assign(&sites(&variant.sites, grammar, &previous));
                    }
                    summary
                }
            };
        }
        if summaries == previous {
            return summaries;
        }
    }
}

/// Finds optional suffixes that must inline after a separated list so the suffix can own the
/// shared separator without an epsilon reduction.
fn separated_tail_sites(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> BTreeSet<SiteId> {
    let prefixes = two_token_summaries(grammar, atom_kinds);
    let mut tails = vec![None; grammar.sites.len()];
    let mut record = |sequence: &[SiteId]| {
        for pair in sequence.windows(2) {
            tails[pair[0].index()] = Some(pair[1]);
        }
    };
    for description in &grammar.types {
        match &description.shape {
            TypeShape::Alias { .. } | TypeShape::Token { .. } => {}
            TypeShape::Sequence { sites } => record(sites),
            TypeShape::Enum { alternatives } => {
                for alternative in alternatives {
                    record(&alternative.sites);
                }
            }
            TypeShape::Pratt { variants } => {
                for variant in variants {
                    record(&variant.sites);
                }
            }
        }
    }

    grammar
        .sites
        .iter()
        .enumerate()
        .filter_map(|(index, site)| {
            let semantics = site.container.semantics()?;
            if !semantics.cardinality.is_repeated() {
                return None;
            }
            let separator = semantics
                .separator
                .filter(|separator| !separator.optional_trailing)?;
            let tail = tails[index]?;
            let tail_site = &grammar.sites[tail.index()];
            if tail_site.container.semantics()?.cardinality
                != crate::ir::ContainerCardinality::Optional
            {
                return None;
            }
            let separator_prefixes = &prefixes[separator.type_id.index()];
            if !separator_prefixes.productive || separator_prefixes.nullable {
                return None;
            }
            let tail_prefixes = &prefixes[tail_site.target.index()];
            if !tail_prefixes.productive || tail_prefixes.nullable || tail_prefixes.one_token {
                return None;
            }
            if tail_prefixes.first != separator_prefixes.first {
                return None;
            }
            let element_first = &prefixes[site.target.index()].first;
            (!element_first.is_empty()
                && !tail_prefixes
                    .second
                    .intersects(element_first)
                    .expect("prefix facts share one grammar width"))
            .then_some(tail)
        })
        .collect()
}

impl<'a> Lowering<'a> {
    /// Creates the terminals and one nonterminal per type that needs one.
    fn new(grammar: &'a GrammarDescription, roles: &'a TypeRoles) -> Self {
        let atom_kinds = expand_atoms(grammar);
        let (first, _) = first_sets(grammar, &atom_kinds);
        let separated_tail_sites = separated_tail_sites(grammar, &atom_kinds);
        let kind_count = grammar.kind_count;
        let mut content_kinds = KindSet::empty(kind_count);
        let mut fixed_kinds = KindSet::empty(kind_count);
        for (index, description) in grammar.types.iter().enumerate() {
            let TypeShape::Token { atom, .. } = &description.shape else {
                continue;
            };
            if roles.content_tokens.contains(&TypeId::new(index as u32)) {
                content_kinds.union_assign(&atom_kinds[atom.index()]);
            } else {
                fixed_kinds.union_assign(&atom_kinds[atom.index()]);
            }
        }
        let admitted_keywords = content_kinds
            .intersection(&fixed_kinds)
            .expect("kind sets of one grammar share a width");
        let mut lr = LrGrammar::new(terminal_names(grammar, &atom_kinds));
        let mut names = BTreeSet::new();
        let mut type_symbols: Vec<Symbol> = Vec::with_capacity(grammar.types.len());
        for (index, description) in grammar.types.iter().enumerate() {
            let type_id = TypeId::new(index as u32);
            let symbol = match &description.shape {
                TypeShape::Token { atom, .. } if !roles.content_tokens.contains(&type_id) => {
                    let kinds: Vec<_> = atom_kinds[atom.index()].iter().collect();
                    match kinds.as_slice() {
                        [kind] => Some(Symbol::Terminal(TerminalId::new(kind.get() as u32))),
                        _ => None,
                    }
                }
                _ => None,
            };
            let symbol = symbol.unwrap_or_else(|| {
                let carries_value = match &description.shape {
                    TypeShape::Token { .. } => roles.content_tokens.contains(&type_id),
                    TypeShape::Sequence { .. } if roles.fixed_sequences.contains(&type_id) => false,
                    // A unit passes its semantic child's value through, so it carries one
                    // exactly when the child does; the child precedes the unit in type order.
                    TypeShape::Sequence { .. } => match roles.attachment_units.get(&type_id) {
                        Some(child) => {
                            let target = grammar.sites[child.index()].target;
                            match type_symbols[target.index()] {
                                Symbol::Terminal(_) => false,
                                Symbol::Nonterminal(id) => lr.nonterminal(id).carries_value,
                            }
                        }
                        None => true,
                    },
                    _ => true,
                };
                let mut name = description.name.clone();
                if !names.insert(name.clone()) {
                    name = format!("{name}#{index}");
                    names.insert(name.clone());
                }
                Symbol::Nonterminal(lr.add_nonterminal(
                    name,
                    NonterminalOrigin::Type(type_id),
                    carries_value,
                ))
            });
            type_symbols.push(symbol);
        }
        Self {
            grammar,
            roles,
            atom_kinds,
            first,
            admitted_keywords,
            lr,
            type_symbols,
            helpers: HashMap::new(),
            names,
            diagnostics: Vec::new(),
            clones: BTreeMap::new(),
            attachment_clones: BTreeMap::new(),
            pending_clones: VecDeque::new(),
            operand_attachments: HashMap::new(),
            separated_tail_sites,
        }
    }

    /// Records, before any rule is emitted, which fixed-token attachments wrap the operand of a
    /// prefix, infix or postfix Pratt variant and what they inherit: the variant's precedence,
    /// so that the attachment's reduce against the next operator is decided as the variant's
    /// would be. The operand inside lowers to the minimum-binding-power clone the Pratt loop
    /// parses it with.
    fn note_operands(&mut self) {
        for (index, description) in self.grammar.types.iter().enumerate() {
            let TypeShape::Pratt { variants } = &description.shape else {
                continue;
            };
            let type_id = TypeId::new(index as u32);
            let Symbol::Nonterminal(owner) = self.type_symbols[index] else {
                continue;
            };
            for variant in variants {
                if matches!(variant.position, PrattPosition::Atom) {
                    continue;
                }
                let precedence = RulePrecedence::from_position(variant.position, owner)
                    .ok()
                    .flatten();
                for site in &variant.sites {
                    let site = &self.grammar.sites[site.index()];
                    let unit = site.target;
                    if self.wraps_operand_of(unit, type_id) {
                        // The attachment around an optional operand introduces its leading
                        // token at the variant's level plus one, so that the token shifts
                        // against the variant's own reduce (`a LIKE b . ESCAPE`).
                        let optional =
                            matches!(strip_boxes(&site.container).1, ContainerShape::Optional(_));
                        let precedence = precedence.map(|precedence| RulePrecedence {
                            attaches_at: optional.then(|| precedence.level.saturating_add(1)),
                            ..precedence
                        });
                        self.operand_attachments.insert(
                            unit,
                            OperandAttachment {
                                pratt: type_id,
                                precedence,
                            },
                        );
                    }
                }
            }
        }
    }

    /// The nonterminals that receive a start rule, in start-type order.
    fn start_nonterminals(&self) -> Vec<NonterminalId> {
        self.roles
            .start_types
            .iter()
            .filter_map(|type_id| match self.type_symbols[type_id.index()] {
                Symbol::Nonterminal(target) => Some(target),
                Symbol::Terminal(_) => None,
            })
            .collect()
    }

    /// Emits the rules of every type that has a nonterminal.
    fn lower_types(&mut self) {
        for index in 0..self.grammar.types.len() {
            let type_id = TypeId::new(index as u32);
            let Symbol::Nonterminal(lhs) = self.type_symbols[index] else {
                continue;
            };
            let shape = self.grammar.types[index].shape.clone();
            match shape {
                TypeShape::Token { atom, .. } => {
                    let kinds: Vec<_> = self.atom_kinds[atom.index()].iter().collect();
                    let content = self.roles.content_tokens.contains(&type_id);
                    for kind in kinds {
                        let terminal = Symbol::Terminal(TerminalId::new(kind.get() as u32));
                        self.lr.add_rule(Rule {
                            lhs,
                            rhs: vec![terminal],
                            value_pops: 0,
                            precedence: None,
                            action: if content {
                                RuleAction::Token { type_id }
                            } else {
                                RuleAction::FixedSequence { type_id }
                            },
                            fold: RuleFold::through(),
                            tail: TailDecision::Keep,
                        });
                    }
                }
                TypeShape::Alias { site } => {
                    let kind = ConstructKind::Construct {
                        type_id,
                        variant: None,
                    };
                    self.lower_alternative(lhs, Vec::new(), &[site], kind, None, None);
                }
                TypeShape::Sequence { sites } => {
                    let attachment = self.operand_attachments.get(&type_id).copied();
                    // The operand inside an attachment is parsed with its minimum binding
                    // power, so it lowers to that clone; the fixed tokens lower as usual.
                    let operands = attachment.map(|attachment| Operands {
                        context: OperandContext {
                            pratt: attachment.pratt,
                            excluded: &[],
                        },
                        bare_last: false,
                    });
                    let precedence = attachment.and_then(|attachment| attachment.precedence);
                    let kind = if self.roles.fixed_sequences.contains(&type_id) {
                        ConstructKind::Fixed { type_id }
                    } else if self.roles.attachment_units.contains_key(&type_id) {
                        ConstructKind::Attachment { type_id }
                    } else if let Some(index) = self.roles.presence_envelopes.get(&type_id) {
                        ConstructKind::Envelope {
                            type_id,
                            presence_index: *index,
                        }
                    } else {
                        ConstructKind::Construct {
                            type_id,
                            variant: None,
                        }
                    };
                    self.lower_alternative(lhs, Vec::new(), &sites, kind, precedence, operands);
                }
                TypeShape::Enum { alternatives } => {
                    for alternative in alternatives {
                        let kind = ConstructKind::Construct {
                            type_id,
                            variant: Some(alternative.declaration),
                        };
                        self.lower_alternative(
                            lhs,
                            Vec::new(),
                            &alternative.sites,
                            kind,
                            None,
                            None,
                        );
                    }
                }
                TypeShape::Pratt { variants } => {
                    for variant in variants {
                        let precedence = self.variant_precedence(&variant, lhs);
                        self.add_pratt_rule(lhs, type_id, &variant, precedence, None);
                    }
                }
            }
        }
    }

    /// Adds the rule of one Pratt variant to `lhs`: the implicit left operand for an infix or
    /// postfix variant, then its sites. The operands of a prefix, infix or postfix variant carry
    /// the restriction's exclusion set, and every operand but a bare last one lowers to the
    /// clone of its minimum binding power; an atom's operands keep the full language, the way
    /// gram.y's `b_expr` has `'(' a_expr ')'`.
    fn add_pratt_rule(
        &mut self,
        lhs: NonterminalId,
        type_id: TypeId,
        variant: &PrattVariant,
        precedence: Option<RulePrecedence>,
        restriction: Option<&PrattRestriction>,
    ) {
        let mut prefix = Vec::new();
        if matches!(
            variant.position,
            PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
        ) {
            prefix.push(Symbol::Nonterminal(lhs));
        }
        let operands = match variant.position {
            PrattPosition::Atom => None,
            _ => Some(Operands {
                context: OperandContext {
                    pratt: type_id,
                    excluded: restriction
                        .map_or(&[], |restriction| restriction.excluded.as_slice()),
                },
                bare_last: true,
            }),
        };
        let kind = ConstructKind::Construct {
            type_id,
            variant: Some(variant.declaration),
        };
        self.lower_alternative(lhs, prefix, &variant.sites, kind, precedence, operands);
    }

    /// Emits the rules of every clone created so far, including the clones those rules create:
    /// every variant but the excluded ones, whatever their position, and the LED variants
    /// below the minimum binding power. A clone's rules compare their precedence with the tokens of the exclusion-level
    /// nonterminal (the base type, or the clone without a minimum), which lists every operator.
    fn lower_clones(&mut self) {
        while let Some((restriction, lhs)) = self.pending_clones.pop_front() {
            let TypeShape::Pratt { variants } = self.grammar.types[restriction.type_id.index()]
                .shape
                .clone()
            else {
                unreachable!("only Pratt types are cloned");
            };
            let owner = self.restricted_nonterminal(restriction.type_id, &restriction.excluded, 0);
            for variant in variants {
                if restriction.excluded.contains(&variant.declaration) {
                    continue;
                }
                let level = match variant.position {
                    PrattPosition::Infix { lbp, .. } => Some(lbp),
                    PrattPosition::Postfix { bp } => Some(bp),
                    PrattPosition::Atom | PrattPosition::Prefix { .. } => None,
                };
                if level.is_some_and(|level| level < restriction.min_bp) {
                    continue;
                }
                // The base lowering already reported an unrepresentable pair as `RCA0402`.
                let precedence = RulePrecedence::from_position(variant.position, owner)
                    .ok()
                    .flatten();
                self.add_pratt_rule(
                    lhs,
                    restriction.type_id,
                    &variant,
                    precedence,
                    Some(&restriction),
                );
            }
        }
    }

    /// Returns the nonterminal of `type_id` restricted to `excluded` and `min_bp`: the type's
    /// own nonterminal when neither restricts, else the clone, created on first reference
    /// (its rules are emitted by [`Self::lower_clones`]).
    fn restricted_nonterminal(
        &mut self,
        type_id: TypeId,
        excluded: &[DeclId],
        min_bp: u8,
    ) -> NonterminalId {
        if excluded.is_empty() && min_bp == 0 {
            let Symbol::Nonterminal(base) = self.type_symbols[type_id.index()] else {
                unreachable!("Pratt types carry a value, so they are never inlined");
            };
            return base;
        }
        let restriction = PrattRestriction {
            type_id,
            excluded: excluded.to_vec(),
            min_bp,
        };
        if let Some(clone) = self.clones.get(&restriction) {
            return *clone;
        }
        let name = self.unique_name(
            self.restriction_name(&self.grammar.types[type_id.index()].name, &restriction),
        );
        let clone = self.lr.add_nonterminal(
            name,
            NonterminalOrigin::Restriction {
                type_id,
                excluded: excluded.to_vec(),
                min_bp,
            },
            true,
        );
        self.clones.insert(restriction.clone(), clone);
        self.pending_clones.push_back((restriction, clone));
        clone
    }

    /// Returns the attachment sequence `unit` whose operand carries the context's exclusion
    /// set: the unit's own nonterminal when the set is empty, else a clone created with its
    /// rule on first reference. The rule inherits the variant's precedence, owned by the
    /// exclusion-level nonterminal whose tokens it is compared with.
    fn attachment_clone(&mut self, unit: TypeId, context: OperandContext<'_>) -> NonterminalId {
        let Symbol::Nonterminal(original) = self.type_symbols[unit.index()] else {
            unreachable!("attachment units carry a value, so they are never inlined");
        };
        if context.excluded.is_empty() {
            return original;
        }
        let key = (unit, context.excluded.to_vec());
        if let Some(clone) = self.attachment_clones.get(&key) {
            return *clone;
        }
        let TypeShape::Sequence { sites } = self.grammar.types[unit.index()].shape.clone() else {
            unreachable!("attachment units are sequences");
        };
        let restriction = PrattRestriction {
            type_id: context.pratt,
            excluded: context.excluded.to_vec(),
            min_bp: 0,
        };
        let base = self.lr.nonterminal(original).name.clone();
        let name = self.unique_name(self.restriction_name(&base, &restriction));
        let clone = self.lr.add_nonterminal(
            name,
            NonterminalOrigin::Restriction {
                type_id: unit,
                excluded: context.excluded.to_vec(),
                min_bp: 0,
            },
            true,
        );
        self.attachment_clones.insert(key, clone);
        let owner = self.restricted_nonterminal(context.pratt, context.excluded, 0);
        let precedence = self
            .operand_attachments
            .get(&unit)
            .and_then(|attachment| attachment.precedence)
            .map(|precedence| RulePrecedence {
                owner,
                ..precedence
            });
        let operands = Operands {
            context,
            bare_last: false,
        };
        self.lower_alternative(
            clone,
            Vec::new(),
            &sites,
            ConstructKind::Attachment { type_id: unit },
            precedence,
            Some(operands),
        );
        clone
    }

    /// Names a clone `Type[excl A,B, min N]` after the excluded variants' authored names and
    /// the minimum binding power, each part present only when it restricts.
    fn restriction_name(&self, base: &str, restriction: &PrattRestriction) -> String {
        let owner = format!("{}::", self.grammar.types[restriction.type_id.index()].name);
        let mut parts = Vec::new();
        if !restriction.excluded.is_empty() {
            let excluded: Vec<&str> = restriction
                .excluded
                .iter()
                .map(|declaration| {
                    let name = self.grammar.declarations[declaration.index()].name.as_str();
                    name.strip_prefix(&owner).unwrap_or(name)
                })
                .collect();
            parts.push(format!("excl {}", excluded.join(",")));
        }
        if restriction.min_bp > 0 {
            parts.push(format!("min {}", restriction.min_bp));
        }
        format!("{base}[{}]", parts.join(", "))
    }

    /// Derives a Pratt variant's rule precedence, reporting `RCA0402` naming the variant when
    /// its binding powers have no precedence equivalent; the rule then carries none.
    fn variant_precedence(
        &mut self,
        variant: &PrattVariant,
        owner: NonterminalId,
    ) -> Option<RulePrecedence> {
        match RulePrecedence::from_position(variant.position, owner) {
            Ok(precedence) => precedence,
            Err(UnrepresentableBindingPowers { lbp, rbp }) => {
                self.diagnostics.push(LrDiagnostic {
                    code: NO_PRECEDENCE_EQUIVALENT,
                    message: format!(
                        "Pratt variant `{}` has lbp = {lbp} and rbp = {rbp}, which no rule \
                         precedence reproduces; the Recursa parser needs rbp = lbp + 1 \
                         (left-associative), rbp = lbp or rbp = lbp - 1 (right-associative)",
                        self.grammar.declarations[variant.declaration.index()].name
                    ),
                    path: String::new(),
                    items: Vec::new(),
                    actions: Vec::new(),
                    notes: Vec::new(),
                });
                None
            }
        }
    }

    /// Lowers one alternative of a type: its sites become site lowerings, the budget decides
    /// which choices stay inlined, and the cartesian product of the sites' alternatives becomes
    /// the rules, each after `prefix`.
    fn lower_alternative(
        &mut self,
        lhs: NonterminalId,
        prefix: Vec<Symbol>,
        sites: &[SiteId],
        kind: ConstructKind,
        precedence: Option<RulePrecedence>,
        operands: Option<Operands<'_>>,
    ) {
        let last = sites.len().saturating_sub(1);
        let mut lowerings: Vec<SiteLowering> = sites
            .iter()
            .enumerate()
            .map(|(position, site)| {
                let context = operands.map(|operands| operands.context);
                let bare = operands.is_some_and(|operands| operands.bare_last) && position == last;
                self.site_lowering(*site, context, bare)
            })
            .collect();
        loop {
            let product: usize = lowerings
                .iter()
                .map(SiteLowering::alternative_count)
                .product();
            if product <= INLINE_BUDGET {
                break;
            }
            if lowerings.iter_mut().rev().any(SiteLowering::demote_valued) {
                continue;
            }
            if lowerings.iter_mut().rev().any(SiteLowering::demote_fixed) {
                continue;
            }
            break;
        }
        // A final repetition lowers to a rejecting tail action until its LR reduction applies.
        let tail = if matches!(
            lowerings.last(),
            Some(SiteLowering::OneOrMore { .. } | SiteLowering::ZeroOrMore { .. })
        ) {
            TailDecision::Reject
        } else {
            TailDecision::Keep
        };
        // The semantic site of an attachment unit passes its operand's occurrence through: the
        // unit's node is the operand's own occurrence with the fixed tokens as siblings.
        let pass_through = match kind {
            ConstructKind::Attachment { type_id } => {
                self.roles.attachment_units.get(&type_id).copied()
            }
            _ => None,
        };
        // A presence envelope's node is the outer group the presence renderer builds around the
        // attached tokens, so its presence site folds to the inner group alone.
        let envelope_presence = match kind {
            ConstructKind::Envelope { presence_index, .. } => Some(presence_index),
            _ => None,
        };
        let alternatives: Vec<Vec<Alternative>> = lowerings
            .iter()
            .map(|lowering| self.alternatives(lowering))
            .collect();
        let mut selection = vec![0_usize; alternatives.len()];
        loop {
            let mut rhs = prefix.clone();
            let mut forms = Vec::with_capacity(alternatives.len());
            let mut fold_sites = Vec::with_capacity(alternatives.len() + prefix.len());
            // The implicit left operand of a Pratt rule is grouped like a direct child.
            fold_sites.extend((0..prefix.len()).map(|position| FoldSite {
                start: position as u32,
                len: 1,
                kind: SiteKind::Group,
                policy: FoldPolicy::Inherited,
            }));
            for (position, ((site_id, site), choice)) in
                sites.iter().zip(&alternatives).zip(&selection).enumerate()
            {
                let alternative = &site[*choice];
                let site_kind = if pass_through == Some(*site_id) {
                    SiteKind::Through
                } else if envelope_presence == Some(position)
                    && alternative.kind == SiteKind::Presence
                {
                    if alternative.symbols.is_empty() {
                        SiteKind::Absent
                    } else {
                        SiteKind::Group
                    }
                } else {
                    alternative.kind
                };
                forms.push(SiteForm {
                    start: rhs.len() as u32,
                    len: alternative.symbols.len() as u32,
                    present: alternative.present,
                    inlined: alternative.inlined,
                });
                fold_sites.push(FoldSite {
                    start: rhs.len() as u32,
                    len: alternative.symbols.len() as u32,
                    kind: site_kind,
                    policy: alternative.policy,
                });
                rhs.extend_from_slice(&alternative.symbols);
            }
            let action = match kind {
                ConstructKind::Construct { type_id, variant } => RuleAction::Construct {
                    type_id,
                    variant,
                    sites: forms,
                },
                ConstructKind::Fixed { type_id } => RuleAction::FixedSequence { type_id },
                ConstructKind::Attachment { type_id } => RuleAction::Attachment { type_id },
                ConstructKind::Envelope {
                    type_id,
                    presence_index,
                } => {
                    // The envelope's reduce pushes its `bool` as a literal, so the presence
                    // choice must be inlined; it is the envelope's only choice and never meets
                    // the budget.
                    assert!(
                        forms[presence_index].inlined,
                        "a presence envelope inlines its presence choice"
                    );
                    RuleAction::Envelope {
                        type_id,
                        present: forms[presence_index].present,
                    }
                }
            };
            let value_pops = self.value_pops(&rhs);
            self.lr.add_rule(Rule {
                lhs,
                rhs,
                value_pops,
                precedence,
                action,
                fold: RuleFold::node(fold_sites),
                tail,
            });
            // Advance the odometer over the sites' alternatives, last site fastest.
            let mut position = selection.len();
            loop {
                if position == 0 {
                    return;
                }
                position -= 1;
                selection[position] += 1;
                if selection[position] < alternatives[position].len() {
                    break;
                }
                selection[position] = 0;
            }
        }
    }

    /// Counts the value-carrying symbols of a right-hand side.
    fn value_pops(&self, rhs: &[Symbol]) -> u32 {
        rhs.iter()
            .filter(|symbol| self.carries_value(**symbol))
            .count() as u32
    }

    /// Reports whether a symbol pushes a value when reduced or shifted.
    fn carries_value(&self, symbol: Symbol) -> bool {
        match symbol {
            Symbol::Terminal(_) => false,
            Symbol::Nonterminal(id) => self.lr.nonterminal(id).carries_value,
        }
    }

    /// Reports whether a choice whose content is `target` is inlined: when the content can
    /// begin with a keyword an admission set also accepts as content.
    fn inline_choice(&self, target: TypeId) -> bool {
        self.first[target.index()]
            .intersects(&self.admitted_keywords)
            .expect("kind sets of one grammar share a width")
    }

    /// Resolves the type whose value a symbol of `type_id` carries: an attachment unit passes
    /// its semantic child's value through.
    fn value_type(&self, type_id: TypeId) -> TypeId {
        match self.roles.attachment_units.get(&type_id) {
            Some(child) => self.value_type(self.grammar.sites[child.index()].target),
            None => type_id,
        }
    }

    /// The symbols of a fixed-only sequence type, one per fixed-token site.
    fn fixed_sequence_symbols(&self, type_id: TypeId) -> Vec<Symbol> {
        let TypeShape::Sequence { sites } = &self.grammar.types[type_id.index()].shape else {
            unreachable!("fixed-only sequences are sequences")
        };
        sites
            .iter()
            .map(|site| self.type_symbols[self.grammar.sites[site.index()].target.index()])
            .collect()
    }

    /// Lowers a site's target under the Pratt rule it belongs to, if any. The site's own
    /// exclusion set wins, else the rule's applies, when the site is a Pratt operand of the
    /// type, directly or through an attachment sequence. A direct operand also takes its
    /// minimum binding power unless it is the rule's bare last symbol (`last`), where the
    /// rule's own precedence decides; any other target lowers as usual.
    fn site_target_symbol(
        &mut self,
        site_id: SiteId,
        context: Option<OperandContext<'_>>,
        last: bool,
    ) -> Symbol {
        let site = &self.grammar.sites[site_id.index()];
        let (target, site_min_bp) = (site.target, site.min_bp);
        let own = self.roles.pratt_exclusions.get(&site_id).cloned();
        let operand = match (&own, context) {
            (Some(own), _) => Some(OperandContext {
                pratt: own.type_id,
                excluded: own.excluded.as_slice(),
            }),
            (None, Some(context)) => Some(context),
            (None, None) => None,
        };
        match operand {
            Some(operand) if target == operand.pratt => {
                let min_bp = if context.is_some() && !last {
                    site_min_bp
                } else {
                    0
                };
                Symbol::Nonterminal(self.restricted_nonterminal(
                    operand.pratt,
                    operand.excluded,
                    min_bp,
                ))
            }
            Some(operand) if self.wraps_operand_of(target, operand.pratt) => {
                Symbol::Nonterminal(self.attachment_clone(target, operand))
            }
            _ => self.type_symbols[target.index()],
        }
    }

    /// Reports whether `unit` is an attachment sequence whose semantic site targets `pratt`.
    fn wraps_operand_of(&self, unit: TypeId, pratt: TypeId) -> bool {
        if !self.roles.attachment_units.contains_key(&unit) {
            return false;
        }
        match &self.grammar.types[unit.index()].shape {
            TypeShape::Sequence { sites } => sites
                .iter()
                .any(|site| self.grammar.sites[site.index()].target == pratt),
            _ => false,
        }
    }

    /// Lowers one site to its site lowering, creating the list cores it needs. The site's
    /// target lowers under the Pratt rule it belongs to, if any (see
    /// [`Self::site_target_symbol`]).
    fn site_lowering(
        &mut self,
        site_id: SiteId,
        context: Option<OperandContext<'_>>,
        last: bool,
    ) -> SiteLowering {
        let site = self.grammar.sites[site_id.index()].clone();
        let target = site.target;
        let symbol = self.site_target_symbol(site_id, context, last);
        let presence = self.roles.presence_sites.contains(&site_id);
        let (_, shape) = strip_boxes(&site.container);
        match shape {
            ContainerShape::Direct | ContainerShape::Boxed(_) => SiteLowering::Required {
                symbol,
                kind: self.required_site_kind(target),
            },
            ContainerShape::Optional(inner) => {
                if presence || self.roles.fixed_sequences.contains(&target) {
                    return SiteLowering::FixedOptional {
                        tokens: self.fixed_sequence_symbols(target),
                        presence,
                        inline: true,
                    };
                }
                let (inner_boxes, inner_shape) = strip_boxes(inner);
                // The attachment around an optional Pratt operand is always inlined: the
                // variant's own reduce competes with the shift of the attachment's token, and
                // an epsilon reduce of a helper has no precedence to settle that with.
                let required_inline = self.separated_tail_sites.contains(&site_id);
                let inline = self.inline_choice(target)
                    || self.operand_attachments.contains_key(&target)
                    || required_inline;
                match inner_shape {
                    ContainerShape::Direct | ContainerShape::Boxed(_) => {
                        SiteLowering::ValuedOptional {
                            inner: OptionalInner::Element(symbol),
                            content: OptionalContent::Element(ElementValue {
                                type_id: self.value_type(target),
                                boxes: inner_boxes,
                            }),
                            inline,
                            required_inline,
                        }
                    }
                    ContainerShape::Repeat {
                        element,
                        cardinality: RepetitionCardinality::OneOrMore,
                        separator,
                    } => {
                        let (element_boxes, _) = strip_boxes(element);
                        let element = ElementValue {
                            type_id: self.value_type(target),
                            boxes: element_boxes,
                        };
                        let separator_symbol =
                            separator.map(|separator| self.type_symbols[separator.type_id.index()]);
                        let list1 = self.non_empty_list(symbol, separator_symbol, element);
                        SiteLowering::ValuedOptional {
                            inner: OptionalInner::List(InnerList {
                                list1,
                                trailing: separator
                                    .filter(|separator| separator.optional_trailing)
                                    .and(separator_symbol),
                                trailing_inline: true,
                            }),
                            content: OptionalContent::NonEmptyList {
                                element,
                                boxes: inner_boxes,
                            },
                            inline,
                            required_inline,
                        }
                    }
                    ContainerShape::Optional(_)
                    | ContainerShape::Repeat {
                        cardinality: RepetitionCardinality::ZeroOrMore,
                        ..
                    } => unreachable!(
                        "the container machine accepts no optional of an optional or of a zero-or-more list"
                    ),
                }
            }
            ContainerShape::Repeat {
                element,
                cardinality,
                separator,
            } => {
                let (element_boxes, _) = strip_boxes(element);
                let element = ElementValue {
                    type_id: self.value_type(target),
                    boxes: element_boxes,
                };
                let separator_symbol =
                    separator.map(|separator| self.type_symbols[separator.type_id.index()]);
                let trailing = separator
                    .filter(|separator| separator.optional_trailing)
                    .and(separator_symbol);
                let list1 = self.non_empty_list(symbol, separator_symbol, element);
                match cardinality {
                    RepetitionCardinality::OneOrMore => SiteLowering::OneOrMore {
                        list1,
                        trailing,
                        trailing_inline: true,
                    },
                    RepetitionCardinality::ZeroOrMore => SiteLowering::ZeroOrMore {
                        list1,
                        element,
                        trailing,
                        inline: self.inline_choice(target),
                        trailing_inline: true,
                    },
                }
            }
        }
    }

    /// The present alternatives of a one-or-more list: the core alone, the core with its
    /// trailing separator when that choice is inlined, or the trailing helper.
    fn list_alternatives(
        &mut self,
        list1: NonterminalId,
        trailing: Option<Symbol>,
        trailing_inline: bool,
    ) -> Vec<Vec<Symbol>> {
        let core = Symbol::Nonterminal(list1);
        match trailing {
            None => vec![vec![core]],
            Some(separator) if trailing_inline => vec![vec![core], vec![core, separator]],
            Some(separator) => vec![vec![Symbol::Nonterminal(
                self.trailing_list(list1, separator),
            )]],
        }
    }

    /// Expands a site lowering to the alternatives it contributes, creating helpers for the
    /// choices that are not inlined.
    fn alternatives(&mut self, lowering: &SiteLowering) -> Vec<Alternative> {
        let present =
            |symbols: Vec<Symbol>, inlined: bool, kind: SiteKind, policy: FoldPolicy| Alternative {
                symbols,
                present: true,
                inlined,
                kind,
                policy,
            };
        let absent = |kind: SiteKind| Alternative {
            symbols: Vec::new(),
            present: false,
            inlined: true,
            kind,
            policy: FoldPolicy::Inherited,
        };
        let inherited = FoldPolicy::Inherited;
        match lowering.clone() {
            SiteLowering::Required { symbol, kind } => {
                vec![present(vec![symbol], false, kind, inherited)]
            }
            SiteLowering::FixedOptional {
                tokens,
                presence,
                inline,
            } => {
                let content = if presence {
                    OptionalContent::Presence
                } else {
                    OptionalContent::Erased
                };
                // An erased group is captured whatever the policy; a presence takes the
                // field's. Through a helper the presence keeps its outer group here, while
                // the erased group's helper builds the whole occurrence.
                if inline {
                    if presence {
                        vec![
                            absent(SiteKind::Presence),
                            present(tokens, true, SiteKind::Presence, inherited),
                        ]
                    } else {
                        vec![
                            absent(SiteKind::Absent),
                            present(tokens, true, SiteKind::Group, FoldPolicy::Mandatory),
                        ]
                    }
                } else {
                    let helper = self.optional_helper(vec![tokens], content);
                    let kind = if presence {
                        SiteKind::Group
                    } else {
                        SiteKind::Through
                    };
                    vec![present(
                        vec![Symbol::Nonterminal(helper)],
                        false,
                        kind,
                        inherited,
                    )]
                }
            }
            SiteLowering::ValuedOptional {
                inner,
                content,
                inline,
                ..
            } => {
                let (inner_alternatives, kind, absent_kind) = match inner {
                    OptionalInner::Element(symbol) => {
                        (vec![vec![symbol]], SiteKind::Group, SiteKind::Absent)
                    }
                    OptionalInner::List(list) => (
                        self.list_alternatives(list.list1, list.trailing, list.trailing_inline),
                        SiteKind::Repetition,
                        SiteKind::Repetition,
                    ),
                };
                if inline {
                    let mut alternatives = vec![absent(absent_kind)];
                    alternatives.extend(
                        inner_alternatives
                            .into_iter()
                            .map(|symbols| present(symbols, true, kind, inherited)),
                    );
                    alternatives
                } else {
                    let helper = self.optional_helper(inner_alternatives, content);
                    vec![present(
                        vec![Symbol::Nonterminal(helper)],
                        false,
                        SiteKind::Through,
                        inherited,
                    )]
                }
            }
            SiteLowering::OneOrMore {
                list1,
                trailing,
                trailing_inline,
            } => self
                .list_alternatives(list1, trailing, trailing_inline)
                .into_iter()
                .map(|symbols| present(symbols, false, SiteKind::Repetition, inherited))
                .collect(),
            SiteLowering::ZeroOrMore {
                list1,
                element,
                trailing,
                inline,
                trailing_inline,
            } => {
                if inline {
                    let mut alternatives = vec![absent(SiteKind::Repetition)];
                    alternatives.extend(
                        self.list_alternatives(list1, trailing, trailing_inline)
                            .into_iter()
                            .map(|symbols| present(symbols, true, SiteKind::Repetition, inherited)),
                    );
                    alternatives
                } else {
                    let helper = self.zero_or_more_list(list1, trailing, element);
                    vec![present(
                        vec![Symbol::Nonterminal(helper)],
                        false,
                        SiteKind::Repetition,
                        inherited,
                    )]
                }
            }
        }
    }

    /// The fold of a required site: a fixed token's leaf, a presence envelope's own outer
    /// group (the presence renderer builds it, with nothing around it), or a group around the
    /// occurrence of any other target, as `parse_direct_child` wraps a child.
    fn required_site_kind(&self, target: TypeId) -> SiteKind {
        if self.roles.presence_envelopes.contains_key(&target) {
            return SiteKind::Through;
        }
        match &self.grammar.types[target.index()].shape {
            TypeShape::Token { .. } if !self.roles.content_tokens.contains(&target) => {
                SiteKind::Leaf
            }
            _ => SiteKind::Group,
        }
    }

    /// Decides every rule's tail (see [`TailDecision`]): Pratt rules clear the flag, an enum
    /// alternative that derives exactly one token sets it, and a rule that ends in a
    /// repetition already set it when it was lowered.
    fn decide_tails(&mut self) {
        let bounds = token_bounds(&self.lr);
        let pratt_lhs: Vec<bool> = (0..self.lr.nonterminal_count())
            .map(|index| {
                let type_id = match self.lr.nonterminal(NonterminalId::new(index as u32)).origin {
                    NonterminalOrigin::Type(type_id)
                    | NonterminalOrigin::Restriction { type_id, .. } => type_id,
                    _ => return false,
                };
                matches!(
                    self.grammar.types[type_id.index()].shape,
                    TypeShape::Pratt { .. }
                )
            })
            .collect();
        let grammar = self.grammar;
        for rule in self.lr.rules_mut() {
            if pratt_lhs[rule.lhs.index()] {
                rule.tail = TailDecision::Clear;
                continue;
            }
            let RuleAction::Construct {
                type_id,
                variant: Some(_),
                ..
            } = &rule.action
            else {
                continue;
            };
            if !matches!(grammar.types[type_id.index()].shape, TypeShape::Enum { .. }) {
                continue;
            }
            let (min, max) = rule.rhs.iter().fold((0_u8, 0_u8), |(min, max), symbol| {
                let (symbol_min, symbol_max) = match symbol {
                    Symbol::Terminal(_) => (1, 1),
                    Symbol::Nonterminal(id) => bounds[id.index()],
                };
                ((min + symbol_min).min(2), (max + symbol_max).min(2))
            });
            if min == 1 && max == 1 {
                rule.tail = TailDecision::Reject;
            }
        }
    }

    /// Returns the shared `list1 -> e | list1 [sep] e` core, creating it on first use.
    fn non_empty_list(
        &mut self,
        element: Symbol,
        separator: Option<Symbol>,
        value: ElementValue,
    ) -> NonterminalId {
        let key = HelperKey::NonEmptyList {
            element,
            boxes: value.boxes,
            separator,
        };
        if let Some(id) = self.helpers.get(&key) {
            return *id;
        }
        let mut name = format!(
            "list1({}{}{}",
            "Box<".repeat(value.boxes as usize),
            self.lr.symbol_name(element),
            ">".repeat(value.boxes as usize)
        );
        if let Some(separator) = separator {
            name.push_str(", sep=");
            name.push_str(self.lr.symbol_name(separator));
        }
        name.push(')');
        let name = self.unique_name(name);
        let id = self
            .lr
            .add_nonterminal(name, NonterminalOrigin::Helper, true);
        self.helpers.insert(key, id);
        let element_pops = self.value_pops(&[element]);
        self.lr.add_rule(Rule {
            lhs: id,
            rhs: vec![element],
            value_pops: element_pops,
            precedence: None,
            action: RuleAction::ListFirst { element: value },
            fold: RuleFold::plain(FoldKind::ListStart, FoldPolicy::Inherited),
            tail: TailDecision::Keep,
        });
        let mut rhs = vec![Symbol::Nonterminal(id)];
        rhs.extend(separator);
        rhs.push(element);
        self.lr.add_rule(Rule {
            lhs: id,
            rhs,
            value_pops: 1 + element_pops,
            precedence: None,
            action: RuleAction::ListPush { element: value },
            fold: RuleFold::plain(FoldKind::ListExtend, FoldPolicy::Inherited),
            tail: TailDecision::Keep,
        });
        id
    }

    /// Returns the shared `list1t -> list1 | list1 sep` helper, creating it on first use.
    fn trailing_list(&mut self, list1: NonterminalId, separator: Symbol) -> NonterminalId {
        let key = HelperKey::Trailing { list1, separator };
        if let Some(id) = self.helpers.get(&key) {
            return *id;
        }
        let name = self.unique_name(format!(
            "list1t{}",
            self.lr.nonterminal(list1).name.trim_start_matches("list1")
        ));
        let id = self
            .lr
            .add_nonterminal(name, NonterminalOrigin::Helper, true);
        self.helpers.insert(key, id);
        for rhs in [
            vec![Symbol::Nonterminal(list1)],
            vec![Symbol::Nonterminal(list1), separator],
        ] {
            self.lr.add_rule(Rule {
                lhs: id,
                rhs,
                value_pops: 1,
                precedence: None,
                action: RuleAction::ListTrailing,
                fold: RuleFold::plain(FoldKind::ListThrough, FoldPolicy::Inherited),
                tail: TailDecision::Keep,
            });
        }
        id
    }

    /// Returns the shared `list -> %empty | list1 [sep]` helper, creating it on first use.
    fn zero_or_more_list(
        &mut self,
        list1: NonterminalId,
        trailing: Option<Symbol>,
        element: ElementValue,
    ) -> NonterminalId {
        let key = HelperKey::ZeroOrMore { list1, trailing };
        if let Some(id) = self.helpers.get(&key) {
            return *id;
        }
        let name = self.unique_name(format!(
            "list{}",
            self.lr.nonterminal(list1).name.trim_start_matches("list1")
        ));
        let id = self
            .lr
            .add_nonterminal(name, NonterminalOrigin::Helper, true);
        self.helpers.insert(key, id);
        self.lr.add_rule(Rule {
            lhs: id,
            rhs: Vec::new(),
            value_pops: 0,
            precedence: None,
            action: RuleAction::ListEmpty { element },
            fold: RuleFold::plain(FoldKind::ListThrough, FoldPolicy::Inherited),
            tail: TailDecision::Keep,
        });
        let mut present = vec![vec![Symbol::Nonterminal(list1)]];
        if let Some(separator) = trailing {
            present.push(vec![Symbol::Nonterminal(list1), separator]);
        }
        for rhs in present {
            self.lr.add_rule(Rule {
                lhs: id,
                rhs,
                value_pops: 1,
                precedence: None,
                action: RuleAction::ListFromNonEmpty { element },
                fold: RuleFold::plain(FoldKind::ListThrough, FoldPolicy::Inherited),
                tail: TailDecision::Keep,
            });
        }
        id
    }

    /// Returns the shared nullable helper around the present alternatives, creating it on first
    /// use: `opt(...)` for erased and valued content, `presence(...)` for a `bool`.
    fn optional_helper(
        &mut self,
        present: Vec<Vec<Symbol>>,
        content: OptionalContent,
    ) -> NonterminalId {
        let key = HelperKey::Optional {
            present: present.clone(),
            content,
        };
        if let Some(id) = self.helpers.get(&key) {
            return *id;
        }
        let described: Vec<&str> = present[0]
            .iter()
            .map(|symbol| self.lr.symbol_name(*symbol))
            .collect();
        let prefix = if content == OptionalContent::Presence {
            "presence"
        } else {
            "opt"
        };
        let name = self.unique_name(format!("{prefix}({})", described.join(" ")));
        let carries_value = content != OptionalContent::Erased;
        let id = self
            .lr
            .add_nonterminal(name, NonterminalOrigin::Helper, carries_value);
        self.helpers.insert(key, id);
        // The helper folds what its inlined form would have: a group (mandatory for an
        // erased group), a repetition group with or without items, or nothing.
        let (absent_fold, present_fold) = match content {
            OptionalContent::Erased => (
                RuleFold::plain(FoldKind::Absent, FoldPolicy::Inherited),
                RuleFold::plain(FoldKind::Group, FoldPolicy::Mandatory),
            ),
            OptionalContent::Presence | OptionalContent::Element(_) => (
                RuleFold::plain(FoldKind::Absent, FoldPolicy::Inherited),
                RuleFold::plain(FoldKind::Group, FoldPolicy::Inherited),
            ),
            OptionalContent::NonEmptyList { .. } => (
                RuleFold::plain(FoldKind::Repetition, FoldPolicy::Inherited),
                RuleFold::plain(FoldKind::Repetition, FoldPolicy::Inherited),
            ),
        };
        self.lr.add_rule(Rule {
            lhs: id,
            rhs: Vec::new(),
            value_pops: 0,
            precedence: None,
            action: RuleAction::OptionalAbsent { content },
            fold: absent_fold,
            tail: TailDecision::Keep,
        });
        for rhs in present {
            let value_pops = self.value_pops(&rhs);
            self.lr.add_rule(Rule {
                lhs: id,
                rhs,
                value_pops,
                precedence: None,
                action: RuleAction::OptionalPresent { content },
                fold: present_fold.clone(),
                tail: TailDecision::Keep,
            });
        }
        id
    }

    /// Returns a nonterminal name not yet taken.
    fn unique_name(&mut self, name: String) -> String {
        if self.names.insert(name.clone()) {
            return name;
        }
        let mut counter = 2;
        loop {
            let candidate = format!("{name}#{counter}");
            if self.names.insert(candidate.clone()) {
                return candidate;
            }
            counter += 1;
        }
    }

    /// Adds a start rule for every entry type that has a nonterminal.
    fn lower_starts(&mut self) {
        for type_id in &self.roles.start_types {
            if let Symbol::Nonterminal(target) = self.type_symbols[type_id.index()] {
                self.lr.add_start(*type_id, target);
            }
        }
    }
}

/// The least and greatest number of tokens each nonterminal derives, each saturated at two, so
/// that "exactly one token" is decidable: a fixpoint over the rules from terminals at one.
fn token_bounds(grammar: &LrGrammar) -> Vec<(u8, u8)> {
    let mut bounds = vec![(2_u8, 0_u8); grammar.nonterminal_count()];
    loop {
        let mut changed = false;
        for rule in grammar.rules() {
            let (min, max) = rule.rhs.iter().fold((0_u8, 0_u8), |(min, max), symbol| {
                let (symbol_min, symbol_max) = match symbol {
                    Symbol::Terminal(_) => (1, 1),
                    Symbol::Nonterminal(id) => bounds[id.index()],
                };
                ((min + symbol_min).min(2), (max + symbol_max).min(2))
            });
            let entry = &mut bounds[rule.lhs.index()];
            let next = (entry.0.min(min), entry.1.max(max));
            if next != *entry {
                *entry = next;
                changed = true;
            }
        }
        if !changed {
            return bounds;
        }
    }
}
