recursa::grammar! {
    module = crate::r#type,
    keyword_matching = sensitive,
    max_lookahead = 1,
    derives(Visit, VisitMut),
}

recursa::tokens! {
    punctuation { BANG => "!" }
}

#[derive(recursa::Node, Debug, Clone)]
#[allow(non_camel_case_types)]
pub struct r#match<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub text: WordText<'input>,
    pub marker: Option<Marker>,
}

#[derive(recursa::Node, Debug, Clone)]
#[tok(BANG)]
pub struct Marker;
