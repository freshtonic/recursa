use std::sync::OnceLock;

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

    /// The terminal prefix patterns for this production.
    ///
    /// For Scan types, returns the single token pattern.
    /// For structs, returns consecutive terminal field patterns from the start.
    /// For enums, returns variant prefix patterns used to build combined peek regexes.
    fn first_patterns() -> &'static [&'static str];

    /// Check whether this production can start at the current input position.
    fn peek(input: &Input<'input, Self::Rules>) -> bool;

    /// Parse this production, advancing the input on success.
    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError>;
}

/// Blanket implementation: every `Scan` type is also a `Parse` type with `NoRules`.
impl<'input, T: Scan<'input>> Parse<'input> for T {
    type Rules = NoRules;
    const IS_TERMINAL: bool = true;

    fn first_patterns() -> &'static [&'static str] {
        // We need a per-Scan-type static slice. Function-local statics in
        // generic functions are shared across monomorphisations, so we use
        // a global map keyed by the pattern string pointer (each Scan type
        // has a unique &'static str PATTERN constant with a distinct address).
        use std::collections::HashMap;
        use std::sync::Mutex;

        static MAP: OnceLock<Mutex<HashMap<usize, &'static [&'static str]>>> = OnceLock::new();
        let map = MAP.get_or_init(|| Mutex::new(HashMap::new()));
        let key = T::PATTERN.as_ptr() as usize;
        let mut guard = map.lock().unwrap();
        guard
            .entry(key)
            .or_insert_with(|| Box::leak(Box::new([T::PATTERN])))
    }

    fn peek(input: &Input<'input, NoRules>) -> bool {
        <T as Scan>::peek(input)
    }

    fn parse(input: &mut Input<'input, NoRules>) -> Result<Self, ParseError> {
        <T as Scan>::parse(input)
    }
}
