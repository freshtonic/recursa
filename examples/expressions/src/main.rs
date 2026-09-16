use recursa_example_expressions::{grammar, parse};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "-a! + 2 * 3 ^ 4".to_owned());
    let expression = parse(&source)?;
    let root = match expression {
        grammar::Expr::Integer(_) => "integer",
        grammar::Expr::Name(_) => "name",
        grammar::Expr::Parenthesized(_) => "parenthesized expression",
        grammar::Expr::Negate(_) => "prefix negation",
        grammar::Expr::Factorial(_) => "postfix factorial",
        grammar::Expr::Power(_, _) => "power",
        grammar::Expr::Multiply(_, _) => "multiplication",
        grammar::Expr::Add(_, _) => "addition",
    };
    println!("parsed expression with {root} at the root");
    Ok(())
}
