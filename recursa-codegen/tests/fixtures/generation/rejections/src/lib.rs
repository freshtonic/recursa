pub mod grammar;

/// Folds an option name as PostgreSQL's scanner does: a quoted name keeps its spelling, and an
/// unquoted one is lowercased.
pub fn fold_name(raw: &str) -> Option<std::borrow::Cow<'_, str>> {
    match raw.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')) {
        Some(inner) => Some(std::borrow::Cow::Borrowed(inner)),
        None => Some(std::borrow::Cow::Owned(raw.to_ascii_lowercase())),
    }
}

/// Folds a width to the range it falls in, as the `opt_float` action reads its `Iconst`: below 1,
/// within the limit, or above 64.
pub fn width_class(raw: &str) -> Option<&'static str> {
    let width = raw.parse::<u64>().ok()?;
    Some(match width {
        0 => "zero",
        1..=64 => "fits",
        _ => "wide",
    })
}
