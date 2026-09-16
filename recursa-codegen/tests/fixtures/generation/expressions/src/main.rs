//! Drives the declared-precedence expression grammar through the generated parser and prints
//! one tagged line per input: the parsed shape for every accepted input, and the diagnostic for
//! every rejected one. The integration test pins each line.

use recursa_generated_expressions::grammar;

/// Inputs the parser accepts, grouped by the fact each one proves.
const ACCEPTED: &[&str] = &[
    // The `UMINUS` override: without it `MINUS` would give the prefix rule the additive level
    // and `- a * b` would negate the product.
    "- a * b",
    "- a + b",
    "- a ^ b",
    "- a !",
    // Left, right and nonassociative infix levels.
    "a + b + c",
    "a + b * c",
    "a ^ b ^ c",
    "a = b + c",
    // The postfix operator, above every infix level.
    "a !",
    "a ^ b !",
    "a ! !",
    // The restricted interior operand of `BETWEEN` against boolean `AND`.
    "a and b",
    "a between b and c",
    "a between b + c and d",
    "a between ( b and c ) and d",
    "a between b and c and d",
    // `ESCAPE` one level above `LIKE`: the clause is shifted, and a token below `LIKE`
    // reduces the comparison first.
    "a like b escape c",
    "a like b",
    "a like b and c",
    "a like b + c escape d",
];

/// Inputs the parser rejects, with the reason each one is a decision of the declaration.
const REJECTED: &[&str] = &[
    // `EQ` is nonassociative, so the tie makes the cell an error action.
    "a = b = c",
    // `LIKE` is nonassociative too.
    "a like b like c",
    // `ESCAPE` is shifted, so the clause must be completed.
    "a like b escape",
    // The restricted operand admits no boolean `AND`, so this `and` closes the clause and the
    // second one has no right operand.
    "a between b and",
    "1 2",
];

/// Renders a parsed expression as a compact prefix form so the assertions read as grammar.
fn shape(expression: &grammar::Expr<'_>) -> String {
    match expression {
        grammar::Expr::Integer(integer) => integer.text.text().to_owned(),
        grammar::Expr::Name(name) => name.text.text().to_owned(),
        grammar::Expr::Parenthesized(inner) => format!("paren({})", shape(inner)),
        grammar::Expr::Negate(operand) => format!("neg({})", shape(operand)),
        grammar::Expr::Factorial(operand) => format!("fac({})", shape(operand)),
        grammar::Expr::Power(left, right) => binary("pow", left, right),
        grammar::Expr::Multiply(left, right) => binary("mul", left, right),
        grammar::Expr::Add(left, right) => binary("add", left, right),
        grammar::Expr::Subtract(left, right) => binary("sub", left, right),
        grammar::Expr::And(left, right) => binary("and", left, right),
        grammar::Expr::Equals(left, right) => binary("eq", left, right),
        grammar::Expr::Like(left, right) => binary("like", left, right),
        grammar::Expr::LikeEscape(left, right, escape) => format!(
            "like({}, {}, escape {})",
            shape(left),
            shape(right),
            escape.text.text()
        ),
        grammar::Expr::Between(low, middle, high) => format!(
            "between({}, {}, {})",
            shape(low),
            shape(middle),
            shape(high)
        ),
    }
}

/// Renders one two-operand variant.
fn binary(name: &str, left: &grammar::Expr<'_>, right: &grammar::Expr<'_>) -> String {
    format!("{name}({}, {})", shape(left), shape(right))
}

fn main() {
    for source in ACCEPTED {
        let lexed = grammar::lex(source);
        assert!(lexed.errors().next().is_none(), "{source} lexes");
        let mut input = lexed.input();
        let parsed = grammar::Expr::parse(&mut input)
            .unwrap_or_else(|error| panic!("{source} parses: {error}"));
        let ast = parsed.into_ast();
        println!(
            "lr ok {source:?} => {} eof={}",
            shape(&ast),
            input.is_eof()
        );
    }
    for source in REJECTED {
        let lexed = grammar::lex(source);
        assert!(lexed.errors().next().is_none(), "{source} lexes");
        let mut input = lexed.input();
        match grammar::Expr::parse(&mut input) {
            Ok(parsed) => {
                let ast = parsed.into_ast();
                println!(
                    "lr unexpected {source:?} => {} eof={}",
                    shape(&ast),
                    input.is_eof()
                );
            }
            Err(error) => println!(
                "lr err {source:?} => code={} kind={:?} found={:?}",
                error.code(),
                error.kind(),
                error.found()
            ),
        }
    }
    println!("expressions-ok");
}
