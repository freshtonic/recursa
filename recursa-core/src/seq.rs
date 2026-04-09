use std::marker::PhantomData;
use std::ops::Deref;

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
impl<T: Clone, S, Trailing> Deref for Seq<T, S, Trailing, AllowEmpty> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}
