use std::ops::Deref;

use crate::Span;

/// Compact span metadata for one generated node and its grammar elements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpanBlock<const N: usize> {
    node: Span,
    elements: Box<[Span; N]>,
}

impl<const N: usize> SpanBlock<N> {
    /// Creates source-free metadata for a programmatically constructed node.
    pub fn synthetic() -> Self {
        Self {
            node: Span::SYNTHETIC,
            elements: Box::new([Span::SYNTHETIC; N]),
        }
    }

    /// The full source extent of the node.
    pub fn node(&self) -> Span {
        self.node
    }

    pub(crate) fn set_node(&mut self, span: Span) {
        self.node = span;
    }

    pub(crate) fn element(&self, index: usize) -> Option<Span> {
        self.elements.get(index).copied()
    }

    pub(crate) fn set_element(&mut self, index: usize, span: Span) -> bool {
        let Some(element) = self.elements.get_mut(index) else {
            return false;
        };
        *element = span;
        true
    }
}

/// Uniform optional source extent for parsed nodes.
pub trait HasSpan {
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
}

impl<T> Deref for Vec1<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use crate::{Span, SpanBlock, Vec1};

    #[test]
    fn span_blocks_start_synthetic_and_check_element_bounds() {
        let mut block = SpanBlock::<2>::synthetic();
        assert_eq!(block.node(), Span::SYNTHETIC);
        assert_eq!(block.element(0), Some(Span::SYNTHETIC));
        assert_eq!(block.element(2), None);

        let real = Span::new(2, 3).unwrap();
        assert!(block.set_element(1, real));
        assert!(!block.set_element(2, real));
        assert_eq!(block.element(1), Some(real));
    }

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
