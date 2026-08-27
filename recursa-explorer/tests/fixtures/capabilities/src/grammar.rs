recursa::grammar! {
    module = crate::grammar,
    diagrams,
    derives(Debug, Clone, Pretty, Visit, Arbitrary),
    keyword_matching = sensitive,
    max_lookahead = 1,
}

recursa::tokens! {
    keywords {
        READY => "ready",
        WAIT => "wait",
    }
}

/// Minimal root exposing both optional Explorer capabilities.
#[derive(recursa::Node)]
pub enum Root {
    /// First deterministic fixed-token choice.
    #[tok(READY)]
    Ready,
    /// Alternate fixed-token choice proving sampling consumes entropy.
    #[tok(WAIT)]
    Wait,
}
