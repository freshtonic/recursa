use std::ops::Deref;

use crate::Span;

#[doc(hidden)]
pub mod span_seal {
    pub trait Sealed {}
}

/// Uniform optional source extent for generated occurrence views.
///
/// Matching generated code supplies implementations; semantic AST values do
/// not carry source provenance or implement this trait.
pub trait HasSpan: span_seal::Sealed {
    fn span(&self) -> Option<Span>;
}

/// A contiguous vector that always contains at least one value.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Vec1<T> {
    values: Vec<T>,
}

impl<T> Vec1<T> {
    pub fn new(first: T) -> Self {
        Self {
            values: vec![first],
        }
    }

    pub fn push(&mut self, value: T) {
        self.values.push(value);
    }

    pub fn append(&mut self, values: &mut Vec<T>) {
        self.values.append(values);
    }

    pub fn first(&self) -> &T {
        &self.values[0]
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn as_slice(&self) -> &[T] {
        &self.values
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.values.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.values.iter_mut()
    }
}

impl<T> Deref for Vec1<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for Vec1<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.values, serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Vec1<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut values = <Vec<T> as serde::Deserialize>::deserialize(deserializer)?;
        if values.is_empty() {
            return Err(serde::de::Error::custom(
                "a Recursa Vec1 requires at least one value",
            ));
        }
        let first = values.remove(0);
        let mut result = Self::new(first);
        result.append(&mut values);
        Ok(result)
    }
}

#[cfg(feature = "arbitrary")]
impl<'arbitrary, T: arbitrary::Arbitrary<'arbitrary>> arbitrary::Arbitrary<'arbitrary> for Vec1<T> {
    fn arbitrary(input: &mut arbitrary::Unstructured<'arbitrary>) -> arbitrary::Result<Self> {
        let first = T::arbitrary(input)?;
        let mut tail = Vec::<T>::arbitrary(input)?;
        let mut result = Self::new(first);
        result.append(&mut tail);
        Ok(result)
    }

    fn size_hint(depth: usize) -> (usize, Option<usize>) {
        T::size_hint(depth)
    }
}

#[cfg(test)]
mod tests {
    use crate::Vec1;

    #[test]
    fn vec1_stays_nonempty_while_exposing_contiguous_values() {
        let mut values = Vec1::new(1);
        values.push(2);
        let mut tail = vec![3, 4];
        values.append(&mut tail);

        assert_eq!(values.first(), &1);
        assert_eq!(values.len(), 4);
        assert!(!values.is_empty());
        assert_eq!(values.as_slice(), &[1, 2, 3, 4]);
        assert_eq!(values.iter().copied().sum::<i32>(), 10);
        assert_eq!(&*values, &[1, 2, 3, 4]);
        assert!(tail.is_empty());
    }
}
