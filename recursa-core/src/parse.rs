use crate::error::ParseError;
use crate::input::Input;
use crate::rules::ParseRules;

/// Recursive descent parser trait.
///
/// Structs derive `Parse` as a sequence (parse fields in order).
/// Enums derive `Parse` as a choice (peek to select variant).
/// `Scan` types get a blanket `Parse` impl automatically.
pub trait Parse<'input>: Sized {
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
    /// The returned string is a regex fragment, not a complete regex --
    /// it has no `\A` anchor. Callers are responsible for anchoring.
    fn first_pattern() -> &'static str;

    /// Check whether this production can start at the current input position.
    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool;

    /// Parse this production, advancing the input on success.
    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError>;
}

/// Implements `Parse` for a type that already implements `Scan`.
///
/// This bridges `Scan` types into the `Parse` trait with
/// `IS_TERMINAL = true`. Use this for manual `Scan` implementations;
/// `#[derive(Scan)]` generates this automatically.
///
/// # Example
///
/// ```text
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
            const IS_TERMINAL: bool = true;

            fn first_pattern() -> &'static str {
                <$ty as $crate::Scan>::PATTERN
            }

            fn peek<R: $crate::ParseRules>(
                input: &$crate::Input<'input>,
                _rules: &R,
            ) -> bool {
                <$ty as $crate::Scan>::peek(input)
            }

            fn parse<R: $crate::ParseRules>(
                input: &mut $crate::Input<'input>,
                _rules: &R,
            ) -> ::std::result::Result<Self, $crate::ParseError> {
                <$ty as $crate::Scan>::parse(input)
            }
        }
    };
}

/// Blanket implementation: `Box<T>` delegates to `T`.
/// Needed for recursive types like `Box<Expr>` in Pratt parsing.
impl<'input, T: Parse<'input>> Parse<'input> for Box<T> {
    const IS_TERMINAL: bool = T::IS_TERMINAL;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        T::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        Ok(Box::new(T::parse(input, rules)?))
    }
}

/// Blanket implementation: `Option<T>` is peek-based.
/// Returns `Some(T)` if `T::peek` succeeds, `None` otherwise.
/// If peek succeeds but parse fails, the error propagates.
impl<'input, T: Parse<'input>> Parse<'input> for Option<T> {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        T::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        if T::peek(input, rules) {
            Ok(Some(T::parse(input, rules)?))
        } else {
            Ok(None)
        }
    }
}
