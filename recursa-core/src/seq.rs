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

/// Deref to `[T]` for `AllowEmpty` variants.
impl<T: Clone, S, R: ParseRules, Trailing> Deref for Seq<T, S, R, Trailing, AllowEmpty> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

/// Deref to `[T]` for `NonEmpty` variants.
impl<T: Clone, S, R: ParseRules, Trailing> Deref for Seq<T, S, R, Trailing, NonEmpty> {
    type Target = [T];

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

// -- Parse loop helpers --
//
// Each trailing policy has its own loop body logic, but the overall structure
// (peek-for-first, loop, collect pairs) is shared. These helpers parse elements
// after the initial element peek has already succeeded, reducing duplication
// across the six Parse impls (3 trailing policies x 2 emptiness policies).

/// Parse a no-trailing separated list. Caller must ensure the first element
/// is peekable before calling.
fn parse_no_trailing<'input, T, S, R>(
    input: &mut Input<'input, R>,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let mut rebound = input.rebind::<<T as Parse>::Rules>();
        let element = <T as Parse>::parse(&mut rebound)?;
        input.commit(rebound.rebind());

        input.consume_ignored();
        let rebound = input.rebind::<NoRules>();
        if !<S as Scan>::peek(&rebound) {
            pairs.push((element, None));
            break;
        }

        let mut rebound = input.rebind::<NoRules>();
        let sep = <S as Scan>::parse(&mut rebound)?;
        input.commit(rebound.rebind());

        pairs.push((element, Some(sep)));
        input.consume_ignored();
    }
    Ok(pairs)
}

/// Parse an optional-trailing separated list. Caller must ensure the first
/// element is peekable before calling.
fn parse_optional_trailing<'input, T, S, R>(
    input: &mut Input<'input, R>,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let mut rebound = input.rebind::<<T as Parse>::Rules>();
        let element = <T as Parse>::parse(&mut rebound)?;
        input.commit(rebound.rebind());

        input.consume_ignored();
        let rebound = input.rebind::<NoRules>();
        if !<S as Scan>::peek(&rebound) {
            pairs.push((element, None));
            break;
        }

        let mut rebound = input.rebind::<NoRules>();
        let sep = <S as Scan>::parse(&mut rebound)?;
        input.commit(rebound.rebind());

        // Peek for next element -- if absent, this was a trailing separator
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            pairs.push((element, Some(sep)));
            break;
        }

        pairs.push((element, Some(sep)));
    }
    Ok(pairs)
}

/// Parse a required-trailing separated list. Caller must ensure the first
/// element is peekable before calling.
fn parse_required_trailing<'input, T, S, R>(
    input: &mut Input<'input, R>,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Scan<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let mut rebound = input.rebind::<<T as Parse>::Rules>();
        let element = <T as Parse>::parse(&mut rebound)?;
        input.commit(rebound.rebind());

        input.consume_ignored();
        let mut rebound = input.rebind::<NoRules>();
        let sep = <S as Scan>::parse(&mut rebound)?;
        input.commit(rebound.rebind());

        pairs.push((element, Some(sep)));

        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            break;
        }
    }
    Ok(pairs)
}

// -- Parse implementations: AllowEmpty --

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
        true
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_no_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S, R> Parse<'input> for Seq<T, S, R, OptionalTrailing, AllowEmpty>
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
        true
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_optional_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S, R> Parse<'input> for Seq<T, S, R, RequiredTrailing, AllowEmpty>
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
        true
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_required_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

// -- Parse implementations: NonEmpty --

impl<'input, T, S, R> Parse<'input> for Seq<T, S, R, NoTrailing, NonEmpty>
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

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        let rebound = input.rebind::<<T as Parse>::Rules>();
        <T as Parse>::peek(&rebound)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                T::first_pattern(),
            ));
        }
        let pairs = parse_no_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S, R> Parse<'input> for Seq<T, S, R, OptionalTrailing, NonEmpty>
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

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        let rebound = input.rebind::<<T as Parse>::Rules>();
        <T as Parse>::peek(&rebound)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                T::first_pattern(),
            ));
        }
        let pairs = parse_optional_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S, R> Parse<'input> for Seq<T, S, R, RequiredTrailing, NonEmpty>
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

    fn peek(input: &Input<'input, Self::Rules>) -> bool {
        let rebound = input.rebind::<<T as Parse>::Rules>();
        <T as Parse>::peek(&rebound)
    }

    fn parse(input: &mut Input<'input, Self::Rules>) -> Result<Self, ParseError> {
        input.consume_ignored();
        let rebound = input.rebind::<<T as Parse>::Rules>();
        if !<T as Parse>::peek(&rebound) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                T::first_pattern(),
            ));
        }
        let pairs = parse_required_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}
