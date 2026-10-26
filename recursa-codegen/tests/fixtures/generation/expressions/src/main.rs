//! Drives the declared-precedence expression grammar through the generated parser and prints
//! one tagged line per input: the parsed shape for every accepted input, and the diagnostic for
//! every rejected one. The integration test pins each line.

use recursa::{Arbitrary, Pretty, PrettyConfig};
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
    // A keyword atom: its token is an element of the rule, not syntax attached to an operand.
    "null + a",
    // The runtime `tail` flag: an operator rule is a left-recursive continuation of its own
    // Node, so its reduce leaves the following token to whatever encloses the expression and
    // the parsed prefix stays acceptable. `a + b` is accepted with input left over.
    "a + b c",
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
    // The runtime `tail` flag again: an atom is a one-token alternative of an enum Node, whose
    // reduce does end the root, so a token outside the root FOLLOW is an error.
    "1 2",
];

/// Renders a parsed expression as a compact prefix form so the assertions read as grammar.
fn shape(expression: &grammar::Expr<'_>) -> String {
    match expression {
        grammar::Expr::Integer(integer) => integer.text.text().to_owned(),
        grammar::Expr::Name(name) => name.text.text().to_owned(),
        grammar::Expr::Null => "null".to_owned(),
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
        grammar::Expr::Abs(operand) => format!("abs({})", shape(operand)),
        grammar::Expr::Center(operand) => format!("center({})", shape(operand)),
        grammar::Expr::Quant(operand, suffix) => format!(
            "quant({}, {} {})",
            shape(operand),
            match suffix.op {
                grammar::QuantOp::Star => "*",
                grammar::QuantOp::Eq => "=",
            },
            suffix.name.text.text()
        ),
    }
}

/// Parses one complete expression.
fn parse_expr(source: &str) -> grammar::Expr<'_> {
    let lexed = grammar::lex(source);
    assert!(lexed.errors().next().is_none(), "{source:?} lexes");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .unwrap_or_else(|error| panic!("{source:?} parses: {error}"));
    assert!(input.is_eof(), "{source:?} is fully consumed");
    parsed.into_ast()
}

/// Reports whether a value is in the language of the restricted `BExpr`: an admitted variant,
/// whose retyped operands (the first and the last symbol of its rule) are `BExpr` again.
fn in_b_expr(expression: &grammar::Expr<'_>) -> bool {
    use grammar::Expr;
    match expression {
        Expr::Integer(_) | Expr::Name(_) | Expr::Null | Expr::Parenthesized(_) => true,
        Expr::Negate(operand) | Expr::Factorial(operand) => in_b_expr(operand),
        Expr::Power(left, right)
        | Expr::Multiply(left, right)
        | Expr::Add(left, right)
        | Expr::Subtract(left, right) => in_b_expr(left) && in_b_expr(right),
        _ => false,
    }
}

/// Checks that every `BETWEEN` middle operand of a sampled value is a `BExpr`.
fn check_restricted(expression: &grammar::Expr<'_>) {
    use grammar::Expr;
    match expression {
        Expr::Integer(_) | Expr::Name(_) | Expr::Null => {}
        Expr::Parenthesized(operand)
        | Expr::Negate(operand)
        | Expr::Factorial(operand)
        | Expr::Abs(operand)
        | Expr::Center(operand)
        | Expr::Quant(operand, _)
        | Expr::LikeEscape(operand, _, _) => check_restricted(operand),
        Expr::Power(left, right)
        | Expr::Multiply(left, right)
        | Expr::Add(left, right)
        | Expr::Subtract(left, right)
        | Expr::And(left, right)
        | Expr::Equals(left, right)
        | Expr::Like(left, right) => {
            check_restricted(left);
            check_restricted(right);
        }
        Expr::Between(low, middle, high) => {
            assert!(in_b_expr(middle), "a sampled BETWEEN operand is a BExpr: {}", shape(middle));
            check_restricted(low);
            check_restricted(middle);
            check_restricted(high);
        }
    }
}

/// Renders a value, parses the rendering, and checks that the parse renders the same text and
/// has the same shape. Returns the rendering.
fn round_trip(expression: &grammar::Expr<'_>, config: PrettyConfig) -> String {
    let rendered = expression.render_with(config);
    let lexed = grammar::lex(&rendered);
    assert!(lexed.errors().next().is_none(), "{rendered:?} lexes");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .unwrap_or_else(|error| panic!("{rendered:?} parses: {error}"));
    assert!(input.is_eof(), "{rendered:?} is fully consumed");
    assert_eq!(parsed.render_with(config), rendered, "the rendering is a fixpoint");
    let ast = parsed.into_ast();
    assert_eq!(
        without_groups(&shape(&ast)),
        without_groups(&shape(expression)),
        "{rendered:?} parses to the rendered value"
    );
    rendered
}

