recursa::grammar! {
    module = crate::grammar,
    keyword_matching = ascii_insensitive,
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
        LineComment => r"--[^\n]+",
        BlockComment => nested(opener = "/*", closer = "*/"),
    }
    categories { UNRESERVED, RESERVED }
    flags { bare_label }
    keywords {
        SELECT => "select" in RESERVED,
        NULL => "null" in RESERVED,
        TRUE => "true" in RESERVED,
        FALSE => "false" in RESERVED,
        CUSTOM_KEYWORD => "kk" in RESERVED,
        WINDOW => "window" in UNRESERVED + bare_label,
        OTHER => "other" in UNRESERVED,
        ROWS => "rows" in RESERVED,
        RANGE => "range" in RESERVED,
        GROUPS => "groups" in RESERVED,
    }
    admissions {
        UnreservedNames = UNRESERVED,
        AllCategories = UnreservedNames + RESERVED,
        ColId = AllCategories,
        WindowRefName = ColId - { ROWS, RANGE, GROUPS },
        AllWordKinds = keywords,
        PsqlVariableName = AllWordKinds - { NULL, TRUE, FALSE },
        BareLabels = bare_label,
    }
    punctuation {
        COLON => ":",
        NOT_EQUAL => "!=",
        BANG_EQ_MINUS => "!=-",
        BANG_SLASH => "!/",
        DASH_DASH => "--",
        QUESTION_QUESTION => "??",
        DOLLAR_TAG => "$tag$",
    }
    matchers {
        DollarRegion => same_delimiter(opener = r"\$(?:[A-Za-z_][A-Za-z0-9_]*)?\$"),
        NestedRegion => nested(opener = "{-", closer = "-}"),
        BoundedNumber => next_exclusion(pattern = r"[0-9]+", excluded = r"[A-Za-z0-9_]"),
        OperatorText => operator_run(
            characters = "-+*/<>=!#%^&|?",
            fences = ["/*", "--"],
            trailing = "+-",
            qualifying = "!#%^&|?",
            priority = 3
        ),
        QuestionOperatorText => operator_run(
            characters = "?~",
            fences = ["??"],
            trailing = "",
            qualifying = "",
            priority = 2
        ),
        PsqlLine => physical_line(prefix = r"\\[A-Za-z]+"),
        PriorityLow => physical_line(prefix = r"§", priority = 1),
        PriorityHigh => physical_line(prefix = r"§", priority = 2),
        PriorityHighFirst => physical_line(prefix = r"¤", priority = 2),
        PriorityLowSecond => physical_line(prefix = r"¤", priority = 1),
    }
}

#[derive(recursa::Node)]
#[tok(SELECT, this)]
pub struct Selection<'input> {
    #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(SELECT, NULL))]
    pub name: WordText<'input>,
}

#[derive(recursa::Node)]
pub struct AdmissionDefinitions<'input> {
    #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(WindowRefName))]
    pub window: WindowName<'input>,
    #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(PsqlVariableName))]
    pub variable: VariableName<'input>,
    #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(BareLabels))]
    pub bare: BareName<'input>,
}

#[derive(recursa::Node)]
pub enum WindowOrRows<'input> {
    Window(WindowName<'input>),
    #[tok(ROWS)]
    Rows,
}

#[derive(recursa::Node)]
pub enum VariableOrNull<'input> {
    Variable(VariableName<'input>),
    #[tok(NULL)]
    Null,
}

#[derive(recursa::Node)]
pub enum BareOrOther<'input> {
    Bare(BareName<'input>),
    #[tok(OTHER)]
    Other,
}

#[derive(recursa::Node)]
pub enum FiveTokenChoice {
    #[tok(SELECT, NULL, TRUE, FALSE, WINDOW)]
    Window,
    #[tok(SELECT, NULL, TRUE, FALSE, OTHER)]
    Other,
}

#[derive(recursa::Node)]
pub struct QuotedSelection<'input> {
    #[lex(pattern = r#"'[^']*'|"[^"]*""#)]
    pub name: QuotedText<'input>,
}

#[derive(recursa::Node)]
pub struct ClosedMatcherDefinitions<'input> {
    #[lex(matcher)]
    pub dollar: DollarRegion<'input>,
    #[lex(matcher)]
    pub nested: NestedRegion<'input>,
    #[lex(matcher)]
    pub number: BoundedNumber<'input>,
    #[lex(matcher)]
    pub operator: OperatorText<'input>,
    #[lex(matcher)]
    pub question_operator: QuestionOperatorText<'input>,
    #[lex(matcher)]
    pub line: PsqlLine<'input>,
    #[lex(matcher)]
    pub priority_low: PriorityLow<'input>,
    #[lex(matcher)]
    pub priority_high: PriorityHigh<'input>,
    #[lex(matcher)]
    pub priority_high_first: PriorityHighFirst<'input>,
    #[lex(matcher)]
    pub priority_low_second: PriorityLowSecond<'input>,
}
