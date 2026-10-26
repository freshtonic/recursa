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

/// The refusal of a pair that is not two names.
pub static TWO_NAMES: recursa::RejectionNote = recursa::RejectionNote::new(
    "Pair",
    Some("42601"),
    "a pair holds exactly two names",
    None,
    "gram.y REL_17_11 15002, a_expr",
);

/// The refusal of a list of more than two names.
pub static AT_MOST_TWO: recursa::RejectionNote = recursa::RejectionNote::new(
    "Bound",
    Some("42601"),
    "a frame holds at most two names",
    None,
    "fixture",
);

/// Refuses a pair that is not two names, at the extent of its list.
pub fn two_names(pair: &grammar::Pair<'_>) -> Result<(), recursa::Refusal> {
    if pair.names.len() == 2 {
        Ok(())
    } else {
        Err(recursa::Refusal::new(&TWO_NAMES).at_field(0))
    }
}

/// Refuses a flat pair that is not two names, at the extent of its list.
#[cfg(feature = "flat-ast")]
pub fn two_names_flat(
    pair: &grammar::FlatPair,
    ast: &grammar::FlatAst<'_, grammar::FlatPair>,
) -> Result<(), recursa::Refusal> {
    if ast[pair.names].len() == 2 {
        Ok(())
    } else {
        Err(recursa::Refusal::new(&TWO_NAMES).at_field(0))
    }
}

/// Refuses a frame of more than two names, at the extent of the whole bound.
pub fn at_most_two(bound: &grammar::Bound<'_>) -> Result<(), recursa::Refusal> {
    match bound {
        grammar::Bound::Many(names) if names.len() > 2 => {
            Err(recursa::Refusal::new(&AT_MOST_TWO))
        }
        _ => Ok(()),
    }
}

/// Refuses a flat frame of more than two names, at the extent of the whole bound.
#[cfg(feature = "flat-ast")]
pub fn at_most_two_flat(
    bound: &grammar::FlatBound,
    ast: &grammar::FlatAst<'_, grammar::FlatBound>,
) -> Result<(), recursa::Refusal> {
    match bound {
        grammar::FlatBound::Many(names) if ast[*names].len() > 2 => {
            Err(recursa::Refusal::new(&AT_MOST_TWO))
        }
        _ => Ok(()),
    }
}
