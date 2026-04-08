/// Configuration for a grammar's ignored content (whitespace, comments, etc.).
///
/// `IGNORE` is a regex pattern matched and skipped between tokens during parsing.
/// It must be a const so derive macros can splice it into lookahead regexes at compile time.
pub trait ParseRules {
    const IGNORE: &'static str;
}

/// No-op rules for `Scan` types that don't skip whitespace.
pub struct NoRules;

impl ParseRules for NoRules {
    const IGNORE: &'static str = "";
}
