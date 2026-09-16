#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Declared precedence: bison's token-precedence facility over the `precedence` declaration
//! of the token grammar.
//!
//! The declaration lists levels from lowest to highest, each `left`, `right` or `nonassoc`
//! over token names, with a binding power that orders the level against the others; the
//! lowering records each terminal's level on the [`LrGrammar`]. This module assigns rule
//! precedence the way bison does: a rule with no `#[parse(prec = ...)]` override takes the
//! level of its last terminal that has one. The resolver in
//! [`precedence`](crate::lr::precedence) then compares a reducing rule's level with the
//! lookahead's declared level.

use super::grammar::{
    Associativity, DeclaredPrecedence, LrGrammar, RuleAction, Symbol, TerminalId,
};
use crate::{GrammarDescription, PrecedenceAssociativity, PrecedenceLevelId};

/// Applies the `precedence` declaration of `description` to the lowered `grammar`: records
/// every declared kind's level on its terminal, gives the rules of each Node or variant with a
/// `#[parse(prec = NAME)]` override that level, and then assigns every remaining rule the
/// precedence of its last declared terminal. Runs once, after every rule is emitted, so that
/// the twins a lookahead filter emits and the start rules are covered.
pub fn apply(grammar: &mut LrGrammar, description: &GrammarDescription) {
    if description.precedence.is_empty() {
        return;
    }
    let level = |id: PrecedenceLevelId| {
        let level = &description.precedence[id.index()];
        DeclaredPrecedence {
            binding_power: level.binding_power,
            associativity: match level.associativity {
                PrecedenceAssociativity::Left => Associativity::Left,
                PrecedenceAssociativity::Right => Associativity::Right,
                PrecedenceAssociativity::NonAssociative => Associativity::NonAssociative,
            },
        }
    };
    for (index, _) in description.precedence.iter().enumerate() {
        let id = PrecedenceLevelId::new(index as u32);
        for kind in &description.precedence[index].kinds {
            grammar.declare_precedence(TerminalId::new(u32::from(kind.get())), level(id));
        }
    }
    // A variant's override wins over its Node's.
    let override_of = |rule: &super::grammar::Rule| -> Option<PrecedenceLevelId> {
        let (type_id, variant) = match rule.action {
            RuleAction::Construct {
                type_id, variant, ..
            } => (type_id, variant),
            RuleAction::Token { type_id } => (type_id, None),
            _ => return None,
        };
        variant
            .and_then(|declaration| description.declarations[declaration.index()].precedence)
            .or_else(|| {
                let declaration = description.types[type_id.index()].declaration;
                description.declarations[declaration.index()].precedence
            })
    };
    for rule in grammar.rules_mut() {
        if rule.precedence.is_some() {
            continue;
        }
        if let Some(id) = override_of(rule) {
            rule.precedence = Some(level(id).rule_precedence());
        }
    }
    assign_rule_precedence(grammar);
}

/// Gives every rule that still has no precedence the declared precedence of its last terminal
/// with one. A rule that already carries an override keeps it; a rule whose terminals are all
/// undeclared keeps none.
pub fn assign_rule_precedence(grammar: &mut LrGrammar) {
    let declared: Vec<_> = (0..grammar.terminal_count())
        .map(|index| grammar.declared_precedence(super::grammar::TerminalId::new(index as u32)))
        .collect();
    for rule in grammar.rules_mut() {
        if rule.precedence.is_some() {
            continue;
        }
        let last = rule.rhs.iter().rev().find_map(|symbol| match symbol {
            Symbol::Terminal(terminal) => declared[terminal.index()],
            Symbol::Nonterminal(_) => None,
        });
        rule.precedence = last.map(DeclaredPrecedence::rule_precedence);
    }
}

#[cfg(test)]
mod tests {
    //! The declaration over textbook grammars shaped after gram.y's conflict families.

    use super::assign_rule_precedence;
    use crate::lr::automaton::{Automaton, AutomatonOptions, ConflictKind, Resolution};
    use crate::lr::grammar::{Associativity, LrGrammar, RuleId, TerminalId, TextbookGrammar};
    use crate::lr::precedence::PrecedenceResolver;
    use crate::lr::report::automaton_dump;
    use crate::lr::tables::{LrTables, SimulationError, simulate};

    /// Finds a terminal by name.
    fn terminal(grammar: &LrGrammar, name: &str) -> TerminalId {
        (0..grammar.terminal_count())
            .map(|index| TerminalId::new(index as u32))
            .find(|terminal| grammar.terminal_name(*terminal) == name)
            .unwrap_or_else(|| panic!("no terminal named {name}"))
    }