/// Renders a sampled value, parses the rendering, and checks that the parse renders the same
/// text.
///
/// The shape is not compared: an interior operand that ends a shorter rule of the same Node,
/// such as the pattern of `LIKE ... ESCAPE` beside `LIKE`, has no precedence bound, so a sampled
/// value can render as text that parses to another tree with the same text.
fn fixpoint(expression: &grammar::Expr<'_>, config: PrettyConfig) {
    let rendered = expression.render_with(config);
    let lexed = grammar::lex(&rendered);
    assert!(lexed.errors().next().is_none(), "{rendered:?} lexes");
    let mut input = lexed.input();
    let parsed = grammar::Expr::parse(&mut input)
        .unwrap_or_else(|error| panic!("{rendered:?} parses: {error}"));
    assert!(input.is_eof(), "{rendered:?} is fully consumed");
    assert_eq!(parsed.render_with(config), rendered, "the rendering is a fixpoint");
}

/// Removes every `paren(...)` from a shape, so a value compares equal to the parse of its
/// rendering, which holds the parentheses `Pretty` wrote.
fn without_groups(shape: &str) -> String {
    let mut output = String::with_capacity(shape.len());
    let mut groups = Vec::new();
    let mut rest = shape;
    while let Some(character) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("paren(") {
            groups.push(true);
            rest = after;
            continue;
        }
        match character {
            '(' => {
                groups.push(false);
                output.push(character);
            }
            ')' => {
                if groups.pop() != Some(true) {
                    output.push(character);
                }
            }
            _ => output.push(character),
        }
        rest = &rest[character.len_utf8()..];
    }
    output
}

/// The bytes of one sample, from a SplitMix64 stream, so each sample reproduces alone.
fn entropy(seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut bytes = Vec::with_capacity(128);
    while bytes.len() < 128 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        bytes.extend_from_slice(&z.to_le_bytes());
    }
    bytes
}

/// Proves that `Pretty` writes the parentheses and the spaces that a parse needs, for values
/// the parser cannot build, and that `Arbitrary` samples only values that parse back.
fn render_repairs() {
    use grammar::Expr;
    let boxed = |source: &'static str| Box::new(parse_expr(source));
    let name = |source: &'static str| match parse_expr(source) {
        Expr::Name(name) => name,
        _ => unreachable!("a name parses as a name"),
    };
    let config = PrettyConfig::default();
    // A value at a restricted site that its restriction refuses is parenthesized.
    let between = Expr::Between(
        boxed("a"),
        Box::new(Expr::And(boxed("b"), boxed("c"))),
        boxed("d"),
    );
    println!("repair {:?}", round_trip(&between, config));
    // The restriction applies again on the retyped edge of an admitted variant.
    let between = Expr::Between(
        boxed("a"),
        Box::new(Expr::Add(boxed("b"), Box::new(Expr::And(boxed("c"), boxed("e"))))),
        boxed("d"),
    );
    println!("repair {:?}", round_trip(&between, config));
    // The token after the first operand decides, not the level of the rule.
    let star = |operand: Expr<'static>| {
        Expr::Quant(
            Box::new(operand),
            grammar::QuantSuffix {
                op: grammar::QuantOp::Star,
                name: name("q"),
            },
        )
    };
    let equals = |operand: Expr<'static>| {
        Expr::Quant(
            Box::new(operand),
            grammar::QuantSuffix {
                op: grammar::QuantOp::Eq,
                name: name("q"),
            },
        )
    };
    println!(
        "repair {:?}",
        round_trip(&star(Expr::Add(boxed("a"), boxed("b"))), config)
    );
    println!(
        "repair {:?}",
        round_trip(&Expr::Add(boxed("a"), Box::new(star(parse_expr("b")))), config)
    );
    println!(
        "repair {:?}",
        round_trip(&Expr::Like(boxed("a"), Box::new(equals(parse_expr("b")))), config)
    );
    println!(
        "repair {:?}",
        round_trip(&equals(Expr::Like(boxed("a"), boxed("b"))), config)
    );
    // Two texts that the lexer would join get a space.
    println!(
        "repair {:?}",
        round_trip(&Expr::Abs(Box::new(Expr::Abs(boxed("a")))), config)
    );
    println!("repair {:?}", round_trip(&Expr::Center(boxed("a")), config));
    println!(
        "repair {:?}",
        round_trip(&Expr::Negate(Box::new(Expr::Negate(boxed("a")))), config)
    );
    // Synthesized parentheses break as the parsed atom breaks.
    let mut sum = parse_expr("a1");
    for index in 2..=12 {
        let operand = match index {
            2 => "a2",
            3 => "a3",
            4 => "a4",
            5 => "a5",
            6 => "a6",
            7 => "a7",
            8 => "a8",
            9 => "a9",
            10 => "a10",
            11 => "a11",
            _ => "a12",
        };
        sum = Expr::Add(Box::new(sum), boxed(operand));
    }
    let product = Expr::Multiply(Box::new(sum), boxed("b"));
    println!(
        "repair {:?}",
        round_trip(&product, PrettyConfig::new(20, 2))
    );
    // Every sampled value parses back to itself.
    let mut checked = 0_usize;
    for seed in 0..2_000_u64 {
        let bytes = entropy(seed);
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        let Ok(sampled) = <Expr<'static> as Arbitrary>::arbitrary(&mut input) else {
            continue;
        };
        check_restricted(&sampled);
        fixpoint(&sampled, config);
        fixpoint(&sampled, PrettyConfig::new(12, 2));
        checked += 1;
    }
    assert!(checked > 1_000, "the sampler yields values: {checked}");
    println!("arbitrary round trip ok");
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
    render_repairs();
    println!("expressions-ok");
}
