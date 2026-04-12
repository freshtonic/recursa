//! SQL pretty-printer using Visitor<N> + TotalVisitor for type-safe dispatch.
//!
//! Parent AST nodes emit their own keyword tokens in enter/exit.
//! Data-carrying children (identifiers, literals) emit their values.
//! Keywords and punctuation stored as PhantomData are invisible to the
//! visitor — the parent handles them.

use std::ops::ControlFlow;

use recursa::fmt::{FormatStyle, GroupKind, PrintEngine, Token};
use recursa::{Break, TotalVisitor, Visit, Visitor};

use crate::ast::select::{FromClause, OrderByClause, SelectItem, SelectStmt, WhereClause};
use crate::ast::{PsqlCommand, Statement, TerminatedStatement};
use crate::tokens::literal;

/// Format an AST into pretty-printed SQL.
pub fn format_sql(root: &impl Visit, style: FormatStyle) -> String {
    let mut formatter = SqlFormatter {
        tokens: Vec::new(),
        style: style.clone(),
    };
    let _ = root.visit(&mut formatter);
    let engine = PrintEngine::new(style);
    engine.print(&formatter.tokens)
}

#[derive(TotalVisitor)]
#[total_visitor(
    dispatch = [
        PsqlCommand,
        TerminatedStatement,
        Statement,
        SelectStmt,
        SelectItem,
        FromClause,
        WhereClause,
        OrderByClause,
        literal::Ident,
        literal::AliasName,
        literal::StringLit,
        literal::IntegerLit,
    ],
    error = ()
)]
struct SqlFormatter {
    tokens: Vec<Token>,
    style: FormatStyle,
}

impl SqlFormatter {
    fn push(&mut self, s: impl Into<String>) {
        self.tokens.push(Token::String(s.into()));
    }

    fn softline(&mut self) {
        self.tokens.push(Token::Break {
            flat: " ".into(),
            broken: "\n".into(),
        });
    }

    fn hardline(&mut self) {
        self.tokens.push(Token::Break {
            flat: "\n".into(),
            broken: "\n".into(),
        });
    }

    fn begin(&mut self, kind: GroupKind) {
        self.tokens.push(Token::Begin(kind));
    }

    fn end(&mut self) {
        self.tokens.push(Token::End);
    }

    fn indent(&mut self) {
        self.tokens.push(Token::Indent);
    }

    fn dedent(&mut self) {
        self.tokens.push(Token::Dedent);
    }

    fn keyword(&mut self, kw: &str) {
        if self.style.uppercase_keywords {
            self.push(kw.to_uppercase());
        } else {
            self.push(kw.to_lowercase());
        }
    }
}

// --- Containers ---

impl Visitor<PsqlCommand> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &PsqlCommand) -> ControlFlow<Break<()>> {
        ControlFlow::Continue(())
    }
    fn exit(&mut self, _node: &PsqlCommand) -> ControlFlow<Break<()>> {
        self.hardline();
        ControlFlow::Continue(())
    }
}

impl Visitor<TerminatedStatement> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &TerminatedStatement) -> ControlFlow<Break<()>> {
        ControlFlow::Continue(())
    }
    fn exit(&mut self, _node: &TerminatedStatement) -> ControlFlow<Break<()>> {
        self.push(";");
        ControlFlow::Continue(())
    }
}

impl Visitor<Statement> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &Statement) -> ControlFlow<Break<()>> {
        ControlFlow::Continue(())
    }
}

// --- SELECT ---

impl Visitor<SelectStmt> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, node: &SelectStmt) -> ControlFlow<Break<()>> {
        eprintln!("FORMATTER: SelectStmt enter, items count: {}", node.items.len());
        self.begin(GroupKind::Consistent);
        self.keyword("SELECT");
        self.indent();
        self.softline();
        ControlFlow::Continue(())
    }
    fn exit(&mut self, _node: &SelectStmt) -> ControlFlow<Break<()>> {
        self.dedent();
        self.end();
        ControlFlow::Continue(())
    }
}

impl Visitor<SelectItem> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &SelectItem) -> ControlFlow<Break<()>> {
        ControlFlow::Continue(())
    }
}

impl Visitor<FromClause> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &FromClause) -> ControlFlow<Break<()>> {
        self.dedent();
        self.softline();
        self.keyword("FROM");
        self.push(" ");
        self.indent();
        ControlFlow::Continue(())
    }
    fn exit(&mut self, _node: &FromClause) -> ControlFlow<Break<()>> {
        self.dedent();
        ControlFlow::Continue(())
    }
}

impl Visitor<WhereClause> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &WhereClause) -> ControlFlow<Break<()>> {
        self.softline();
        self.keyword("WHERE");
        self.push(" ");
        ControlFlow::Continue(())
    }
}

impl Visitor<OrderByClause> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, _node: &OrderByClause) -> ControlFlow<Break<()>> {
        self.softline();
        self.keyword("ORDER BY");
        self.push(" ");
        ControlFlow::Continue(())
    }
}

// --- Data-carrying tokens ---

impl Visitor<literal::Ident> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, node: &literal::Ident) -> ControlFlow<Break<()>> {
        self.push(&node.0);
        ControlFlow::Continue(())
    }
}

impl Visitor<literal::AliasName> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, node: &literal::AliasName) -> ControlFlow<Break<()>> {
        self.push(&node.0);
        ControlFlow::Continue(())
    }
}

impl Visitor<literal::StringLit> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, node: &literal::StringLit) -> ControlFlow<Break<()>> {
        self.push(&node.0);
        ControlFlow::Continue(())
    }
}

impl Visitor<literal::IntegerLit> for SqlFormatter {
    type Error = ();
    fn enter(&mut self, node: &literal::IntegerLit) -> ControlFlow<Break<()>> {
        eprintln!("FORMATTER: IntegerLit enter: {}", &node.0);
        self.push(&node.0);
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use super::*;
    use crate::ast::parse_sql_file;
    use crate::rules::SqlRules;

    fn format(sql: &str) -> String {
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        format_sql(&commands[0], FormatStyle::default())
    }

    #[test]
    fn format_simple_select() {
        let result = format("select 1 as one;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("one"), "got: {result}");
    }

    #[test]
    fn format_select_star_from() {
        let result = format("select * from t;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("FROM"), "got: {result}");
        assert!(result.contains("t"), "got: {result}");
    }

    #[test]
    fn format_select_where() {
        let result = format("select a from t where a = 1;");
        eprintln!("format_select_where got: {result}");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("FROM"), "got: {result}");
        assert!(result.contains("WHERE"), "got: {result}");
    }

    #[test]
    fn format_uppercase_keywords() {
        let result = format("select a from t;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("FROM"), "got: {result}");
    }

    #[test]
    fn format_lowercase_keywords() {
        let mut input = Input::new("SELECT a FROM t;");
        let commands = parse_sql_file(&mut input).unwrap();
        let style = FormatStyle {
            uppercase_keywords: false,
            ..Default::default()
        };
        let result = format_sql(&commands[0], style);
        assert!(result.contains("select"), "got: {result}");
        assert!(result.contains("from"), "got: {result}");
    }
}
