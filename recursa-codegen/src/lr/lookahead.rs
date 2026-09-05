//! Lookahead-filter substitution (`docs/table-driven-parsing.md`, section 5).
//!
//! Placeholder: the substitution pass lands with its tests.

#[cfg(test)]
mod tests {
    //! The lookahead-filter tests: the normalized grammar carries the declaration, the
    //! PostgreSQL `NOT BETWEEN` shape conflicts without the filter and not with it, an ambiguous
    //! position is `RCA0404`, and a token at the end of a rule follows the FOLLOW rule.

    use crate::materialization::LoweredGrammar;
    use crate::source_analysis::{SourceAnalysisInput, analyze_source_tree};

    /// The `tokens!` declaration of the `NOT BETWEEN` fixture, with or without the filter.
    fn tokens(lookahead: bool) -> String {
        let filter = if lookahead {
            "    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }\n"
        } else {
            ""
        };
        format!(
            r#"
recursa::tokens! {{
    ignore = r"[ \t\n\r]+",
    keywords {{
        NOT => "not",
        BETWEEN => "between",
        AND => "and",
        IN => "in",
        LIKE => "like",
    }}
    punctuation {{
        LPAREN => "(",
        RPAREN => ")",
        SEMI => ";",
    }}
{filter}}}
"#
        )
    }

    /// The `NOT BETWEEN` fixture: unary `NOT cond` beside `name NOT BETWEEN`, `NOT IN` and
    /// `NOT LIKE`, with two conditions juxtaposed so that `NOT` can follow a condition.
    fn not_between_grammar(lookahead: bool) -> String {
        format!(
            r#"
recursa::grammar! {{ module = crate, keyword_matching = sensitive, max_lookahead = 3 }}
{}
#[derive(recursa::Node)]
#[tok(this, SEMI)]
pub struct Rule<'input> {{
    pub guard: Cond<'input>,
    pub action: Cond<'input>,
}}

#[derive(recursa::Node)]
pub enum Cond<'input> {{
    Not {{ mark: NotMark, inner: Box<Cond<'input>> }},
    Test(Test<'input>),
}}

#[derive(recursa::Node)]
#[tok(NOT)]
pub struct NotMark;

#[derive(recursa::Node)]
pub enum Test<'input> {{
    NotBetween(NotBetween<'input>),
    NotIn(NotIn<'input>),
    NotLike(NotLike<'input>),
    Between(Between<'input>),
    Name {{ name: Name<'input> }},
}}

#[derive(recursa::Node)]
pub struct NotBetween<'input> {{
    pub subject: Name<'input>,
    #[tok(NOT, BETWEEN)]
    pub low: Number<'input>,
    #[tok(AND)]
    pub high: Number<'input>,
}}

#[derive(recursa::Node)]
pub struct NotIn<'input> {{
    pub subject: Name<'input>,
    #[tok(NOT, IN, LPAREN)]
    pub member: Number<'input>,
    #[tok(RPAREN)]
    pub close: Close,
}}

#[derive(recursa::Node)]
#[tok(RPAREN)]
pub struct Close;

#[derive(recursa::Node)]
pub struct NotLike<'input> {{
    pub subject: Name<'input>,
    #[tok(NOT, LIKE)]
    pub pattern: Name<'input>,
}}

#[derive(recursa::Node)]
pub struct Between<'input> {{
    pub subject: Name<'input>,
    #[tok(BETWEEN)]
    pub low: Number<'input>,
    #[tok(AND)]
    pub high: Number<'input>,
}}

#[derive(recursa::Node)]
pub struct Name<'input> {{
    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
    pub text: NameText<'input>,
}}

#[derive(recursa::Node)]
pub struct Number<'input> {{
    #[lex(pattern = r"[0-9]+")]
    pub text: NumberText<'input>,
}}
"#,
            tokens(lookahead)
        )
    }

    /// Runs source analysis over one inline grammar.
    fn lowered(source: &str) -> LoweredGrammar {
        let package = tempfile::tempdir().expect("temporary lookahead fixture");
        std::fs::write(package.path().join("grammar.rs"), source).expect("write fixture");
        analyze_source_tree(
            SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("fixture failed source analysis:\n{error}"))
        .analyzed
        .lowered
    }

    #[test]
    fn the_normalized_grammar_carries_the_filter_and_its_merged_kind() {
        let lowered = lowered(&not_between_grammar(true));
        let grammar = &lowered.grammar;
        let kind = |authored: &str| {
            grammar
                .kinds
                .iter()
                .position(|kind| kind.authored.as_deref() == Some(authored))
                .map(|index| crate::KindId::new(index as u16))
                .unwrap_or_else(|| panic!("no kind authored as {authored}"))
        };
        assert_eq!(
            grammar.lookahead_filters,
            [crate::LookaheadFilter {
                merged: kind("NOT_LA"),
                token: kind("NOT"),
                triggers: vec![kind("BETWEEN"), kind("IN"), kind("LIKE")],
            }]
        );
        // The merged kind is the last kind and no grammar type matches it.
        assert_eq!(kind("NOT_LA").index(), grammar.kinds.len() - 1);
        assert_eq!(grammar.kinds[kind("NOT_LA").index()].name, "merged:NOT_LA");
        assert!(grammar.types.iter().all(|description| match &description.shape {
            crate::TypeShape::Token { atom, .. } => {
                grammar.atoms[atom.index()].expression != crate::AtomExpression::Kind(kind("NOT_LA"))
            }
            _ => true,
        }));
    }
}
