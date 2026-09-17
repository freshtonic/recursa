#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Declared precedence for the Recursa parser's [`ConflictResolver`].
//!
//! Recursa resolves every shift/reduce conflict the way bison does, and with one source only:
//! the `precedence { ... }` declaration of the token grammar
//! ([`declared_precedence`] records it on the terminals and
//! assigns each rule its level). See `docs/declared-precedence.md`.
//!
//! The reducing rule's level is the level of its last terminal with a declared level, or of the
//! level its `#[parse(prec = NAME)]` override names. The lookahead's level is its own declared
//! level. The higher level wins: a higher rule reduces, a higher token shifts. On a tie the
//! rule's associativity decides, as `%left`, `%right` and `%nonassoc` do: `left` reduces,
//! `right` shifts, and `nonassoc` makes the cell an error action. If either side has no level,
//! the conflict stays unresolved and is reported as `RCA0400`.
//!
//! There is no second source: no per-nonterminal token levels, and no rule precedence derived
//! from anything but the declaration. An operator variant's attached fixed tokens lower inline
//! into the variant's own rule (`docs/declared-precedence.md`, "Attachment units inline"), so
//! the operator the author wrote is a terminal of the rule that reduces and an override
//! authored on the variant lands on that same rule.

use std::cmp::Ordering;

use super::automaton::{ConflictResolver, Item, Resolution};
use super::grammar::{Associativity, LrGrammar, RuleId, RulePrecedence, TerminalId};
use super::report::LrDiagnostic;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// The declared-precedence conflict resolver: it holds no state, because every fact it reads
/// is already on the grammar's rules and terminals.
pub struct PrecedenceResolver;

impl PrecedenceResolver {
    /// Builds the resolver for `grammar`.
    ///
    /// The signature keeps the fallible shape [`build_with_helper_copies`](crate::lr::build_with_helper_copies)
    /// expects of a resolver factory, so that a resolver derived from the rules can still fail;
    /// declared precedence never does, because the declaration is validated in source analysis.
    pub const fn new(_grammar: &LrGrammar) -> Result<Self, Vec<LrDiagnostic>> {
        Ok(Self)
    }
}

/// Bison's verdict for one rule level against one token level.
fn verdict(rule: &RulePrecedence, token: u8) -> Resolution {
    match rule.level.cmp(&token) {
        Ordering::Greater => Resolution::Reduce,
        Ordering::Less => Resolution::Shift,
        Ordering::Equal => match rule.associativity {
            Associativity::Left => Resolution::Reduce,
            Associativity::Right => Resolution::Shift,
            Associativity::NonAssociative => Resolution::Error,
        },
    }
}

impl ConflictResolver for PrecedenceResolver {
    fn resolve(
        &self,
        grammar: &LrGrammar,
        reduce: RuleId,
        lookahead: TerminalId,
        _shift_items: &[Item],
    ) -> Option<Resolution> {
        let precedence = grammar.rule(reduce).precedence?;
        let declared = grammar.declared_precedence(lookahead)?;
        Some(verdict(&precedence, declared.binding_power))
    }
}
