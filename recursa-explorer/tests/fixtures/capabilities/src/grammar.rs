recursa::grammar! {
    module = crate::grammar,
    diagrams,
    derives(Debug, Clone, Pretty, Visit, Arbitrary),
    keyword_matching = sensitive,
}

recursa::tokens! {
    keywords {
        READY => "ready",
        WAIT => "wait",
    }
}

recursa::ast_node! {
    /// Minimal root exposing both optional Explorer capabilities.

    pub enum Root {
        /// First deterministic fixed-token choice.
        #[tok(READY)]
        Ready,
        /// Alternate fixed-token choice proving sampling consumes entropy.
        #[tok(WAIT)]
        Wait,
    }
}
