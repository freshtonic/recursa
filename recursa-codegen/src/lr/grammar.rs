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

use crate::{DeclId, PrattPosition, TypeId};

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
    /// Prefix and postfix operators, which have one operand and no associativity question.
    None,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// The binding powers of a Pratt variant, recorded on its lowered rule for precedence resolution.
pub struct RulePrecedence {
    /// The authored Pratt position with its raw binding powers.
    pub position: PrattPosition,
    /// The precedence level, which is `lbp` for infix and postfix and `bp` for prefix.
    pub level: u8,
    /// The associativity derived from the binding-power pair.
    pub associativity: Associativity,
    /// The Pratt nonterminal whose operator tokens this precedence is compared with: the rule's
    /// own left-hand side for a Pratt rule, and the Pratt nonterminal (or exclusion clone) that
    /// owns the operand for a fixed-token attachment around one.
    pub owner: NonterminalId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// An infix binding-power pair with no precedence equivalent: `rbp` is neither `lbp + 1`
/// (left-associative) nor `lbp` or `lbp - 1` (right-associative). The lowering reports it as
/// `RCA0402` naming the variant.
pub struct UnrepresentableBindingPowers {
    /// The left binding power.
    pub lbp: u8,
    /// The right binding power.
    pub rbp: u8,
}

impl RulePrecedence {
    /// Derives the precedence facts of one Pratt position of `owner`; atoms have none, and an
    /// infix pair outside the three representable shapes is the error.
    pub fn from_position(
        position: PrattPosition,
        owner: NonterminalId,
    ) -> Result<Option<Self>, UnrepresentableBindingPowers> {
        match position {
            PrattPosition::Atom => Ok(None),
            PrattPosition::Prefix { bp } | PrattPosition::Postfix { bp } => Ok(Some(Self {
                position,
                level: bp,
                associativity: Associativity::None,
                owner,
            })),
            PrattPosition::Infix { lbp, rbp } => {
                let associativity = if u16::from(rbp) == u16::from(lbp) + 1 {
                    Associativity::Left
                } else if rbp == lbp || u16::from(rbp) + 1 == u16::from(lbp) {
                    Associativity::Right
                } else {
                    return Err(UnrepresentableBindingPowers { lbp, rbp });
                };
                Ok(Some(Self {
                    position,
                    level: lbp,
                    associativity,
                    owner,
                }))
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// What the reduce action of a rule must build; ticket #113 renders one match arm per action.
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
    /// A sequence, alias, enum alternative or Pratt variant built from its value-carrying sites.
    Construct {
        /// The grammar type being built.
        type_id: TypeId,
        /// The enum alternative or Pratt variant selected, when the type has several.
        variant: Option<DeclId>,
    },
    /// A fixed-token-only sequence: it carries no value and pops none.
    FixedSequence {
        /// The normalized fixed-only sequence type.
        type_id: TypeId,
    },
    /// An absent optional: `None` for a valued target, `false` for a presence, nothing when erased.
    OptionalAbsent,
    /// A present optional: `Some(value)` for a valued target, `true` for a presence, nothing when
    /// erased.
    OptionalPresent,
    /// An empty zero-or-more list.
    ListEmpty,
    /// A one-element list opened by its first element.
    ListFirst,
    /// A list extended by one element after an optional separator.
    ListPush,
    /// A list whose value passes through unchanged: a trailing separator or a non-empty zero-or-more list.
    ListPass,
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
    /// Binding powers recorded for precedence resolution; `None` outside Pratt variants.
    pub precedence: Option<RulePrecedence>,
    /// What reducing the rule builds.
    pub action: RuleAction,
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
    /// A distinct Pratt language: the grammar type minus the LED variants a
    /// `#[parse(pratt(exclude(...)))]` site names and minus the LED variants below an operand's
    /// minimum binding power, or the fixed-token attachment around such an operand, cloned once
    /// per distinct restriction the way gram.y has `b_expr` beside `a_expr`. Its rules build
    /// the original type.
    Restriction {
        /// The Pratt type, or the attachment sequence around one of its operands, cloned.
        type_id: TypeId,
        /// The excluded LED variants, as normalized declarations in ascending order.
        excluded: Vec<DeclId>,
        /// The minimum binding power; zero for an exclusion-only clone and for attachments.
        min_bp: u8,
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
}

/// Display name of the synthetic end marker terminal.
pub const END_MARKER: &str = "$end";

impl LrGrammar {
    /// Starts a grammar over the given token-kind names; the end marker is appended.
    pub fn new(kind_names: impl IntoIterator<Item = String>) -> Self {
        let mut terminals: Vec<String> = kind_names.into_iter().collect();
        terminals.push(END_MARKER.to_owned());
        Self {
            terminals,
            nonterminals: Vec::new(),
            rules: Vec::new(),
            rules_by_lhs: Vec::new(),
            starts: Vec::new(),
            by_name: BTreeMap::new(),
            filters: Vec::new(),
        }
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
        assert!(self.starts.is_empty(), "rules are replaced before the start rules exist");
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
        Self { grammar, terminals }
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
        self.grammar.add_rule(Rule {
            lhs,
            rhs: symbols,
            value_pops,
            precedence: None,
            action: RuleAction::Construct {
                type_id,
                variant: None,
            },
        })
    }

    /// Adds `lhs -> rhs` as a Pratt rule with the binding powers of `position`, the way the
    /// lowering records an infix, prefix or postfix variant.
    pub fn pratt_rule(&mut self, lhs: &str, rhs: &str, position: PrattPosition) -> RuleId {
        self.attachment_rule(lhs, rhs, lhs, position)
    }

    /// Adds `lhs -> rhs` carrying the binding powers of `position` as a rule of `owner`'s
    /// Pratt language, the way the lowering records a fixed-token attachment around an operand
    /// (`Plus -> + E` for `E -> E Plus`).
    pub fn attachment_rule(
        &mut self,
        lhs: &str,
        rhs: &str,
        owner: &str,
        position: PrattPosition,
    ) -> RuleId {
        let id = self.rule(lhs, rhs);
        let owner = self.nonterminal(owner);
        self.grammar.rules[id.index()].precedence = RulePrecedence::from_position(position, owner)
            .expect("textbook Pratt rules use representable binding powers");
        id
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
