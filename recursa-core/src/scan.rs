use regex::Regex;

use crate::error::ParseError;
use crate::input::Input;

/// Leaf-level token matching via regex.
///
/// Each token type implements `Scan` with a regex pattern and a constructor.
/// Types that implement `Scan` should also implement `Parse` (via `#[derive(Scan)]`
/// which generates both impls, or manually using the `impl_parse_for_scan!` macro).
pub trait Scan<'input>: Sized {
    /// The regex pattern that matches this token (without `\A` anchor -- added automatically).
    const PATTERN: &'static str;

    /// Returns the compiled, cached regex for this token.
    /// Implementations should use a `static OnceLock<Regex>` for caching.
    fn regex() -> &'static Regex;

    /// Construct this token from the matched text.
    fn from_match(matched: &'input str) -> Result<Self, ParseError>;

    /// Check whether this token can be parsed at the current position without advancing.
    fn peek(input: &Input<'input>) -> bool {
        Self::regex().is_match(input.remaining())
    }

    /// Attempt to parse this token, advancing the input on success.
    fn parse(input: &mut Input<'input>) -> Result<Self, ParseError> {
        match Self::regex().find(input.remaining()) {
            Some(m) if m.start() == 0 => {
                let matched = &input.source()[input.cursor()..input.cursor() + m.len()];
                let result = Self::from_match(matched)?;
                input.advance(m.len());
                Ok(result)
            }
            Some(_) | None => Err(ParseError::new(
                input.source().to_string(),
                input.cursor()..input.cursor(),
                Self::PATTERN,
            )),
        }
    }
}
