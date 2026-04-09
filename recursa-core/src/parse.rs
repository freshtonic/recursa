use crate::error::ParseError;
use crate::input::Input;
use crate::rules::{NoRules, ParseRules};
use crate::scan::Scan;

/// Recursive descent parser trait.
///
/// Structs derive `Parse` as a sequence (parse fields in order).
/// Enums derive `Parse` as a choice (peek to select variant).
/// `Scan` types get a blanket implementation automatically.
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

/// Blanket implementation: every `Scan` type is also a `Parse` type with `NoRules`.
impl<'input, T: Scan<'input>> Parse<'input> for T {
    type Rules = NoRules;
    const IS_TERMINAL: bool = true;

    fn first_pattern() -> &'static str {
        T::PATTERN
    }

    fn peek(input: &Input<'input, NoRules>) -> bool {
        <T as Scan>::peek(input)
    }

    fn parse(input: &mut Input<'input, NoRules>) -> Result<Self, ParseError> {
        <T as Scan>::parse(input)
    }
}
