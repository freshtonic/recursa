use crate::error::ParseError;
use crate::input::Input;
use crate::rules::ParseRules;

/// Recursive descent parser trait.
///
/// Structs derive `Parse` as a sequence (parse fields in order).
/// Enums derive `Parse` as a choice (peek to select variant).
pub trait Parse<'input>: Sized {
    /// Check whether this production can start at the current input position.
    ///
    /// The only guarantee given when `peek` returns true is that sufficient lookahead has been peformed to determine
    /// that `Self` is valid parse choice. It does *NOT* imply that `Self::parse(..)` will be successful because there
    /// could be syntax errors beyond the lookahead.
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool;

    /// Parse this production, advancing the input on success.
    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError>;
}

/// Blanket implementation: `Box<T>` delegates to `T`.
/// Needed for recursive types like `Box<Expr>` in Pratt parsing.
impl<'input, T: Parse<'input>> Parse<'input> for Box<T> {
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        Ok(Box::new(T::parse::<R>(input)?))
    }
}

/// Blanket implementation: `Option<T>` is peek-based.
/// Returns `Some(T)` if `T::peek` succeeds, `None` otherwise.
/// If peek succeeds but parse fails, the error propagates.
impl<'input, T: Parse<'input>> Parse<'input> for Option<T> {
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        if T::peek::<R>(input) {
            let mut fork = input.fork();
            match T::parse::<R>(&mut fork) {
                Ok(val) => {
                    input.commit(fork);
                    Ok(Some(val))
                }
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}

/// Unit type as a no-op separator for `Seq<T, ()>`.
/// Always peeks true and parses without consuming input.
/// Use with `OptionalTrailing` so the loop checks `T::peek` between elements.
impl<'input> Parse<'input> for () {
    fn peek<R: ParseRules>(_input: &Input<'input>) -> bool {
        true
    }

    fn parse<R: ParseRules>(_input: &mut Input<'input>) -> Result<Self, ParseError> {
        Ok(())
    }
}

/// Blanket implementation: `PhantomData<T>` parses `T` but discards the value.
/// Useful for keyword tokens in structs where the token is needed for parsing
/// but carries no information worth storing.
impl<'input, T: Parse<'input>> Parse<'input> for std::marker::PhantomData<T> {
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        T::parse::<R>(input)?;
        Ok(std::marker::PhantomData)
    }
}

/// Blanket implementations: tuples `(T1, T2, ..., Tn)` parse as a sequence,
/// equivalent to a tuple struct with the same fields. Useful for grouping
/// short fixed-length keyword sequences without having to declare a named
/// wrapper type. `peek` delegates to the first element; `parse` parses each
/// element in declaration order with `consume_ignored` between them.
macro_rules! impl_parse_for_tuple {
    ($head:ident, $($tail:ident),+) => {
        impl<'input, $head, $($tail),+> Parse<'input> for ($head, $($tail),+)
        where
            $head: Parse<'input>,
            $($tail: Parse<'input>),+
        {
            fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
                $head::peek::<R>(input)
            }

            #[allow(non_snake_case)]
            fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
                let mut fork = input.fork();
                R::consume_ignored(&mut fork);
                let $head = $head::parse::<R>(&mut fork)?;
                $(
                    R::consume_ignored(&mut fork);
                    let $tail = $tail::parse::<R>(&mut fork)?;
                )+
                input.commit(fork);
                Ok(($head, $($tail),+))
            }
        }
    };
}

impl_parse_for_tuple!(A, B);
impl_parse_for_tuple!(A, B, C);
impl_parse_for_tuple!(A, B, C, D);
impl_parse_for_tuple!(A, B, C, D, E);
impl_parse_for_tuple!(A, B, C, D, E, F);
impl_parse_for_tuple!(A, B, C, D, E, F, G);
impl_parse_for_tuple!(A, B, C, D, E, F, G, H);

/// Blanket implementation: `Vec<T>` parses zero-or-more `T` with no separator.
/// Repeatedly parses `T` while `T::peek` succeeds.
impl<'input, T: Parse<'input>> Parse<'input> for Vec<T> {
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        let mut items = Vec::new();
        loop {
            let mut fork = input.fork();
            R::consume_ignored(&mut fork);
            if !T::peek::<R>(&fork) {
                break;
            }
            input.commit(fork);
            items.push(T::parse::<R>(input)?);
        }
        Ok(items)
    }
}