    /// Assigns declared rule precedence, finishes the grammar and builds its automaton.
    fn build(mut textbook: TextbookGrammar, start: &str) -> (LrGrammar, Automaton) {
        textbook.assign_declared_precedence();
        textbook.start(start);
        let grammar = textbook.finish();
        let precedence = PrecedenceResolver::new(&grammar).expect("the resolver is infallible");
        let automaton = Automaton::build(&grammar, &precedence, AutomatonOptions::default());
        (grammar, automaton)
    }

    /// Parses `source` (space-separated terminal names) and returns the rules reduced.
    fn parse(
        grammar: &LrGrammar,
        automaton: &Automaton,
        source: &str,
    ) -> Result<Vec<String>, SimulationError> {
        let tables = LrTables::pack(grammar, automaton);
        let start = automaton.start_states()[0].1.index() as u32;
        let input: Vec<TerminalId> = source
            .split_whitespace()
            .map(|name| terminal(grammar, name))
            .collect();
        simulate(grammar, &tables, start, &input).map(|trace| trace.reductions)
    }

    /// Parses `source` and returns the rules reduced, or panics.
    fn reductions(grammar: &LrGrammar, automaton: &Automaton, source: &str) -> Vec<String> {
        parse(grammar, automaton, source)
            .unwrap_or_else(|error| panic!("{source} does not parse: {error:?}"))
    }

    /// The resolved conflicts as `(rule, lookahead, verdict)`.
    fn settled(grammar: &LrGrammar, automaton: &Automaton) -> Vec<(String, String, Resolution)> {
        automaton
            .resolved_conflicts()
            .iter()
            .map(|resolved| {
                (
                    grammar.render_rule(resolved.reduce),
                    grammar.terminal_name(resolved.lookahead).to_owned(),
                    resolved.resolution,
                )
            })
            .collect()
    }

    /// `a LIKE b . ESCAPE`: pg-sql attaches the pattern and its optional escape clause to the
    /// `LIKE` variant, and a target may end in a bare label, so `ESCAPE` can also name the
    /// column. gram.y: `%nonassoc BETWEEN IN_P LIKE ...` then `%nonassoc ESCAPE`.
    fn like_escape() -> (TextbookGrammar, RuleId) {
        let mut grammar = TextbookGrammar::new(&["id", "LIKE", "ESCAPE", "VALUE", "+"]);
        grammar.rule("T", "E");
        grammar.rule("T", "E Label");
        grammar.rule("Label", "ESCAPE");
        grammar.rule("Label", "VALUE");
        grammar.rule("Label", "id");
        grammar.rule("E", "id");
        grammar.rule("E", "E LIKE E");
        let escape = grammar.rule("E", "E LIKE E ESCAPE E");
        grammar.rule("E", "E + E");
        (grammar, escape)
    }

    #[test]
    fn without_the_declaration_the_escape_clause_is_a_conflict() {
        let (grammar, automaton) = build(like_escape().0, "T");
        let escape = terminal(&grammar, "ESCAPE");
        let conflicts: Vec<_> = automaton
            .conflicts()
            .iter()
            .filter(|conflict| conflict.lookahead == escape)
            .collect();
        assert_eq!(conflicts.len(), 1, "{:?}", automaton.conflicts());
        assert!(matches!(
            conflicts[0].kind,
            ConflictKind::ShiftReduce { .. }
        ));
    }

