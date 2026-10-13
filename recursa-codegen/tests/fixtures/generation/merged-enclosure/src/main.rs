//! Drives the merged-kind enclosure grammar and prints what the test compares: the Debug value
//! of every parse, which arm of the two-armed close the feed selected, the Pretty rendering of
//! each parse, and the rendering of a value reassembled against the declared associativity,
//! which only Pretty's precedence repair can parenthesize.

use recursa::Pretty as _;
use recursa_generated_merged_enclosure::grammar;

fn main() {
    for source in [
        // The merged arm: `)` before a set operator is read as `RPAREN_SELECT_LA`.
        "(a) union b;",
        "(a) except b;",
        "(a) intersect b;",
        "(a union b) except c;",
        // The plain arm: nothing a trigger follows.
        "(a);",
        "((a));",
        "a union (b);",
        // No parentheses at all, so the repair has nothing to reproduce.
        "a union b union c;",
        "a union b intersect c;",
    ] {
        let lexed = grammar::lex(source);
        let kinds = lexed
            .tokens()
            .map(|token| token.kind().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let mut input = lexed.input();
        let parsed = grammar::Statement::parse(&mut input)
            .expect("positive input parses")
            .into_ast();
        println!("ok {source:?} => {parsed:?} kinds=[{kinds}]");
        println!("pretty {source:?} => {:?}", parsed.render());
    }

    // The merged kind exists, is never lexed, and spells like the token it merges.
    let merged = grammar::TokenKind::RPAREN_SELECT_LA;
    println!(
        "merged kind {merged:?} displays as {merged} like {}",
        grammar::TokenKind::RPAREN
    );
    let lexed = grammar::lex("(a) union b;");
    let lexed_kinds = lexed.tokens().map(|token| token.kind()).collect::<Vec<_>>();
    assert!(
        !lexed_kinds.contains(&merged),
        "a lexing result never holds the merged kind: {lexed_kinds:?}"
    );

    // Pretty's precedence repair: `union` is left associative, so the parser never builds a
    // right-nested chain and no parenthesized Node stands in the value. Reassembling the parsed
    // operands right-nested leaves a value whose rendering needs parentheses the AST does not
    // hold, which is exactly what the enclosure proof supplies.
    let lexed = grammar::lex("a union b union c;");
    let mut input = lexed.input();
    let statement = grammar::Statement::parse(&mut input)
        .expect("a left-nested chain parses")
        .into_ast();
    let grammar::Query::Union(left, c) = statement.query else {
        panic!("`union` is left associative, so the root is a union");
    };
    let grammar::Query::Union(a, b) = *left else {
        panic!("the left operand of a left-nested chain is a union");
    };
    let regrouped = grammar::Query::Union(a, Box::new(grammar::Query::Union(b, c)));
    println!("regrouped union => {:?}", regrouped.render());

    // The same repair one level down: `intersect` binds tighter than `union`, so a union as the
    // operand of an intersection is parenthesized.
    let lexed = grammar::lex("a union b;");
    let mut input = lexed.input();
    let statement = grammar::Statement::parse(&mut input)
        .expect("a union parses")
        .into_ast();
    let grammar::Query::Union(a, b) = statement.query else {
        panic!("the root is a union");
    };
    let union = grammar::Query::Union(a.clone(), b);
    let intersected = grammar::Query::Intersect(Box::new(union), a);
    println!("regrouped intersect => {:?}", intersected.render());

    // Every rendering the repair produces parses back, and to the same shape.
    for rendered in [regrouped.render(), intersected.render()] {
        let source = format!("{rendered};");
        let lexed = grammar::lex(&source);
        let mut input = lexed.input();
        let reparsed = grammar::Statement::parse(&mut input)
            .expect("a repaired rendering parses")
            .into_ast();
        println!("reparsed {source:?} => {reparsed:?}");
    }

    println!("merged-enclosure-ok");
}
