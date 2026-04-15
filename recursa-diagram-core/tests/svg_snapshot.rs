//! Snapshot fixture for a realistic SELECT-statement railroad tree.
//!
//! This is a golden-file test: the generated SVG is compared byte-for-byte
//! against a committed fixture. To regenerate after an intentional change,
//! run the test with `UPDATE_SNAPSHOTS=1`.

use std::fs;
use std::path::PathBuf;

use recursa_diagram_core::{
    layout::{Choice, Node, NonTerminal, OneOrMore, Optional, Sequence, Terminal, zero_or_more},
    render,
};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("select_stmt.svg")
}

/// Build a representative railroad tree for a minimal SELECT statement:
///
/// ```text
/// SELECT Column (, Column)*  [FromClause]
/// ```
///
/// Uses every composite variant: Sequence, OneOrMore (with separator),
/// Optional, and a Choice for Column alternatives.
fn select_stmt_tree() -> Node {
    // Column := identifier | expression
    let column = Node::Choice(Choice::new(
        0,
        vec![
            Node::NonTerminal(NonTerminal::new(
                "identifier",
                Some("identifier.html".into()),
            )),
            Node::NonTerminal(NonTerminal::new(
                "expression",
                Some("expression.html".into()),
            )),
        ],
    ));

    // Columns := Column (, Column)*
    let columns = Node::OneOrMore(OneOrMore::new(
        column,
        Some(Node::Terminal(Terminal::new(","))),
    ));

    // FromClause := FROM table (, table)*
    let from_clause = Node::NonTerminal(NonTerminal::new(
        "FromClause",
        Some("FromClause.html".into()),
    ));

    // Full: SELECT Columns [FromClause] [WHERE expression]*
    Node::Sequence(Sequence::new(vec![
        Node::Terminal(Terminal::new("SELECT")),
        columns,
        Node::Optional(Optional::new(from_clause)),
        // Throw in a zero-or-more to exercise Optional(OneOrMore(..)).
        zero_or_more(
            Node::NonTerminal(NonTerminal::new("WhereClause", None)),
            None,
        ),
    ]))
}

#[test]
fn select_stmt_matches_snapshot() {
    let svg = render(&select_stmt_tree());
    let path = fixture_path();

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture dir");
        }
        fs::write(&path, &svg).expect("write snapshot");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing snapshot at {}: {e}. Run with UPDATE_SNAPSHOTS=1 to create it.",
            path.display()
        )
    });
    assert_eq!(
        svg, expected,
        "SELECT-statement snapshot diverged. If the change is intentional, re-run with UPDATE_SNAPSHOTS=1."
    );
}
