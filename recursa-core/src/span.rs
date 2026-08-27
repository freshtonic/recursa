use std::fmt;
use std::ops::Range;

/// Checked byte offsets in source text.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Span {
    /// Inclusive checked byte offset, or the synthetic sentinel.
    start: u32,
    /// Exclusive checked byte offset, or the synthetic sentinel.
    end: u32,
}

impl Span {
    /// A source-free location used for constructed nodes.
    pub const SYNTHETIC: Self = Self {
        start: u32::MAX,
        end: u32::MAX,
    };

    /// Creates a real half-open byte span after checking ordering and capacity.
    pub fn new(start: usize, end: usize) -> Result<Self, OffsetOverflow> {
        if start > end || end >= u32::MAX as usize {
            return Err(OffsetOverflow);
        }
        Ok(Self {
            start: start as u32,
            end: end as u32,
        })
    }

    /// Returns the inclusive start byte offset.
    pub const fn start(self) -> u32 {
        self.start
    }

    /// Returns the exclusive end byte offset.
    pub const fn end(self) -> u32 {
        self.end
    }

    /// Converts the checked offsets to a standard half-open range.
    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    /// Reports whether this span belongs to a synthesized value without source.
    pub const fn is_synthetic(self) -> bool {
        self.start == u32::MAX && self.end == u32::MAX
    }
}

/// A byte range could not be represented without colliding with `SYNTHETIC`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OffsetOverflow;

impl fmt::Display for OffsetOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("source span exceeds Recursa's supported byte range")
    }
}

impl std::error::Error for OffsetOverflow {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_spans_cannot_reach_the_synthetic_sentinel() {
        assert!(Span::SYNTHETIC.is_synthetic());
        assert!(Span::new(3, 2).is_err());
        assert!(Span::new(0, u32::MAX as usize).is_err());
        assert!(!Span::new(0, u32::MAX as usize - 1).unwrap().is_synthetic());
    }
}
