#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! The LR grammar that the table-driven lowering produces and the automaton consumes.
//!
//! An [`LrGrammar`] is a plain context-free grammar over dense symbol identities: terminals are
//! token kinds plus one synthetic end marker, nonterminals are grammar types plus the helper
//! nonterminals the lowering invents, and rules carry the facts the later tickets need (value
//! pops for the reduce action, binding powers for precedence). It is source-independent and
//! knows nothing about how a rule came to exist beyond its [`RuleAction`].

use std::collections::BTreeMap;
use std::fmt;

use crate::{DeclId, TypeId};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense terminal identity: token kinds keep their [`KindId`](crate::KindId) index and the end
/// marker is the last terminal.
pub struct TerminalId(u32);

impl TerminalId {
    /// Creates a terminal identity from its dense index.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the identity as a collection index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense nonterminal identity in declaration order.
pub struct NonterminalId(u32);

impl NonterminalId {
    /// Creates a nonterminal identity from its dense index.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the identity as a collection index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense rule identity in declaration order; the table encoding relies on this order.
pub struct RuleId(u32);

impl RuleId {
    /// Creates a rule identity from its dense index.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the identity as a collection index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One grammar symbol on the right-hand side of a rule.
pub enum Symbol {
    /// A token kind or the end marker.
    Terminal(TerminalId),
    /// A grammar type or helper nonterminal.
    Nonterminal(NonterminalId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// Associativity a binding-power pair lowers to; the precedence resolver consults it on a tie.
pub enum Associativity {
    /// `rbp = lbp + 1`: the left operand binds first.
    Left,
    /// `rbp = lbp` or `rbp = lbp - 1`: the right operand binds first.
    Right,
    /// A `nonassoc` level of the `precedence` declaration: a tie is a syntax error, as in
    /// bison.
    NonAssociative,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// One terminal's entry in the `precedence` declaration: its level's binding power and
/// associativity. Levels are declared with strictly ascending binding powers, so the binding
/// power orders the levels and identifies one.
pub struct DeclaredPrecedence {
    /// The level's binding power: it orders the levels and identifies one.
    pub binding_power: u8,
    /// The associativity of the level: `left`, `right` or `nonassoc`.
    pub associativity: Associativity,
}

impl DeclaredPrecedence {
    /// The rule precedence a rule takes from this level.
    pub const fn rule_precedence(self) -> RulePrecedence {
        RulePrecedence {
            level: self.binding_power,
            associativity: self.associativity,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The precedence of a lowered rule, for shift/reduce resolution: the declared level the rule
/// takes from its last declared terminal or from its `#[parse(prec = NAME)]` override.
pub struct RulePrecedence {
    /// The declared level, which orders it against every other level and every token's.
    pub level: u8,
    /// The level's associativity, which settles a tie as bison's `%left`, `%right` and
    /// `%nonassoc` do.
    pub associativity: Associativity,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// How one grammar site of a construction rule appears on the rule's right-hand side.
///
/// The lowering expands optional sites into alternatives (`docs/table-driven-parsing.md`,
/// section 5), so a rule may hold fewer or more symbols than its type has sites. Each form
/// names the slice of the right-hand side that belongs to one site, in site order, so that a
/// reduce action can walk the node's fields and the rule's symbols together.
pub struct SiteForm {
    /// Index of the site's first symbol on the right-hand side.
    pub start: u32,
    /// Number of right-hand-side symbols the site occupies; zero for an absent choice.
    pub len: u32,
    /// Whether the site's content is present in this alternative.
    pub present: bool,
    /// Whether the choice was inlined: an absent inlined optional has no symbol and a present
    /// one shows its inner symbols, while a helper site shows the one helper symbol whose value
    /// is the complete field value.
    pub inlined: bool,
}

impl SiteForm {
    /// The form of a required site or a helper site: one symbol carrying the whole value.
    pub const fn required(start: u32) -> Self {
        Self {
            start,
            len: 1,
            present: true,
            inlined: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The capture policy a provenance fold applies to an occurrence, as the runtime's
/// `lr::provenance::FoldPolicy` decodes it (the codes are the protocol).
///
/// The lowering emits `Inherited` for ordinary sites and `Mandatory` for erased optional
/// fixed-token groups. A grammar with an authored override is rejected (`RCA0406`).
pub enum FoldPolicy {
    /// The parse context's policy.
    Inherited,
    /// `CapturePolicy::Off`.
    Off,
    /// `CapturePolicy::Optional`.
    Optional,
    /// `CapturePolicy::Mandatory`.
    Mandatory,
}

impl FoldPolicy {
    /// The packed code.
    pub const fn code(self) -> u32 {
        match self {
            Self::Inherited => 0,
            Self::Off => 1,
            Self::Optional => 2,
            Self::Mandatory => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// What the reduce of a rule folds its popped occurrence entries into; mirrors the runtime's
/// `lr::provenance::FoldKind` (the codes are the protocol).
pub enum FoldKind {
    /// The one popped entry stays: a content-token or fixed-token rule.
    Through,
    /// An absent optional helper: no occurrence.
    Absent,
    /// A grammar type, attachment unit, fixed-only sequence or presence envelope: one node with
    /// one child per site.
    Node,
    /// A present optional helper: one group over every popped entry.
    Group,
    /// A one-or-more list inside an optional helper: one repetition group over the entries.
    Repetition,
    /// `list1 -> e`.
    ListStart,
    /// `list1 -> list1 [sep] e`.
    ListExtend,
    /// `list1t -> list1 [sep]`, `list -> list1 [sep]` and `list -> %empty`.
    ListThrough,
}

impl FoldKind {
    /// The packed code.
    pub const fn code(self) -> u32 {
        match self {
            Self::Through => 0,
            Self::Absent => 1,
            Self::Node => 2,
            Self::Group => 3,
            Self::Repetition => 4,
            Self::ListStart => 5,
            Self::ListExtend => 6,
            Self::ListThrough => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The occurrence one site of a node fold builds; mirrors the runtime's
/// `lr::provenance::SiteKind` (the codes are the protocol). Each kind names the occurrence shape
/// produced for that element.
pub enum SiteKind {
    /// A fixed-token leaf.
    Leaf,
    /// A semantic element, or an inlined present optional or erased group around the entries.
    Group,
    /// A helper's or attachment operand's entry unchanged.
    Through,
    /// An absent inlined optional or erased group.
    Absent,
    /// An inlined presence: the outer group over the inner group of the sequence tokens.
    Presence,
    /// An inlined repetition, present or absent, over the list entry and any trailing separator.
    Repetition,
    /// An inlined cardinality-one attachment unit (`docs/declared-precedence.md`, "Attachment
    /// units inline"): the group this site builds around the node the removed private
    /// attachment nonterminal's own `Node` fold built, over the site's own [`FoldSite::nested`]
    /// sites, so inlining the rule leaves the field's occurrence unchanged.
    Attachment,
}

impl SiteKind {
    /// The packed code.
    pub const fn code(self) -> u32 {
        match self {
            Self::Leaf => 0,
            Self::Group => 1,
            Self::Through => 2,
            Self::Absent => 3,
            Self::Presence => 4,
            Self::Repetition => 5,
            Self::Attachment => 6,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// One site of a node fold: the popped entries it occupies, in right-hand-side order, and the
/// shape it builds.
///
/// `start`/`len` index the rule's popped entries for every kind, [`SiteKind::Attachment`]
/// included: an inlined attachment unit's fixed tokens and operand are popped entries of the
/// owner's own rule, so the site occupies them exactly as the one attachment-nonterminal entry
/// it replaces did. `nested` is empty except for an `Attachment` site, where it carries the
/// sites of the removed attachment rule's own `Node` fold, each indexing the same popped
/// entries. One level of nesting is all an inlined attachment ever needs, so a nested site is
/// never itself an `Attachment` site.
pub struct FoldSite {
    /// Index of the site's first entry among the rule's popped entries.
    pub start: u32,
    /// Number of entries the site occupies; zero for an absent choice.
    pub len: u32,
    /// The shape built.
    pub kind: SiteKind,
    /// The policy of the site's group and leaves.
    pub policy: FoldPolicy,
    /// The sites of the node an inlined attachment unit builds inside this site's group;
    /// non-empty only when `kind` is [`SiteKind::Attachment`].
    pub nested: Vec<FoldSite>,
}

impl FoldSite {
    /// A site occupying the popped-entries range `start..start + len`, with no nested sites.
    pub const fn flat(start: u32, len: u32, kind: SiteKind, policy: FoldPolicy) -> Self {
        Self {
            start,
            len,
            kind,
            policy,
            nested: Vec::new(),
        }
    }

    /// An [`SiteKind::Attachment`] site over the popped-entries range `start..start + len`,
    /// whose `nested` sites reproduce the `Node` fold the private attachment nonterminal's own
    /// rule would have built, now that inlining has removed that rule.
    pub const fn attachment(start: u32, len: u32, policy: FoldPolicy, nested: Vec<Self>) -> Self {
        Self {
            start,
            len,
            kind: SiteKind::Attachment,
            policy,
            nested,
        }
    }

    /// Appends this site's packed form to `into`: `start, len, kind, policy`, then, for an
    /// [`SiteKind::Attachment`] site, its nested site count followed by each nested site's own
    /// packed form.
    fn pack(&self, into: &mut Vec<u32>) {
        into.extend([self.start, self.len, self.kind.code(), self.policy.code()]);
        if self.kind == SiteKind::Attachment {
            into.push(self.nested.len() as u32);
            for site in &self.nested {
                debug_assert_ne!(
                    site.kind,
                    SiteKind::Attachment,
                    "an inlined attachment's own sites are never themselves inlined attachments"
                );
                site.pack(into);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// The provenance fold of one rule: what its reduce builds on the occurrence stack.
pub struct RuleFold {
    /// What the popped entries fold into.
    pub kind: FoldKind,
    /// The policy of the node, group or leaf built.
    pub policy: FoldPolicy,
    /// The sites of a [`FoldKind::Node`]; empty otherwise.
    pub sites: Vec<FoldSite>,
}

impl RuleFold {
    /// A pass-through fold.
    pub const fn through() -> Self {
        Self {
            kind: FoldKind::Through,
            policy: FoldPolicy::Inherited,
            sites: Vec::new(),
        }
    }

    /// A fold without sites.
    pub const fn plain(kind: FoldKind, policy: FoldPolicy) -> Self {
        Self {
            kind,
            policy,
            sites: Vec::new(),
        }
    }

    /// A node fold over `sites` under the inherited policy.
    pub const fn node(sites: Vec<FoldSite>) -> Self {
        Self {
            kind: FoldKind::Node,
            policy: FoldPolicy::Inherited,
            sites,
        }
    }

    /// Whether the fold or one of its sites (recursively, through a `Node`-kind site's own
    /// nested sites) applies a mandatory policy.
    pub fn is_mandatory(&self) -> bool {
        fn site_mandatory(site: &FoldSite) -> bool {
            site.policy == FoldPolicy::Mandatory || site.nested.iter().any(site_mandatory)
        }
        self.policy == FoldPolicy::Mandatory || self.sites.iter().any(site_mandatory)
    }

    /// Appends the packed form: `kind, policy, site count`, then each site's own packed form
    /// (see [`FoldSite::pack`]).
    pub fn pack(&self, into: &mut Vec<u32>) {
        into.push(self.kind.code());
        into.push(self.policy.code());
        into.push(self.sites.len() as u32);
        for site in &self.sites {
            site.pack(into);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// Whether a rule's reduction spine requires a following token to belong to root FOLLOW. The
/// runtime keeps one flag along the spine of last symbols; each reduce applies its rule's decision
/// to it.
pub enum TailDecision {
    /// The rule's last symbol decides: the flag is unchanged.
    Keep,
    /// The rule ends only before root FOLLOW, including a one-token enum alternative or a rule
    /// whose final site is a repetition.
    Reject,
    /// A left-recursive continuation of its own enum Node (`Expr -> Expr PLUS Expr`): the
    /// reduce completes an operand of an enclosing expression, so what follows belongs to
    /// whatever encloses it and the flag is cleared.
    Clear,
}

impl TailDecision {
    /// The packed code.
    pub const fn code(self) -> u32 {
        match self {
            Self::Keep => 0,
            Self::Reject => 1,
            Self::Clear => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The Rust value of one element as a reduce action names it: a grammar type's value wrapped in
/// `boxes` layers of `Box`.
///
/// The type is never an attachment unit; the lowering resolves a unit to the type of the
/// semantic site it passes through, because a unit's reduce is a pass-through.
pub struct ElementValue {
    /// The grammar type whose value the element carries.
    pub type_id: TypeId,
    /// `Box` layers around the value in the authored container shape.
    pub boxes: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// What an optional helper nonterminal wraps, so its reduce action knows the value to push.
pub enum OptionalContent {
    /// An erased optional fixed-token group: no value.
    Erased,
    /// A fixed-token presence: a `bool`.
    Presence,
    /// `Option<T>` around one element.
    Element(ElementValue),
    /// `Option<Vec1<T>>`, with `boxes` layers of `Box` between the option and the list.
    NonEmptyList {
        /// The list element.
        element: ElementValue,
        /// `Box` layers between the option and the list.
        boxes: u32,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// What the reduce action of a rule must build; the table-driven renderer emits one match arm
/// per action.
pub enum RuleAction {
    /// The synthetic start rule of a grammar type: reducing it accepts the parse.
    Accept {
        /// The grammar type whose parse completes.
        type_id: TypeId,
    },
    /// A content-token or admission-set member rule: the value is the token just popped.
    Token {
        /// The token type the value belongs to.
        type_id: TypeId,
    },
    /// A sequence, alias or enum alternative built from its value-carrying sites.
    Construct {
        /// The grammar type being built.
        type_id: TypeId,
        /// The enum alternative selected, when the type has several.
        variant: Option<DeclId>,
        /// One form per explicit site of the type, in site order.
        sites: Vec<SiteForm>,
    },
    /// A fixed-token-only sequence: it carries no value and pops none.
    FixedSequence {
        /// The normalized fixed-only sequence type.
        type_id: TypeId,
    },
    /// A repetition attachment unit: fixed tokens around one semantic site whose value passes
    /// through unchanged.
    Attachment {
        /// The synthetic unit type.
        type_id: TypeId,
    },
    /// A fixed-token presence envelope: required tokens around an optional sequence, reducing
    /// to the `bool` that says whether the sequence occurred.
    Envelope {
        /// The synthetic envelope type.
        type_id: TypeId,
        /// Whether this alternative carries the optional sequence.
        present: bool,
    },
    /// An absent optional helper: `None`, `false` or nothing, by content.
    OptionalAbsent {
        /// What the helper wraps.
        content: OptionalContent,
    },
    /// A present optional helper: `Some(value)`, `true` or nothing, by content.
    OptionalPresent {
        /// What the helper wraps.
        content: OptionalContent,
    },
    /// The empty alternative of a zero-or-more list helper: an empty `Vec`.
    ListEmpty {
        /// The list element.
        element: ElementValue,
    },
    /// A one-element non-empty list opened by its first element: `Vec1::new(element)`.
    ListFirst {
        /// The list element.
        element: ElementValue,
    },
    /// A non-empty list extended by one element after an optional separator.
    ListPush {
        /// The list element.
        element: ElementValue,
    },
    /// A zero-or-more list helper that holds a non-empty list: `Vec1` becomes `Vec`.
    ListFromNonEmpty {
        /// The list element.
        element: ElementValue,
    },
    /// A non-empty list with or without its trailing separator: the value passes through.
    ListTrailing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One production of the LR grammar.
pub struct Rule {
    /// The nonterminal the rule derives.
    pub lhs: NonterminalId,
    /// The right-hand side, empty for an epsilon rule.
    pub rhs: Vec<Symbol>,
    /// How many right-hand-side symbols carry a value: fixed tokens and fixed-only sequences do
    /// not, and neither does a bare terminal, whose token the [`RuleAction::Token`] rule reads
    /// from the popped state-stack entry.
    pub value_pops: u32,
    /// The declared level recorded for precedence resolution; `None` when the rule has no
    /// declared terminal and no `#[parse(prec = NAME)]` override.
    pub precedence: Option<RulePrecedence>,
    /// What reducing the rule builds.
    pub action: RuleAction,
    /// What reducing the rule folds on the occurrence stack.
    pub fold: RuleFold,
    /// Whether the rule's end is decided by its caller FOLLOW.
    pub tail: TailDecision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Why a nonterminal exists, so reports and later tickets can trace it to the authored grammar.
pub enum NonterminalOrigin {
    /// The nonterminal stands for one grammar type application.
    Type(TypeId),
    /// The synthetic start nonterminal of one grammar type.
    Start(TypeId),
    /// A helper the container lowering invented around a symbol.
    Helper,
    /// A per-site copy of a shared helper (`opt(X)@site`), made when the shared form's LALR(1)
    /// lookaheads merged into a reduce/reduce conflict (`docs/table-driven-parsing.md`, section
    /// 5). Its rules are the original's with self-references redirected, its reduce actions
    /// and folds are the original's, and it owns the use sites that the original no longer has.
    HelperCopy {
        /// The shared helper it was copied from.
        original: NonterminalId,
    },
    /// An authored restricted expression (`docs/declared-precedence.md`, "Restricted
    /// expressions"): a nonterminal whose rules are the admitted alternatives of one enum type,
    /// each with its first or last recursive operand parsed through this nonterminal instead of
    /// through the base. Its rules build the base type, so it is a grammar symbol only.
    DeclaredRestriction {
        /// The base enum type whose value every rule of this nonterminal builds.
        type_id: TypeId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One nonterminal of the LR grammar.
pub struct Nonterminal {
    /// Display name used by reports; unique within the grammar.
    pub name: String,
    /// Where the nonterminal came from.
    pub origin: NonterminalOrigin,
    /// Whether reducing to this nonterminal pushes a value; fixed-only helpers do not.
    pub carries_value: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One lookahead filter over terminals, as the table-driven token feed applies it: a record of
/// `token` followed by a record of one of `triggers` is presented to the automaton as `merged`.
pub struct TerminalFilter {
    /// The lexed terminal the filter merges.
    pub token: TerminalId,
    /// The merged terminal the feed presents instead.
    pub merged: TerminalId,
    /// The lexed terminals that trigger the merge, in declaration order.
    pub triggers: Vec<TerminalId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One grammar type's synthetic start rule, which gives the type its own start state.
pub struct StartRule {
    /// The grammar type the start rule enters.
    pub type_id: TypeId,
    /// The rule `S'(type) -> type`.
    pub rule: RuleId,
}

#[derive(Clone, Eq, PartialEq)]
/// A context-free grammar in the form the LALR(1) construction consumes.
///
/// Invariants: terminal `terminal_count() - 1` is the end marker; every [`StartRule`] rule has a
/// right-hand side of exactly one nonterminal; rules are stored in declaration order and that
/// order is the rule numbering the tables and reports use.
pub struct LrGrammar {
    /// Terminal names in dense order, ending with the end marker.
    terminals: Vec<String>,
    /// Nonterminals in dense order.
    nonterminals: Vec<Nonterminal>,
    /// Rules in declaration order.
    rules: Vec<Rule>,
    /// Rule identities grouped by left-hand side, in declaration order.
    rules_by_lhs: Vec<Vec<RuleId>>,
    /// Start rules in grammar-type order.
    starts: Vec<StartRule>,
    /// Nonterminal lookup by name, so the textbook builders can refer to symbols by spelling.
    by_name: BTreeMap<String, NonterminalId>,
    /// The lookahead filters the token feed must apply, in declaration order.
    filters: Vec<TerminalFilter>,
    /// The `precedence` declaration by terminal, `None` for a terminal outside it.
    declared_precedence: Vec<Option<DeclaredPrecedence>>,
}

/// Display name of the synthetic end marker terminal.
pub const END_MARKER: &str = "$end";

impl LrGrammar {
    /// Starts a grammar over the given token-kind names; the end marker is appended.
    pub fn new(kind_names: impl IntoIterator<Item = String>) -> Self {
        let mut terminals: Vec<String> = kind_names.into_iter().collect();
        terminals.push(END_MARKER.to_owned());
        let declared_precedence = vec![None; terminals.len()];
        Self {
            terminals,
            nonterminals: Vec::new(),
            rules: Vec::new(),
            rules_by_lhs: Vec::new(),
            starts: Vec::new(),
            by_name: BTreeMap::new(),
            filters: Vec::new(),
            declared_precedence,
        }
    }

    /// Records `terminal`'s entry in the `precedence` declaration.
    pub fn declare_precedence(&mut self, terminal: TerminalId, precedence: DeclaredPrecedence) {
        self.declared_precedence[terminal.index()] = Some(precedence);
    }

    /// The `precedence` declaration's entry for `terminal`, if it is in one.
    pub fn declared_precedence(&self, terminal: TerminalId) -> Option<DeclaredPrecedence> {
        self.declared_precedence[terminal.index()]
    }

    /// Returns the end-marker terminal.
    pub fn end_marker(&self) -> TerminalId {
        TerminalId::new((self.terminals.len() - 1) as u32)
    }

    /// Returns the number of terminals including the end marker.
    pub fn terminal_count(&self) -> usize {
        self.terminals.len()
    }

    /// Returns the number of nonterminals.
    pub fn nonterminal_count(&self) -> usize {
        self.nonterminals.len()
    }

    /// Returns the display name of a terminal.
    pub fn terminal_name(&self, terminal: TerminalId) -> &str {
        &self.terminals[terminal.index()]
    }

    /// Returns one nonterminal.
    pub fn nonterminal(&self, id: NonterminalId) -> &Nonterminal {
        &self.nonterminals[id.index()]
    }

    /// Returns every nonterminal in dense order.
    pub fn nonterminals(&self) -> &[Nonterminal] {
        &self.nonterminals
    }

    /// Returns one rule.
    pub fn rule(&self, id: RuleId) -> &Rule {
        &self.rules[id.index()]
    }

    /// Returns every rule in declaration order, mutably, for the lowering's tail post-pass.
    pub fn rules_mut(&mut self) -> &mut [Rule] {
        &mut self.rules
    }

    /// Returns every rule in declaration order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Returns the rules deriving one nonterminal, in declaration order.
    pub fn rules_of(&self, lhs: NonterminalId) -> &[RuleId] {
        &self.rules_by_lhs[lhs.index()]
    }

    /// Returns the start rules in grammar-type order.
    pub fn starts(&self) -> &[StartRule] {
        &self.starts
    }

    /// Returns the display name of a symbol.
    pub fn symbol_name(&self, symbol: Symbol) -> &str {
        match symbol {
            Symbol::Terminal(terminal) => self.terminal_name(terminal),
            Symbol::Nonterminal(nonterminal) => &self.nonterminal(nonterminal).name,
        }
    }

    /// Looks a nonterminal up by its display name.
    pub fn nonterminal_by_name(&self, name: &str) -> Option<NonterminalId> {
        self.by_name.get(name).copied()
    }

    /// Adds a nonterminal; names must be unique.
    pub fn add_nonterminal(
        &mut self,
        name: impl Into<String>,
        origin: NonterminalOrigin,
        carries_value: bool,
    ) -> NonterminalId {
        let name = name.into();
        let id = NonterminalId::new(self.nonterminals.len() as u32);
        assert!(
            self.by_name.insert(name.clone(), id).is_none(),
            "duplicate nonterminal name `{name}`"
        );
        self.nonterminals.push(Nonterminal {
            name,
            origin,
            carries_value,
        });
        self.rules_by_lhs.push(Vec::new());
        id
    }

    /// Adds a rule and returns its identity.
    pub fn add_rule(&mut self, rule: Rule) -> RuleId {
        let id = RuleId::new(self.rules.len() as u32);
        self.rules_by_lhs[rule.lhs.index()].push(id);
        self.rules.push(rule);
        id
    }

    /// The lookahead filters the token feed must apply.
    pub fn filters(&self) -> &[TerminalFilter] {
        &self.filters
    }

    /// Records the lookahead filters the substitution pass applied, which the tables carry to
    /// the token feed.
    pub fn set_filters(&mut self, filters: Vec<TerminalFilter>) {
        self.filters = filters;
    }

    /// Replaces every rule with `rules`, in the given order, before any start rule exists.
    ///
    /// The lookahead-filter pass uses it to substitute merged terminals and to insert a twin
    /// rule directly after its original, so the rules of one nonterminal stay contiguous for the
    /// reduce dispatcher. The new order is the rule numbering from then on.
    pub fn replace_rules(&mut self, rules: Vec<Rule>) {
        assert!(
            self.starts.is_empty(),
            "rules are replaced before the start rules exist"
        );
        self.rules.clear();
        for group in &mut self.rules_by_lhs {
            group.clear();
        }
        for rule in rules {
            self.add_rule(rule);
        }
    }

    /// Adds the synthetic start nonterminal and rule `S'(type) -> target` for one grammar type.
    pub fn add_start(&mut self, type_id: TypeId, target: NonterminalId) -> RuleId {
        let name = format!("$accept({})", self.nonterminal(target).name);
        let carries_value = self.nonterminal(target).carries_value;
        let lhs = self.add_nonterminal(name, NonterminalOrigin::Start(type_id), carries_value);
        let rule = self.add_rule(Rule {
            lhs,
            rhs: vec![Symbol::Nonterminal(target)],
            value_pops: u32::from(carries_value),
            precedence: None,
            action: RuleAction::Accept { type_id },
            fold: RuleFold::through(),
            tail: TailDecision::Keep,
        });
        self.starts.push(StartRule { type_id, rule });
        rule
    }

    /// Reports whether a rule is a start rule.
    pub fn is_start_rule(&self, rule: RuleId) -> bool {
        matches!(self.rule(rule).action, RuleAction::Accept { .. })
    }

    /// Computes which nonterminals derive the empty string.
    pub fn nullable_nonterminals(&self) -> Vec<bool> {
        let mut nullable = vec![false; self.nonterminals.len()];
        let mut changed = true;
        while changed {
            changed = false;
            for rule in &self.rules {
                if nullable[rule.lhs.index()] {
                    continue;
                }
                let all_nullable = rule.rhs.iter().all(|symbol| match symbol {
                    Symbol::Terminal(_) => false,
                    Symbol::Nonterminal(id) => nullable[id.index()],
                });
                if all_nullable {
                    nullable[rule.lhs.index()] = true;
                    changed = true;
                }
            }
        }
        nullable
    }

    /// Renders one rule as `lhs -> rhs`, or `lhs -> %empty` for an epsilon rule.
    pub fn render_rule(&self, id: RuleId) -> String {
        let rule = self.rule(id);
        let mut text = self.nonterminal(rule.lhs).name.clone();
        text.push_str(" ->");
        if rule.rhs.is_empty() {
            text.push_str(" %empty");
        }
        for symbol in &rule.rhs {
            text.push(' ');
            text.push_str(self.symbol_name(*symbol));
        }
        text
    }
}

impl fmt::Debug for LrGrammar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = formatter.debug_list();
        for index in 0..self.rules.len() {
            list.entry(&self.render_rule(RuleId::new(index as u32)));
        }
        list.finish()
    }
}

/// Builds textbook grammars from symbolic rule text, for tests and worked examples.
///
/// Terminals are named up front; every other identifier in a rule is a nonterminal, created on
/// first sight in order of appearance. Each nonterminal gets a synthetic grammar type identity
/// equal to its dense index so that tables and reports have something to name.
#[cfg(test)]
pub(crate) struct TextbookGrammar {
    /// The grammar under construction.
    grammar: LrGrammar,
    /// Terminal lookup by spelling.
    terminals: BTreeMap<String, TerminalId>,
    /// Declared precedence levels by spelling, including precedence-only names.
    levels: BTreeMap<String, DeclaredPrecedence>,
}

#[cfg(test)]
impl TextbookGrammar {
    /// Starts a grammar over the given terminal spellings.
    pub fn new(terminals: &[&str]) -> Self {
        let grammar = LrGrammar::new(terminals.iter().map(|name| (*name).to_owned()));
        let terminals = terminals
            .iter()
            .enumerate()
            .map(|(index, name)| ((*name).to_owned(), TerminalId::new(index as u32)))
            .collect();
        Self {
            grammar,
            terminals,
            levels: BTreeMap::new(),
        }
    }

    /// Returns the nonterminal for a spelling, creating it on first use.
    fn nonterminal(&mut self, name: &str) -> NonterminalId {
        if let Some(id) = self.grammar.nonterminal_by_name(name) {
            return id;
        }
        let type_id = TypeId::new(self.grammar.nonterminal_count() as u32);
        self.grammar
            .add_nonterminal(name, NonterminalOrigin::Type(type_id), true)
    }

    /// Adds one rule `lhs -> rhs`, where `rhs` is whitespace-separated symbols; empty means
    /// epsilon.
    pub fn rule(&mut self, lhs: &str, rhs: &str) -> RuleId {
        let lhs = self.nonterminal(lhs);
        let mut symbols = Vec::new();
        let mut value_pops = 0;
        for name in rhs.split_whitespace() {
            if let Some(terminal) = self.terminals.get(name) {
                symbols.push(Symbol::Terminal(*terminal));
            } else {
                symbols.push(Symbol::Nonterminal(self.nonterminal(name)));
                value_pops += 1;
            }
        }
        let type_id = match self.grammar.nonterminal(lhs).origin {
            NonterminalOrigin::Type(type_id) => type_id,
            _ => unreachable!("textbook nonterminals are grammar types"),
        };
        let sites = (0..symbols.len() as u32).map(SiteForm::required).collect();
        // Textbook grammars exercise the automaton, its conflicts and its tables, never a
        // provenance fold; the fold is a placeholder that no test reduces under capture.
        self.grammar.add_rule(Rule {
            lhs,
            rhs: symbols,
            value_pops,
            precedence: None,
            action: RuleAction::Construct {
                type_id,
                variant: None,
                sites,
            },
            fold: RuleFold::through(),
            tail: TailDecision::Keep,
        })
    }

    /// Adds `lhs -> rhs` as a rule of a helper nonterminal, creating `lhs` as a helper on first
    /// use, the way the lowering records `opt(...)` and `list(...)`; the action is a placeholder,
    /// because textbook grammars never reduce a value. Declare a helper's rules before the rules
    /// that use it, because [`rule`](Self::rule) creates an unknown symbol as a grammar type.
    pub fn helper_rule(&mut self, lhs: &str, rhs: &str) -> RuleId {
        let lhs = match self.grammar.nonterminal_by_name(lhs) {
            Some(id) => {
                assert_eq!(
                    self.grammar.nonterminal(id).origin,
                    NonterminalOrigin::Helper,
                    "`{lhs}` was created as a grammar type; declare helper rules first"
                );
                id
            }
            None => self
                .grammar
                .add_nonterminal(lhs, NonterminalOrigin::Helper, true),
        };
        let mut symbols = Vec::new();
        let mut value_pops = 0;
        for name in rhs.split_whitespace() {
            if let Some(terminal) = self.terminals.get(name) {
                symbols.push(Symbol::Terminal(*terminal));
            } else {
                symbols.push(Symbol::Nonterminal(self.nonterminal(name)));
                value_pops += 1;
            }
        }
        self.grammar.add_rule(Rule {
            lhs,
            rhs: symbols,
            value_pops,
            precedence: None,
            action: RuleAction::ListTrailing,
            fold: RuleFold::through(),
            tail: TailDecision::Keep,
        })
    }

    /// Adds `lhs -> terminal` as a token-wrapper rule whose reduce yields the token, the way
    /// the lowering records an admission-set member or a content token.
    pub fn token_rule(&mut self, lhs: &str, terminal: &str) -> RuleId {
        let lhs = self.nonterminal(lhs);
        let terminal = self.terminals[terminal];
        let type_id = match self.grammar.nonterminal(lhs).origin {
            NonterminalOrigin::Type(type_id) => type_id,
            _ => unreachable!("textbook nonterminals are grammar types"),
        };
        self.grammar.add_rule(Rule {
            lhs,
            rhs: vec![Symbol::Terminal(terminal)],
            value_pops: 0,
            precedence: None,
            action: RuleAction::Token { type_id },
            fold: RuleFold::through(),
            tail: TailDecision::Keep,
        })
    }

    /// Declares one level of the `precedence` declaration over terminal spellings, the way
    /// `left(bp = N) { ... }` does; a spelling that is no terminal is a precedence-only name
    /// such as bison's `UMINUS`, usable by [`Self::rule_precedence`] alone. Levels must be
    /// declared from lowest to highest binding power.
    pub fn precedence_level(
        &mut self,
        associativity: Associativity,
        binding_power: u8,
        names: &[&str],
    ) {
        let precedence = DeclaredPrecedence {
            binding_power,
            associativity,
        };
        for name in names {
            if let Some(terminal) = self.terminals.get(*name) {
                self.grammar.declare_precedence(*terminal, precedence);
            }
            self.levels.insert((*name).to_owned(), precedence);
        }
    }

    /// Gives `rule` the precedence of the declared level that holds `name`, the way
    /// `#[parse(prec = NAME)]` (bison's `%prec`) does.
    pub fn rule_precedence(&mut self, rule: RuleId, name: &str) {
        let precedence = self.levels[name];
        self.grammar.rules[rule.index()].precedence = Some(precedence.rule_precedence());
    }

    /// Gives every rule without an override the precedence of its last declared terminal, as
    /// the lowering does after every rule is emitted.
    pub fn assign_declared_precedence(&mut self) {
        super::declared_precedence::assign_rule_precedence(&mut self.grammar);
    }

    /// Adds the start rule for one nonterminal.
    pub fn start(&mut self, name: &str) -> RuleId {
        let target = self.nonterminal(name);
        let type_id = match self.grammar.nonterminal(target).origin {
            NonterminalOrigin::Type(type_id) => type_id,
            _ => unreachable!("textbook nonterminals are grammar types"),
        };
        self.grammar.add_start(type_id, target)
    }

    /// Finishes the grammar.
    pub fn finish(self) -> LrGrammar {
        self.grammar
    }
}