    #[test]
    fn escape_declared_above_like_shifts_the_escape_clause() {
        let (mut textbook, escape_rule) = like_escape();
        textbook.precedence_level(Associativity::NonAssociative, 5, &["LIKE"]);
        textbook.precedence_level(Associativity::NonAssociative, 6, &["ESCAPE"]);
        textbook.precedence_level(Associativity::NonAssociative, 7, &["id", "VALUE"]);
        textbook.precedence_level(Associativity::Left, 10, &["+"]);
        // `a_expr LIKE a_expr ESCAPE a_expr %prec LIKE`: the clause's last declared terminal is
        // `ESCAPE`, one level above `LIKE`, so the override keeps the rule at `LIKE`'s level.
        textbook.rule_precedence(escape_rule, "LIKE");
        let (grammar, automaton) = build(textbook, "T");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let settled = settled(&grammar, &automaton);
        assert!(
            settled.contains(&(
                "E -> E LIKE E".to_owned(),
                "ESCAPE".to_owned(),
                Resolution::Shift
            )),
            "{settled:?}"
        );
        // `E -> E LIKE E` (5) against `+` (10) shifts.
        assert!(
            settled.contains(&(
                "E -> E LIKE E".to_owned(),
                "+".to_owned(),
                Resolution::Shift
            )),
            "{settled:?}"
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id LIKE id ESCAPE id"),
            [
                "E -> id",
                "E -> id",
                "E -> id",
                "E -> E LIKE E ESCAPE E",
                "T -> E"
            ]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id LIKE id + id ESCAPE id"),
            [
                "E -> id",
                "E -> id",
                "E -> id",
                "E -> E + E",
                "E -> id",
                "E -> E LIKE E ESCAPE E",
                "T -> E"
            ]
        );
        // A bare `ESCAPE` label after a comparison is a syntax error, as in PostgreSQL; after
        // a plain column it is still a label.
        assert!(parse(&grammar, &automaton, "id LIKE id ESCAPE").is_err());
        assert_eq!(
            reductions(&grammar, &automaton, "id ESCAPE"),
            ["E -> id", "Label -> ESCAPE", "T -> E Label"]
        );
    }

    /// `x IS JSON . VALUE`: the postfix `IS JSON` test with its optional type, against
    /// `VALUE` as a bare label. gram.y: `%nonassoc IS ...` far below `%nonassoc IDENT ...
    /// VALUE_P WITH WITHOUT`.
    #[test]
    fn value_declared_with_ident_shifts_after_is_json() {
        let mut grammar = TextbookGrammar::new(&["id", "IS", "JSON", "VALUE"]);
        grammar.rule("T", "E");
        grammar.rule("T", "E Label");
        grammar.rule("Label", "VALUE");
        grammar.rule("Label", "id");
        grammar.rule("E", "id");
        grammar.rule("E", "E IS JSON");
        let typed = grammar.rule("E", "E IS JSON VALUE");
        grammar.precedence_level(Associativity::NonAssociative, 6, &["IS"]);
        grammar.precedence_level(Associativity::NonAssociative, 7, &["id", "VALUE"]);
        // `%prec IS`: the typed form's last declared terminal is `VALUE`, one level above.
        grammar.rule_precedence(typed, "IS");
        let (grammar, automaton) = build(grammar, "T");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            settled(&grammar, &automaton),
            [(
                "E -> E IS JSON".to_owned(),
                "VALUE".to_owned(),
                Resolution::Shift
            )]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id IS JSON VALUE"),
            ["E -> id", "E -> E IS JSON VALUE", "T -> E"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id IS JSON id"),
            ["E -> id", "E -> E IS JSON", "Label -> id", "T -> E Label"]
        );
    }

