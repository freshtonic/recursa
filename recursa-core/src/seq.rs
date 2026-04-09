use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use crate::error::ParseError;
use crate::input::Input;
use crate::parse::Parse;
use crate::rules::{NoRules, ParseRules};
use crate::scan::Scan;

// -- Marker types --

/// No trailing separator allowed. Last element has no separator.
pub struct NoTrailing;

/// Trailing separator is required. Every element must be followed by a separator.
pub struct RequiredTrailing;

/// Trailing separator is optional. Last element may or may not have a separator.
pub struct OptionalTrailing;

/// Sequence may be empty (zero elements).
pub struct AllowEmpty;

/// Sequence must have at least one element.
pub struct NonEmpty;

// -- Seq type --

/// A separated list of elements with type-level configuration.
///
/// - `T`: element type
/// - `S`: separator type
/// - `R`: parse rules for whitespace handling between elements and separators
/// - `Trailing`: trailing separator policy (`NoTrailing`, `RequiredTrailing`, `OptionalTrailing`)
/// - `Empty`: emptiness policy (`AllowEmpty`, `NonEmpty`)
///
/// The `R` parameter determines which `ParseRules` govern whitespace consumption
/// between elements and separators during parsing. When `Seq` is used inside a
/// struct with `#[parse(rules = WsRules)]`, pass `WsRules` as `R` so that
/// whitespace is correctly skipped. Defaults to `NoRules` (no whitespace skipping).
pub struct Seq<T, S, R: ParseRules = NoRules, Trailing = NoTrailing, Empty = AllowEmpty> {
    pairs: Vec<(T, Option<S>)>,
    elements: Vec<T>,
    _phantom: PhantomData<(R, Trailing, Empty)>,
}

impl<T: Clone, S, R: ParseRules, Trailing, Empty> Seq<T, S, R, Trailing, Empty> {
    /// Create a Seq from raw element-separator pairs.
    pub fn from_pairs(pairs: Vec<(T, Option<S>)>) -> Self {
        let elements = pairs.iter().map(|(t, _)| t.clone()).collect();
        Self {
            pairs,
            elements,
            _phantom: PhantomData,
        }
    }

    /// Access the raw element-separator pairs.
    pub fn pairs(&self) -> &[(T, Option<S>)] {
        &self.pairs
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether the sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

impl<T: Clone, S, R: ParseRules> Seq<T, S, R, NoTrailing, AllowEmpty> {
    /// Create an empty Seq (only available for AllowEmpty variants).
    pub fn empty() -> Self {
        Self {
            pairs: Vec::new(),
            elements: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

/// Deref to `Vec<T>` for `AllowEmpty` variants.
impl<T: Clone, S, R: ParseRules, Trailing> Deref for Seq<T, S, R, Trailing, AllowEmpty> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl<T: fmt::Debug, S: fmt::Debug, R: ParseRules, Trailing, Empty> fmt::Debug
    for Seq<T, S, R, Trailing, Empty>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Seq").field("pairs", &self.pairs).finish()
    }
}

// -- Parse implementations --

impl<'input, T, S, R> Parse<'input> for Seq<T, S, R, NoTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
    R: ParseRules,
{
    type Rules = R;
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek(_input: &Input<'input, Self::Rules>) -> bool {
        // AllowEmpty: always valid (might parse zero elements)
        true
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        let mut pairs = Vec::new();

        // Peek for first element
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(pairs));
        }

        loop {
            // Parse element
            let mut rebound = input.rebind::<<T as Parse>::Rules>();
            let element = <T as Parse>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            // Peek for separator
            input.consume_ignored();
            let rebound = input.rebind::<NoRules>();
            if !<S as Scan>::peek(&rebound) {
                // No separator -- this is the last element
                pairs.push((element, None));
                break;
            }

            // Parse separator
            let mut rebound = input.rebind::<NoRules>();
            let sep = <S as Scan>::parse(&mut rebound)?;
            input.commit(rebound.rebind());

            pairs.push((element, Some(sep)));

            input.consume_ignored();
        }

        Ok(Self::from_pairs(pairs))
    }
}
