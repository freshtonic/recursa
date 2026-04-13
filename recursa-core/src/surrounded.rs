use std::marker::PhantomData;
use std::ops::Deref;

use crate::error::ParseError;
use crate::input::Input;
use crate::parse::Parse;
use crate::rules::ParseRules;
use crate::visitor::{AsNodeKey, Break, TotalVisitor, Visit};

/// A value surrounded by open and close delimiters.
///
/// The delimiters are parsed and consumed but not stored — only the inner
/// value is accessible. Derefs to the inner value.
pub struct Surrounded<Open, Inner, Close> {
    _open: PhantomData<Open>,
    pub inner: Inner,
    _close: PhantomData<Close>,
}

impl<Open, Inner: std::fmt::Debug, Close> std::fmt::Debug for Surrounded<Open, Inner, Close> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Surrounded")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<Open, Inner: Clone, Close> Clone for Surrounded<Open, Inner, Close> {
    fn clone(&self) -> Self {
        Self {
            _open: PhantomData,
            inner: self.inner.clone(),
            _close: PhantomData,
        }
    }
}

impl<Open, Inner, Close> Deref for Surrounded<Open, Inner, Close> {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'input, Open, Inner, Close> Parse<'input> for Surrounded<Open, Inner, Close>
where
    Open: Parse<'input>,
    Inner: Parse<'input>,
    Close: Parse<'input>,
{
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        Open::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        Open::parse::<R>(input)?;
        R::consume_ignored(input);
        let inner = Inner::parse::<R>(input)?;
        R::consume_ignored(input);
        Close::parse::<R>(input)?;
        Ok(Surrounded {
            _open: PhantomData,
            inner,
            _close: PhantomData,
        })
    }
}

impl<Open: 'static, Inner: Visit, Close: 'static> AsNodeKey for Surrounded<Open, Inner, Close> {}
impl<Open: 'static, Inner: Visit, Close: 'static> Visit for Surrounded<Open, Inner, Close> {
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> std::ops::ControlFlow<Break<V::Error>> {
        self.inner.visit(visitor)
    }
}

// -- FormatTokens --

impl<Open: crate::fmt::TokenText, Inner: crate::fmt::FormatTokens, Close: crate::fmt::TokenText>
    crate::fmt::FormatTokens for Surrounded<Open, Inner, Close>
{
    fn format_tokens(&self, tokens: &mut Vec<crate::fmt::Token>) {
        tokens.push(crate::fmt::Token::String(Open::TEXT.to_string()));
        self.inner.format_tokens(tokens);
        tokens.push(crate::fmt::Token::String(Close::TEXT.to_string()));
    }
}
