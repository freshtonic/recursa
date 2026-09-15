//! The merged-kind precedence grammar (recursa#128): PostgreSQL's window-frame conflict, where
//! the token that decides is the merged kind of a lookahead filter.
//!
//! `BETWEEN` is a keyword that also names a column, so `rows between ...` is either the frame
//! extent `BETWEEN low AND high` or the single bound whose offset is a column called `between`.
//! Every operator that can follow the column settles the choice by its own precedence against
//! `BETWEEN`'s. One does not: the `NOT` of `not like`, which the `lookahead` filter turns into
//! `NOT_LA`, a kind the `precedence` block could not name before this ticket. gram.y names it
//! (`%nonassoc BETWEEN IN_P LIKE ILIKE SIMILAR NOT_LA`, `gram.y:837`), which is what the block
//! below does, and what makes `ROWS BETWEEN . NOT_LA` decidable.
//!
//! The historical integration test generated this file as written and with `NOT_LA` removed
//! from the `precedence` block, then compared the results.

recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
    derives(Pretty),
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    keywords {
        NOT => "not",
        BETWEEN => "between",
        AND => "and",
        LIKE => "like",
        ROWS => "rows",
        PRECEDING => "preceding",
    }
    punctuation {
        LPAREN => "(",
        RPAREN => ")",
        SEMI => ";",
    }
    // `parser.c`'s `case NOT:`, cut down to the two triggers this grammar has.
    lookahead { NOT_LA = NOT before { BETWEEN, LIKE } }
    // gram.y:837 `%nonassoc BETWEEN IN_P LIKE ILIKE SIMILAR NOT_LA`, cut down the same way.
    // `NOT_LA` is the merged kind of the filter above, not a lexed token; naming it here is
    // what recursa#128 adds.
    precedence {
        nonassoc(bp = 20) { BETWEEN, LIKE, NOT_LA },
    }
}

recursa::ast_node! {
    /// `rows extent ;`, gram.y's `window_clause` cut down to its frame.
    #[derive(Debug, Clone)]
    #[tok(ROWS, this, SEMI)]
    pub struct Frame {
        pub extent: Extent,
    }
}

recursa::ast_node! {
    /// gram.y's `frame_extent`: `BETWEEN bound AND bound` or one bound.
    #[derive(Debug, Clone)]
    pub enum Extent {
        Between {
            #[tok(BETWEEN)]
            low: Bound,
            #[tok(AND)]
            high: Bound,
        },
        Single(Bound),
    }
}

recursa::ast_node! {
    /// gram.y's `frame_bound`: an offset expression and `PRECEDING`.
    #[derive(Debug, Clone)]
    #[tok(this, PRECEDING)]
    pub struct Bound {
        pub offset: Expr,
    }
}

recursa::ast_node! {
    /// gram.y's `a_expr`, cut down to prefix `NOT`, infix `LIKE` and the `NOT LIKE` test whose
    /// `NOT` the filter merges. The prefix's operand can begin with a trigger read as a name, so
    /// its rule is twinned with `NOT_LA`.
    #[derive(Debug, Clone)]
    #[pratt]
    pub enum Expr {
        Column(Ident),
        /// The enclosing atom Pretty needs to repair a precedence it cannot express.
        Paren(#[tok(LPAREN, this, RPAREN)] boxed!(Self)),
        #[parse(prefix, bp = 10)]
        Not(#[tok(NOT, this)] boxed!(Self)),
        #[parse(postfix, bp = 20)]
        NotLike(boxed!(Self), #[tok(NOT, LIKE, this)] boxed!(Self)),
        #[parse(infix, lbp = 20, rbp = 21)]
        #[tok(LIKE)]
        Like(boxed!(Self), boxed!(Self)),
    }
}

recursa::ast_node! {
    /// gram.y's `ColId`: a name that the keywords `between` and `like` also spell.
    #[derive(Debug, Clone)]
    pub struct Ident {
        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*", admits(BETWEEN, LIKE))]
        pub text: IdentText,
    }
}
