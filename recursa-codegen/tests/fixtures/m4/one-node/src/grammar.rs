recursa::grammar! {
    module = crate::r#type,
    keyword_matching = sensitive,
    max_lookahead = 1,
}

#[derive(recursa::Node)]
#[allow(non_camel_case_types)]
pub struct r#match<'input> {
    #[lex(pattern = r"[A-Za-z]+")]
    pub text: WordText<'input>,
}
