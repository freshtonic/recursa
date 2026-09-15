recursa::grammar! {
    module = crate::r#type,
    keyword_matching = sensitive,
    derives(Visit, VisitMut),
}

recursa::tokens! {
    punctuation { BANG => "!" }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[allow(non_camel_case_types)]
    pub struct r#match {
        #[lex(pattern = r"[A-Za-z]+")]
        pub text: WordText,
        pub marker: Option<Marker>,
    }
}

recursa::ast_node! {
    #[derive(Debug, Clone)]
    #[tok(BANG)]
    pub struct Marker;
}
