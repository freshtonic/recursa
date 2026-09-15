//! Drives the merged-kind precedence grammar and prints what the test compares for the LR parser
//! with and without `NOT_LA` in its `precedence` block:
//! the Debug value of every positive parse with the cursor it leaves, the Pretty rendering,
//! and for every failing input the error and the restored cursor.

use recursa::Pretty as _;
use recursa_generated_lookahead_precedence::grammar;

/// Drives every input through one parser: `$tag` prefixes each printed line and `$input`
/// builds that parser's input over a lexing result.
macro_rules! drive {
    ($tag:expr, $input:expr) => {{
        let tag: &str = $tag;
        for source in [
            // A frame extent and a single bound, neither of which needs a decision.
            "rows between a preceding and b preceding;",
            "rows a preceding;",
            // `between` and `like` as column names: the reduce.
            "rows between preceding;",
            "rows like preceding;",
            "rows between like preceding;",
            // The prefix `NOT` and the `NOT LIKE` test inside a bound.
            "rows not a preceding;",
            "rows a not like b preceding;",
            "rows between not a preceding and b preceding;",
            // `NOT` before a trigger read as a name: the prefix twin.
            "rows not like preceding;",
            "rows not between preceding;",
        ] {
            let lexed = grammar::lex(source);
            let kinds = lexed
                .tokens()
                .map(|token| token.kind().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let mut input = $input(&lexed);
            match grammar::Frame::parse(&mut input) {
                Ok(parsed) => println!(
                    "{tag} ok {source:?} => {:?} cursor={} eof={} kinds=[{kinds}]",
                    parsed.into_ast(),
                    input.cursor(),
                    input.is_eof()
                ),
                // A rejected frame prints no `found`: the two styles reject the same
                // inputs but discover it at different tokens, one by its decision table
                // and one by the action the declaration installed.
                Err(error) => println!(
                    "{tag} no {source:?} => code={} kind={:?} cursor={} kinds=[{kinds}]",
                    error.code(),
                    error.kind(),
                    input.cursor()
                ),
            }
        }

        // The Pretty rendering of a parse that both styles accept.
        let source = "rows between a preceding and b preceding;";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        let parsed = grammar::Frame::parse_spanned(&mut input).expect("the frame parses");
        println!("{tag} pretty {source:?} => {:?}", parsed.render());

        // The input the merged kind's level decides: `ROWS BETWEEN . NOT_LA`, where the
        // reduce takes `between` as the column a `not like` test follows and the shift takes
        // it as the frame keyword. gram.y's `%nonassoc` makes the tie an error, so the
        // The LR parser rejects it, as PostgreSQL does; the retired predictive parser reached the
        // same verdict by reading on.
        let source = "rows between not like a preceding;";
        let lexed = grammar::lex(source);
        let mut input = $input(&lexed);
        match grammar::Frame::parse(&mut input) {
            Ok(parsed) => println!("{tag} decision ok => {:?}", parsed.into_ast()),
            Err(error) => println!(
                "{tag} decision no => code={} kind={:?} cursor={}",
                error.code(),
                error.kind(),
                input.cursor()
            ),
        }

        // Failing inputs restore the cursor and name the lexed token, never the merged kind.
        for source in ["", "rows;", "rows between a preceding;", "rows a preceding"] {
            let lexed = grammar::lex(source);
            let mut input = $input(&lexed);
            let error = grammar::Frame::parse(&mut input).expect_err("failing input fails");
            println!(
                "{tag} err {source:?} => code={} kind={:?} found={:?} cursor={}",
                error.code(),
                error.kind(),
                error.found(),
                input.cursor()
            );
            assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
        }
    }};
}

/// Builds the sole generated parser input.
fn parser_input<'input>(lexed: &grammar::LexResult<'input>) -> grammar::Input<'input> {
    lexed.input()
}

fn main() {
    drive!("lr", parser_input);
    println!("lookahead-precedence-ok");
}
