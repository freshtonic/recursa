use regex::Regex;

use crate::error::ParseError;

/// Leaf-level token matching via regex.
///
/// Defines a token's pattern, compiled regex, and how to construct it from matched text.
/// Types that implement `Scan` should also implement `Parse` (via `#[derive(Scan)]`
/// which generates both impls, or manually using the `impl_parse_for_scan!` macro).
///
/// `Scan` does not provide `peek` or `parse` methods — those live on the `Parse` trait.
/// The `impl_parse_for_scan!` macro and `#[derive(Scan)]` generate the `Parse` impl
/// that uses `regex()` and `from_match()` internally.
pub trait Scan<'input>: Sized {
    /// The regex pattern that matches this token (without `\A` anchor — added automatically).
    const PATTERN: &'static str;

    /// Returns the compiled, cached regex for this token.
    /// Implementations should use a `static OnceLock<Regex>` for caching.
    fn regex() -> &'static Regex;

    /// Construct this token from the matched text.
    fn from_match(matched: &'input str) -> Result<Self, ParseError>;
}
