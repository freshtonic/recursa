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

recursa::ast_node! {

    #[tok(SELECT, this)]
    pub struct Selection {
        #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(SELECT, NULL))]
        pub name: WordText,
    }
}

recursa::ast_node! {

    pub struct AdmissionDefinitions {
        #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(WindowRefName))]
        pub window: WindowName,
        #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(PsqlVariableName))]
        pub variable: VariableName,
        #[lex(pattern = r"[A-JL-Za-jl-z]+", admits(BareLabels))]
        pub bare: BareName,
    }
}

recursa::ast_node! {

    pub enum WindowOrRows {
        Window(WindowName),
        #[tok(ROWS)]
        Rows,
    }
}

recursa::ast_node! {

    pub enum VariableOrNull {
        Variable(VariableName),
        #[tok(NULL)]
        Null,
    }
}

recursa::ast_node! {

    pub enum BareOrOther {
        Bare(BareName),
        #[tok(OTHER)]
        Other,
    }
}

recursa::ast_node! {

    pub enum FiveTokenChoice {
        #[tok(SELECT, NULL, TRUE, FALSE, WINDOW)]
        Window,
        #[tok(SELECT, NULL, TRUE, FALSE, OTHER)]
        Other,
    }
}

recursa::ast_node! {

    pub struct QuotedSelection {
        #[lex(pattern = r#"'[^']*'|"[^"]*""#)]
        pub name: QuotedText,
    }
}

recursa::ast_node! {

    pub struct ClosedMatcherDefinitions {
        #[lex(matcher)]
        pub dollar: DollarRegion,
        #[lex(matcher)]
        pub nested: NestedRegion,
        #[lex(matcher)]
        pub number: BoundedNumber,
        #[lex(matcher)]
        pub operator: OperatorText,
        #[lex(matcher)]
        pub question_operator: QuestionOperatorText,
        #[lex(matcher)]
        pub line: PsqlLine,
        #[lex(matcher)]
        pub priority_low: PriorityLow,
        #[lex(matcher)]
        pub priority_high: PriorityHigh,
        #[lex(matcher)]
        pub priority_high_first: PriorityHighFirst,
        #[lex(matcher)]
        pub priority_low_second: PriorityLowSecond,
    }
}