    /// `UNBOUNDED . PRECEDING` and `NESTED . PATH`: an unreserved keyword that names a column
    /// against the syntax it opens. gram.y: `%nonassoc UNBOUNDED NESTED` just below
    /// `%nonassoc IDENT ... PRECEDING ... PATH`.
    #[test]
    fn unbounded_and_nested_declared_below_preceding_and_path_shift() {
        let mut grammar =
            TextbookGrammar::new(&["id", "UNBOUNDED", "PRECEDING", "NESTED", "PATH", "string"]);
        grammar.rule("S", "Bound");
        grammar.rule("S", "Column");
        grammar.rule("Bound", "UNBOUNDED PRECEDING");
        grammar.rule("Bound", "E PRECEDING");
        grammar.rule("Column", "NESTED PATH string");
        grammar.rule("Column", "E PATH string");
        grammar.rule("E", "Col");
        grammar.rule("Col", "id");
        grammar.rule("Col", "UNBOUNDED");
        grammar.rule("Col", "NESTED");
        grammar.precedence_level(Associativity::NonAssociative, 8, &["UNBOUNDED", "NESTED"]);
        grammar.precedence_level(
            Associativity::NonAssociative,
            9,
            &["id", "PRECEDING", "PATH"],
        );
        let (grammar, automaton) = build(grammar, "S");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            settled(&grammar, &automaton),
            [
                (
                    "Col -> UNBOUNDED".to_owned(),
                    "PRECEDING".to_owned(),
                    Resolution::Shift
                ),
                (
                    "Col -> NESTED".to_owned(),
                    "PATH".to_owned(),
                    Resolution::Shift
                ),
            ]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "UNBOUNDED PRECEDING"),
            ["Bound -> UNBOUNDED PRECEDING", "S -> Bound"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id PRECEDING"),
            [
                "Col -> id",
                "E -> Col",
                "Bound -> E PRECEDING",
                "S -> Bound"
            ]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "NESTED PATH string"),
            ["Column -> NESTED PATH string", "S -> Column"]
        );
    }

    /// `ROWS BETWEEN . - 1 PRECEDING`: `BETWEEN` opens a frame extent or names a column, and
    /// the token that decides is an operator whose precedence the `precedence` declaration
    /// carries for `Col -> BETWEEN` and for the arithmetic rules alike.
    /// gram.y: `%nonassoc BETWEEN ...` below `%left '+' '-'`.
    #[test]
    fn a_keyword_column_before_a_window_frame_shifts_the_frame_operator() {
        let mut grammar = TextbookGrammar::new(&[
            "id",
            "ROWS",
            "BETWEEN",
            "AND",
            "PRECEDING",
            "CURRENT",
            "ROW",
            "-",
        ]);
        grammar.rule("Frame", "ROWS Extent");
        grammar.rule("Extent", "Bound");
        grammar.rule("Extent", "BETWEEN Bound AND Bound");
        grammar.rule("Bound", "E PRECEDING");
        grammar.rule("Bound", "CURRENT ROW");
        grammar.rule("E", "Col");
        grammar.rule("Col", "id");
        grammar.rule("Col", "BETWEEN");
        let negate = grammar.rule("E", "- E");
        grammar.rule("E", "E - E");
        grammar.precedence_level(Associativity::NonAssociative, 5, &["BETWEEN"]);
        grammar.precedence_level(Associativity::Left, 10, &["-"]);
        grammar.precedence_level(Associativity::Right, 12, &["UMINUS"]);
        // `a_expr: '-' a_expr %prec UMINUS`.
        grammar.rule_precedence(negate, "UMINUS");
        let (grammar, automaton) = build(grammar, "Frame");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        let settled = settled(&grammar, &automaton);
        assert!(
            settled.contains(&(
                "Col -> BETWEEN".to_owned(),
                "-".to_owned(),
                Resolution::Shift
            )),
            "{settled:?}"
        );
        assert_eq!(
            reductions(
                &grammar,
                &automaton,
                "ROWS BETWEEN - id PRECEDING AND CURRENT ROW"
            ),
            [
                "Col -> id",
                "E -> Col",
                "E -> - E",
                "Bound -> E PRECEDING",
                "Bound -> CURRENT ROW",
                "Extent -> BETWEEN Bound AND Bound",
                "Frame -> ROWS Extent"
            ]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "ROWS BETWEEN PRECEDING"),
            [
                "Col -> BETWEEN",
                "E -> Col",
                "Bound -> E PRECEDING",
                "Extent -> Bound",
                "Frame -> ROWS Extent"
            ]
        );
        // `- id - id` is `(- id) - id` because `%prec UMINUS` binds the prefix at 12, above
        // the infix `-` at 10.
        assert_eq!(
            reductions(&grammar, &automaton, "ROWS - id - id PRECEDING"),
            [
                "Col -> id",
                "E -> Col",
                "E -> - E",
                "Col -> id",
                "E -> Col",
                "E -> E - E",
                "Bound -> E PRECEDING",
                "Extent -> Bound",
                "Frame -> ROWS Extent"
            ]
        );
    }

    /// `c_expr: select_with_parens %prec UMINUS | select_with_parens indirection`: the
    /// parenthesised alternative has no terminal of its own, and the override gives it a level
    /// below `'.'`, so the indirection is shifted.
    #[test]
    fn a_prec_override_on_a_parenthesised_alternative_shifts_the_indirection() {
        let mut grammar = TextbookGrammar::new(&["id", "(", ")", "SELECT", "."]);
        grammar.rule("E", "id");
        grammar.rule("Sub", "( SELECT id )");
        let parenthesised = grammar.rule("E", "Sub");
        grammar.rule("E", "Sub . id");
        grammar.rule("E", "E . id");
        grammar.precedence_level(Associativity::Right, 18, &["UMINUS"]);
        grammar.precedence_level(Associativity::Left, 20, &["."]);
        grammar.rule_precedence(parenthesised, "UMINUS");
        let (grammar, automaton) = build(grammar, "E");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            settled(&grammar, &automaton),
            [("E -> Sub".to_owned(), ".".to_owned(), Resolution::Shift)]
        );
        let rule = grammar.rule(parenthesised);
        let precedence = rule
            .precedence
            .expect("the override gives the rule a precedence");
        assert_eq!(precedence.level, 18);
        assert_eq!(precedence.associativity, Associativity::Right);
        assert_eq!(
            reductions(&grammar, &automaton, "( SELECT id ) . id"),
            ["Sub -> ( SELECT id )", "E -> Sub . id"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "( SELECT id ) . id . id"),
            ["Sub -> ( SELECT id )", "E -> Sub . id", "E -> E . id"]
        );
    }

    #[test]
    fn without_the_override_the_parenthesised_alternative_is_a_conflict() {
        let mut grammar = TextbookGrammar::new(&["id", "(", ")", "SELECT", "."]);
        grammar.rule("E", "id");
        grammar.rule("Sub", "( SELECT id )");
        grammar.rule("E", "Sub");
        grammar.rule("E", "Sub . id");
        grammar.rule("E", "E . id");
        grammar.precedence_level(Associativity::Left, 20, &["."]);
        let (grammar, automaton) = build(grammar, "E");
        assert_eq!(automaton.conflicts().len(), 1);
        assert_eq!(automaton.conflicts()[0].lookahead, terminal(&grammar, "."));
    }

    /// `SET SESSION . CHARACTERISTICS`: the scope keyword reduces before an assignment whose
    /// variable may be the unreserved `characteristics`, or `CHARACTERISTICS` is shifted for
    /// the transaction form; two literal keywords with no operator hierarchy behind them.
    #[test]
    fn local_and_session_literals_below_characteristics_shift_the_transaction_form() {
        let mut grammar =
            TextbookGrammar::new(&["id", "SET", "LOCAL", "SESSION", "CHARACTERISTICS", "="]);
        grammar.rule("Stmt", "SET Scope Var = id");
        grammar.rule("Stmt", "SET SESSION CHARACTERISTICS");
        grammar.rule("Scope", "LOCAL");
        grammar.rule("Scope", "SESSION");
        grammar.rule("Var", "id");
        grammar.rule("Var", "CHARACTERISTICS");
        grammar.precedence_level(Associativity::NonAssociative, 1, &["LOCAL", "SESSION"]);
        grammar.precedence_level(Associativity::NonAssociative, 2, &["CHARACTERISTICS"]);
        let (grammar, automaton) = build(grammar, "Stmt");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            settled(&grammar, &automaton),
            [(
                "Scope -> SESSION".to_owned(),
                "CHARACTERISTICS".to_owned(),
                Resolution::Shift
            )]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "SET SESSION CHARACTERISTICS"),
            ["Stmt -> SET SESSION CHARACTERISTICS"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "SET LOCAL CHARACTERISTICS = id"),
            [
                "Scope -> LOCAL",
                "Var -> CHARACTERISTICS",
                "Stmt -> SET Scope Var = id"
            ]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "SET SESSION id = id"),
            [
                "Scope -> SESSION",
                "Var -> id",
                "Stmt -> SET Scope Var = id"
            ]
        );
    }

    #[test]
    fn a_nonassoc_tie_is_an_error_action_recorded_in_the_dump() {
        let mut grammar = TextbookGrammar::new(&["id", "<"]);
        grammar.rule("E", "id");
        grammar.rule("E", "E < E");
        grammar.precedence_level(Associativity::NonAssociative, 7, &["<"]);
        let (grammar, automaton) = build(grammar, "E");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            settled(&grammar, &automaton),
            [("E -> E < E".to_owned(), "<".to_owned(), Resolution::Error)]
        );
        let dump = automaton_dump(&grammar, &automaton);
        assert!(
            dump.contains("resolved by precedence (an error)."),
            "{dump}"
        );
        assert!(dump.contains("error (nonassociative)"), "{dump}");
        assert_eq!(
            reductions(&grammar, &automaton, "id < id"),
            ["E -> id", "E -> id", "E -> E < E"]
        );
        let error = parse(&grammar, &automaton, "id < id < id").unwrap_err();
        assert_eq!(error.position, 3);
    }

    #[test]
    fn a_left_or_right_level_settles_a_tie_the_way_bison_does() {
        let mut grammar = TextbookGrammar::new(&["id", "+", "^"]);
        grammar.rule("E", "id");
        grammar.rule("E", "E + E");
        grammar.rule("E", "E ^ E");
        grammar.precedence_level(Associativity::Left, 10, &["+"]);
        grammar.precedence_level(Associativity::Right, 30, &["^"]);
        let (grammar, automaton) = build(grammar, "E");
        assert!(
            automaton.conflicts().is_empty(),
            "{:?}",
            automaton.conflicts()
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id + id + id"),
            ["E -> id", "E -> id", "E -> E + E", "E -> id", "E -> E + E"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id ^ id ^ id"),
            ["E -> id", "E -> id", "E -> id", "E -> E ^ E", "E -> E ^ E"]
        );
        assert_eq!(
            reductions(&grammar, &automaton, "id + id ^ id"),
            ["E -> id", "E -> id", "E -> id", "E -> E ^ E", "E -> E + E"]
        );
    }

    #[test]
    fn a_rule_takes_the_precedence_of_its_last_declared_terminal() {
        let mut grammar = TextbookGrammar::new(&["a", "b", "c", "d"]);
        let both = grammar.rule("X", "a b Y c d");
        let none = grammar.rule("X", "Y a");
        let epsilon = grammar.rule("Y", "");
        let overridden = grammar.rule("X", "X b X");
        grammar.precedence_level(Associativity::Left, 1, &["b"]);
        grammar.precedence_level(Associativity::Right, 2, &["c"]);
        grammar.precedence_level(Associativity::Left, 40, &["HIGH"]);
        grammar.rule_precedence(overridden, "HIGH");
        grammar.assign_declared_precedence();
        grammar.start("X");
        let grammar = grammar.finish();
        let level = |rule: RuleId| {
            grammar
                .rule(rule)
                .precedence
                .map(|precedence| (precedence.level, precedence.associativity))
        };
        // `d` has no declared precedence, so the last declared terminal is `c`.
        assert_eq!(level(both), Some((2, Associativity::Right)));
        assert_eq!(level(none), None);
        assert_eq!(level(epsilon), None);
        // The override wins over the rule's own last declared terminal `b`.
        assert_eq!(level(overridden), Some((40, Associativity::Left)));
        assert_eq!(grammar.declared_precedence(terminal(&grammar, "d")), None);
    }

    #[test]
    fn reduce_reduce_is_never_resolved_by_precedence() {
        let mut grammar = TextbookGrammar::new(&["a", "b"]);
        grammar.rule("S", "X b");
        grammar.rule("S", "Y b");
        grammar.rule("X", "a");
        grammar.rule("Y", "a");
        grammar.precedence_level(Associativity::NonAssociative, 1, &["a"]);
        grammar.precedence_level(Associativity::NonAssociative, 2, &["b"]);
        let (grammar, automaton) = build(grammar, "S");
        assert_eq!(automaton.conflicts().len(), 1);
        assert!(matches!(
            automaton.conflicts()[0].kind,
            ConflictKind::ReduceReduce { .. }
        ));
        assert_eq!(automaton.conflicts()[0].lookahead, terminal(&grammar, "b"));
        assert!(automaton.resolved_conflicts().is_empty());
    }

    #[test]
    fn an_undeclared_lookahead_leaves_the_conflict() {
        let mut grammar = TextbookGrammar::new(&["id", "x", "y"]);
        grammar.rule("S", "E");
        grammar.rule("S", "E y");
        grammar.rule("E", "id");
        grammar.rule("E", "E x");
        grammar.rule("E", "E x y");
        grammar.precedence_level(Associativity::Left, 1, &["x"]);
        let (grammar, automaton) = build(grammar, "S");
        assert_eq!(automaton.conflicts().len(), 1);
        assert_eq!(automaton.conflicts()[0].lookahead, terminal(&grammar, "y"));
        assert!(automaton.resolved_conflicts().is_empty());
    }

    #[test]
    fn assignment_is_idempotent_and_keeps_overrides() {
        let mut grammar = TextbookGrammar::new(&["a", "b"]);
        let rule = grammar.rule("X", "a b");
        grammar.precedence_level(Associativity::Left, 1, &["a"]);
        grammar.precedence_level(Associativity::Right, 2, &["b", "LOW"]);
        grammar.rule_precedence(rule, "LOW");
        grammar.assign_declared_precedence();
        grammar.start("X");
        let mut grammar = grammar.finish();
        assign_rule_precedence(&mut grammar);
        let precedence = grammar.rule(rule).precedence.unwrap();
        assert_eq!(
            (precedence.level, precedence.associativity),
            (2, Associativity::Right)
        );
    }
}
