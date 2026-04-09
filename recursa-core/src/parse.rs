use crate::error::ParseError;
use crate::input::Input;
use crate::rules::ParseRules;

/// Recursive descent parser trait.
///
/// Structs derive `Parse` as a sequence (parse fields in order).
/// Enums derive `Parse` as a choice (peek to select variant).
/// `Scan` types get a `Parse` impl via `#[derive(Scan)]` or manually
/// using the [`impl_parse_for_scan!`] macro.
pub trait Parse<'input>: Sized {
    type Rules: ParseRules;

    /// Whether this type is a leaf token (Scan type) or a composite production.
    const IS_TERMINAL: bool;

    /// A regex fragment representing the terminal prefix of this production.
    ///
    /// For Scan types: the token's pattern (e.g., `"let"`).
    /// For structs: consecutive terminal field patterns joined with IGNORE
    ///   (e.g., `"pub(?:\\s+)?fn"`).
    /// For enums: an alternation of variant patterns wrapped in groups
    ///   (e.g., `"(pub(?:\\s+)?fn)|(pub(?:\\s+)?struct)"`).
    ///
    /// The returned string is a regex fragment, not a complete regex —
    /// it has no `\A` anchor. Callers are responsible for anchoring.
    fn first_pattern() -> &'static str;

    /// Check whether this production can start at the current input position.
    fn peek(input: &Input<'input, Self::Rules>) -> bool;

    /// Parse this production, advancing the input on success.
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}

/// Blanket implementation: `Box<T>` delegates to `T`.
/// Needed for recursive types like `Box<Expr>` in Pratt parsing.
impl<'input, T: Parse<'input>> Parse<'input> for Box<T> {
    type Rules = T::Rules;
    const IS_TERMINAL: bool = T::IS_TERMINAL;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        T::peek(input)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        Ok(Box::new(T::parse(input)?))
    }
}

/// Blanket implementation: `Option<T>` is peek-based.
/// Returns `Some(T)` if `T::peek` succeeds, `None` otherwise.
/// If peek succeeds but parse fails, the error propagates.
impl<'input, T: Parse<'input>> Parse<'input> for Option<T> {
    type Rules = T::Rules;
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        T::peek(input)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        if T::peek(input) {
            Ok(Some(T::parse(input)?))
        } else {
            Ok(None)
        }
    }
}

/// Implements `Parse` for a type that already implements `Scan`.
///
/// This bridges `Scan` types into the `Parse` trait with `NoRules` and
/// `IS_TERMINAL = true`. Use this for manual `Scan` implementations;
/// `#[derive(Scan)]` generates this automatically.
///
/// # Example
///
/// ```ignore
/// impl Scan<'_> for MyKeyword { /* ... */ }
/// impl_parse_for_scan!(MyKeyword);
/// ```
#[macro_export]
macro_rules! impl_parse_for_scan {
    ($ty:ty) => {
        impl<'input> $crate::Parse<'input> for $ty
        where
            $ty: $crate::Scan<'input>,
        {
            type Rules = $crate::NoRules;
            const IS_TERMINAL: bool = true;

            fn first_pattern() -> &'static str {
                <$ty as $crate::Scan>::PATTERN
            }

            fn peek(input: &$crate::Input<'input, $crate::NoRules>) -> bool {
                <$ty as $crate::Scan>::peek(input)
            }

            fn parse(
                input: &mut $crate::Input<'input, $crate::NoRules>,
            ) -> ::std::result::Result<Self, $crate::ParseError> {
                <$ty as $crate::Scan>::parse(input)
            }
        }
    };
}
