use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use crate::error::ParseError;
use crate::input::Input;
use crate::parse::Parse;
use crate::rules::ParseRules;

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
/// - `T`: element type (implements `Parse`)
/// - `S`: separator type (implements `Parse`)
/// - `Trailing`: trailing separator policy (`NoTrailing`, `RequiredTrailing`, `OptionalTrailing`)
/// - `Empty`: emptiness policy (`AllowEmpty`, `NonEmpty`)
pub struct Seq<T, S, Trailing = NoTrailing, Empty = AllowEmpty> {
    pairs: Vec<(T, Option<S>)>,
    elements: Vec<T>,
    _phantom: PhantomData<(Trailing, Empty)>,
}

impl<T: Clone, S, Trailing, Empty> Seq<T, S, Trailing, Empty> {
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

impl<T: Clone, S> Seq<T, S, NoTrailing, AllowEmpty> {
    /// Create an empty Seq (only available for AllowEmpty + NoTrailing).
    pub fn empty() -> Self {
        Self {
            pairs: Vec::new(),
            elements: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

impl<T: Clone, S, Trailing> Deref for Seq<T, S, Trailing, AllowEmpty> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl<T: Clone, S, Trailing> Deref for Seq<T, S, Trailing, NonEmpty> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl<T: fmt::Debug, S: fmt::Debug, Trailing, Empty> fmt::Debug for Seq<T, S, Trailing, Empty> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Seq").field("pairs", &self.pairs).finish()
    }
}

// -- Parse loop helpers --

fn parse_no_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
    rules: &R,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = <T as Parse>::parse(input, rules)?;

        R::consume_ignored(input);
        if !<S as Parse>::peek(input, rules) {
            pairs.push((element, None));
            break;
        }

        let sep = <S as Parse>::parse(input, rules)?;
        pairs.push((element, Some(sep)));
        R::consume_ignored(input);
    }
    Ok(pairs)
}

fn parse_optional_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
    rules: &R,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = <T as Parse>::parse(input, rules)?;

        R::consume_ignored(input);
        if !<S as Parse>::peek(input, rules) {
            pairs.push((element, None));
            break;
        }

        let sep = <S as Parse>::parse(input, rules)?;

        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            pairs.push((element, Some(sep)));
            break;
        }

        pairs.push((element, Some(sep)));
    }
    Ok(pairs)
}

fn parse_required_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
    rules: &R,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = <T as Parse>::parse(input, rules)?;

        R::consume_ignored(input);
        let sep = <S as Parse>::parse(input, rules)?;

        pairs.push((element, Some(sep)));

        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            break;
        }
    }
    Ok(pairs)
}

// -- Parse implementations: AllowEmpty --

impl<'input, T, S> Parse<'input> for Seq<T, S, NoTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(_input: &Input<'input>, _rules: &R) -> bool {
        true
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_no_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, OptionalTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(_input: &Input<'input>, _rules: &R) -> bool {
        true
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_optional_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, RequiredTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(_input: &Input<'input>, _rules: &R) -> bool {
        true
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_required_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}

// -- Parse implementations: NonEmpty --

impl<'input, T, S> Parse<'input> for Seq<T, S, NoTrailing, NonEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        <T as Parse>::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                T::first_pattern(),
            ));
        }
        let pairs = parse_no_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, OptionalTrailing, NonEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        <T as Parse>::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                T::first_pattern(),
            ));
        }
        let pairs = parse_optional_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, RequiredTrailing, NonEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        T::first_pattern()
    }

    fn peek<R: ParseRules>(input: &Input<'input>, rules: &R) -> bool {
        <T as Parse>::peek(input, rules)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !<T as Parse>::peek(input, rules) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                T::first_pattern(),
            ));
        }
        let pairs = parse_required_trailing::<T, S, R>(input, rules)?;
        Ok(Self::from_pairs(pairs))
    }
}
