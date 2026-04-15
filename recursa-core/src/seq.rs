use std::fmt;
use std::marker::PhantomData;
use std::ops::{ControlFlow, Deref};

use crate::fmt::FormatTokens;

use crate::error::ParseError;
use crate::input::Input;
use crate::parse::Parse;
use crate::rules::ParseRules;
use crate::visitor::{AsNodeKey, Break, TotalVisitor, Visit};

// -- Marker types --

/// No trailing separator allowed. Last element has no separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoTrailing;

/// Trailing separator is required. Every element must be followed by a separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequiredTrailing;

/// Trailing separator is optional. Last element may or may not have a separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OptionalTrailing;

/// Sequence may be empty (zero elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AllowEmpty;

/// Sequence must have at least one element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

impl<T: Clone, S: Clone, Trailing, Empty> Clone for Seq<T, S, Trailing, Empty> {
    fn clone(&self) -> Self {
        Self {
            pairs: self.pairs.clone(),
            elements: self.elements.clone(),
            _phantom: PhantomData,
        }
    }
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

// Equality, ordering, and hashing all delegate to the canonical `pairs`
// representation (`elements` is derived from `pairs` and stays in sync).
// Bounds are intentionally only on the data-bearing type parameters `T`/`S`;
// the marker parameters `Trailing`/`Empty` live only in `PhantomData`.

impl<T: PartialEq, S: PartialEq, Trailing, Empty> PartialEq for Seq<T, S, Trailing, Empty> {
    fn eq(&self, other: &Self) -> bool {
        self.pairs == other.pairs
    }
}

impl<T: Eq, S: Eq, Trailing, Empty> Eq for Seq<T, S, Trailing, Empty> {}

impl<T: PartialOrd, S: PartialOrd, Trailing, Empty> PartialOrd for Seq<T, S, Trailing, Empty> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.pairs.partial_cmp(&other.pairs)
    }
}

impl<T: Ord, S: Ord, Trailing, Empty> Ord for Seq<T, S, Trailing, Empty> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pairs.cmp(&other.pairs)
    }
}

impl<T: std::hash::Hash, S: std::hash::Hash, Trailing, Empty> std::hash::Hash
    for Seq<T, S, Trailing, Empty>
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pairs.hash(state);
    }
}

// -- Parse loop helpers --

fn parse_no_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = T::parse::<R>(input)?;

        R::consume_ignored(input);
        if !S::peek::<R>(input) {
            pairs.push((element, None));
            break;
        }

        let sep = S::parse::<R>(input)?;
        pairs.push((element, Some(sep)));
        R::consume_ignored(input);
    }
    Ok(pairs)
}

fn parse_optional_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = T::parse::<R>(input)?;

        R::consume_ignored(input);
        if !S::peek::<R>(input) {
            pairs.push((element, None));
            break;
        }

        let sep = S::parse::<R>(input)?;

        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            pairs.push((element, Some(sep)));
            break;
        }

        pairs.push((element, Some(sep)));
    }
    Ok(pairs)
}

fn parse_required_trailing<'input, T, S, R>(
    input: &mut Input<'input>,
) -> Result<Vec<(T, Option<S>)>, ParseError>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
    R: ParseRules,
{
    let mut pairs = Vec::new();
    loop {
        let element = T::parse::<R>(input)?;

        R::consume_ignored(input);
        let sep = S::parse::<R>(input)?;

        pairs.push((element, Some(sep)));

        R::consume_ignored(input);
        if !T::peek::<R>(input) {
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
    fn peek<R: ParseRules>(_input: &Input<'input>) -> bool {
        true
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_no_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, OptionalTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    fn peek<R: ParseRules>(_input: &Input<'input>) -> bool {
        true
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_optional_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, RequiredTrailing, AllowEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    fn peek<R: ParseRules>(_input: &Input<'input>) -> bool {
        true
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            return Ok(Self::from_pairs(Vec::new()));
        }
        let pairs = parse_required_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

// -- Parse implementations: NonEmpty --

impl<'input, T, S> Parse<'input> for Seq<T, S, NoTrailing, NonEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                "sequence element",
            ));
        }
        let pairs = parse_no_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, OptionalTrailing, NonEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                "sequence element",
            ));
        }
        let pairs = parse_optional_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

impl<'input, T, S> Parse<'input> for Seq<T, S, RequiredTrailing, NonEmpty>
where
    T: Parse<'input> + Clone,
    S: Parse<'input> + Clone,
{
    fn peek<R: ParseRules>(input: &Input<'input>) -> bool {
        T::peek::<R>(input)
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>) -> Result<Self, ParseError> {
        R::consume_ignored(input);
        if !T::peek::<R>(input) {
            return Err(ParseError::new(
                input.source(),
                input.cursor()..input.cursor(),
                "sequence element",
            ));
        }
        let pairs = parse_required_trailing::<T, S, R>(input)?;
        Ok(Self::from_pairs(pairs))
    }
}

// -- Visit --

impl<T: Visit + Clone, S: Visit + Clone, Trailing: 'static, Empty: 'static> AsNodeKey
    for Seq<T, S, Trailing, Empty>
{
}

impl<T: Visit + Clone, S: Visit + Clone, Trailing: 'static, Empty: 'static> Visit
    for Seq<T, S, Trailing, Empty>
{
    fn visit<V: TotalVisitor>(&self, visitor: &mut V) -> ControlFlow<Break<V::Error>> {
        for (element, sep) in self.pairs() {
            element.visit(visitor)?;
            if let Some(sep) = sep {
                sep.visit(visitor)?;
            }
        }
        ControlFlow::Continue(())
    }
}

// -- FormatTokens --

impl<T: FormatTokens + Clone, S: FormatTokens + Clone, Trailing: 'static, Empty: 'static>
    FormatTokens for Seq<T, S, Trailing, Empty>
{
    fn format_tokens(&self, tokens: &mut Vec<crate::fmt::Token>) {
        for (element, sep) in self.pairs() {
            element.format_tokens(tokens);
            if let Some(sep) = sep {
                sep.format_tokens(tokens);
                // Break opportunity after each separator.
                // In a Consistent group, all breaks break together.
                // In flat mode, uses a space.
                tokens.push(crate::fmt::Token::Break {
                    flat: " ".into(),
                    broken: "\n".into(),
                });
            }
        }
    }
}
