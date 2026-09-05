//! Drives the Pratt fixture grammar and prints what the test compares between the two parser
//! styles: the Debug value of every positive parse with the cursor it leaves, and for every
//! failing input the error and the restored cursor.

use recursa_generated_table_driven_pratt::grammar;

fn main() {
    for source in [
        "1 - 2 - 3",
        "2 ^ 3 ^ 2",
        "-a!",
        "-2 ^ 2",
        "1 + 2 * 3",
        "1 * 2 + 3",
        "2 * 3 ^ 2!",
        "(1 + 2) * 3",
        "a! ^ 2",
        "1 + -2",
        "- - 1",
        "1 + 2 in 3",
        "a in b + c",
        "a in b in c",
        "position(a + b in c in d)",
        "position(x in y) + 1",
        "position((a in b) in c)",
        "-position(a in b)!",
        "a between b + c and d * e",
        "a between b and c in d",
        "-a between b and c!",
        "a between b between c and d and e",
        "position(a between b and c in d in e)",
        "a from b + c for d! in e",
        "a from b for c from d for e",
        "a[b!]",
        "-a[b][c]!",
        "position(a[b] from c for d in e)",
        "1 2",
    ] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let parsed = grammar::Expr::parse(&mut input).expect("positive input parses");
        println!(
            "ok {source:?} => {:?} cursor={} eof={}",
            parsed.into_ast(),
            input.cursor(),
            input.is_eof()
        );
    }

    for source in [
        "",
        "1 +",
        "(1 + 2",
        "!1",
        "- * 2",
        "position(a in)",
        "position(a)",
        "in 1",
        "a between b",
        "a between b in c and d",
        "position(a between b in c and d in e)",
        "a from b in c for d",
        "a[b + c]",
        "a[b in c]",
        "a[",
    ] {
        let lexed = grammar::lex(source);
        let mut input = lexed.input();
        let error = grammar::Expr::parse(&mut input).expect_err("failing input fails");
        println!(
            "err {source:?} => code={} kind={:?} span={:?} found={:?} cursor={}",
            error.code(),
            error.kind(),
            error.span(),
            error.found(),
            input.cursor()
        );
        assert_eq!(input.cursor(), 0, "a failed parse restores the cursor");
    }
    println!("table-driven-pratt-ok");
}
