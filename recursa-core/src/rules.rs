use std::sync::OnceLock;

use regex::Regex;

/// Configuration for a grammar's ignored content (whitespace, comments, etc.).
///
/// `IGNORE` is a regex pattern matched and skipped between tokens during parsing.
/// It must be a const so derive macros can splice it into lookahead regexes at compile time.
///
/// Each implementation must provide `ignore_cache()` returning a reference to a
/// per-type `OnceLock<Regex>`. This ensures each rules type gets its own cached
/// compiled regex. The `ignore_regex()` default method handles lazy compilation.
pub trait ParseRules {
    const IGNORE: &'static str;

    /// Return a reference to this type's `OnceLock<Regex>` for caching the compiled
    /// ignore pattern. Each implementation should have its own function-local static.
    fn ignore_cache() -> &'static OnceLock<Regex>;

    /// Return the compiled ignore regex, or `None` if `IGNORE` is empty.
    /// Lazily compiled on first call and cached via `ignore_cache()`.
    fn ignore_regex() -> Option<&'static Regex> {
        if Self::IGNORE.is_empty() {
            return None;
        }
        Some(Self::ignore_cache().get_or_init(|| {
            Regex::new(&format!(r"\A(?:{})", Self::IGNORE)).unwrap()
        }))
    }
}

/// No-op rules for `Scan` types that don't skip whitespace.
pub struct NoRules;

impl ParseRules for NoRules {
    const IGNORE: &'static str = "";

    fn ignore_cache() -> &'static OnceLock<Regex> {
        static CACHE: OnceLock<Regex> = OnceLock::new();
        &CACHE
    }
}
