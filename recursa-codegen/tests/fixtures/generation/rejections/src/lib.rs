pub mod grammar;

/// Folds an option name as PostgreSQL's scanner does: a quoted name keeps its spelling, and an
/// unquoted one is lowercased.
pub fn fold_name(raw: &str) -> Option<std::borrow::Cow<'_, str>> {
    match raw.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')) {
        Some(inner) => Some(std::borrow::Cow::Borrowed(inner)),
        None => Some(std::borrow::Cow::Owned(raw.to_ascii_lowercase())),
    }
}
