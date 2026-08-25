use std::fmt;

use crate::{FollowSet, Input};

/// A low-level parse failure.
///
/// Its structured diagnostic fields are added with the symbolic-failure
/// runtime slice; this type exists now to complete the stable Parse contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    _private: (),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("parse failed")
    }
}

impl std::error::Error for ParseError {}

/// Parsing contract implemented by Recursa-generated grammar types.
pub trait Parse<'i, A = ()>: Sized {
    type Out;

    fn parse(input: &mut Input<'i, A>, follow: &FollowSet) -> Result<Self::Out, ParseError>;

    fn parse_spanned(
        input: &mut Input<'i, A>,
        follow: &FollowSet,
    ) -> Result<Self::Out, ParseError> {
        Self::parse(input, follow)
    }
}
