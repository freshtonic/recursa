//! Generated Rust names, source anchors, and deterministic private hashes.
//!
//! These helpers form a narrow naming boundary rather than a generic utility module: every
//! item converts authored identity into emitted Rust or a stable generation-local anchor.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    macro_rules! focused_case {
        ($name:ident) => {
            #[test]
            fn $name() {
                test_support::$name();
            }
        };
    }
    focused_case!(snake_case_produces_stable_generated_element_names);
    focused_case!(synthetic_fixed_spelling_uppercases_only_ascii_insensitive_keywords);
    focused_case!(fixed_accessor_names_remain_valid_for_numeric_spellings);
}

/// Builds generic arguments from frozen inputs without changing canonical generation order.
pub(super) fn generic_arguments(generics: &Generics) -> Vec<proc_macro2::TokenStream> {
    generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => {
                let lifetime = &parameter.lifetime;
                quote!(#lifetime)
            }
            GenericParam::Type(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
            GenericParam::Const(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
        })
        .collect()
}

/// Builds emitted ident from frozen inputs without changing canonical generation order.
pub(super) fn emitted_ident(name: &str) -> Result<Ident, String> {
    syn::parse_str::<Ident>(name)
        .or_else(|_| syn::parse_str::<Ident>(&format!("r#{name}")))
        .map_err(|error| format!("cannot emit Rust identifier `{name}`: {error}"))
}

/// Builds emitted crate path from frozen inputs without changing canonical generation order.
pub(super) fn emitted_crate_path(segments: &[String]) -> Result<syn::Path, String> {
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    Ok(path)
}

/// Builds sibling ident from frozen inputs without changing canonical generation order.
pub(super) fn sibling_ident(ident: &Ident, suffix: &str) -> Ident {
    Ident::new(
        &format!("{}{suffix}", ident.unraw()),
        proc_macro2::Span::call_site(),
    )
}

/// Builds snake case from frozen inputs without changing canonical generation order.
pub(super) fn snake_case(value: &str) -> String {
    if value
        .chars()
        .any(|character| character.is_ascii_alphabetic())
        && value
            .chars()
            .all(|character| !character.is_ascii_alphabetic() || character.is_ascii_uppercase())
    {
        let lowered = value.to_ascii_lowercase();
        let trimmed = lowered.trim_matches('_');
        return if trimmed.is_empty() {
            "element".to_owned()
        } else {
            trimmed.to_owned()
        };
    }
    let mut output = String::new();
    let mut previous_was_separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase()
                && !output.is_empty()
                && !previous_was_separator
                && output
                    .chars()
                    .last()
                    .is_some_and(|last| last.is_ascii_lowercase())
            {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !output.is_empty() && !previous_was_separator {
            output.push('_');
            previous_was_separator = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        "element".to_owned()
    } else {
        output
    }
}

/// Builds source line column from frozen inputs without changing canonical generation order.
pub(super) fn source_line_column(source: &str, byte_offset: u32) -> Result<(usize, usize), String> {
    let prefix = source
        .get(..byte_offset as usize)
        .ok_or_else(|| "the Node item position is not a UTF-8 source boundary".to_owned())?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    Ok((line, column))
}

/// Builds normalized source identity from frozen inputs without changing canonical generation order.
pub(super) fn normalized_source_identity(path: &Path) -> Result<String, String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => segments.push(
                segment
                    .to_str()
                    .ok_or_else(|| "the Node source path is not valid UTF-8".to_owned())?,
            ),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("the Node source identity is not manifest-relative".to_owned());
            }
        }
    }
    if segments.is_empty() {
        return Err("the Node source identity is empty".to_owned());
    }
    Ok(segments.join("/"))
}

/// Builds declaration loader anchor from frozen inputs without changing canonical generation order.
pub(super) fn declaration_loader_anchor(
    file: &str,
    line: usize,
    column: usize,
    name: &str,
) -> String {
    let input = format!("{file}:{line}:{column}:{name}");
    format!("{:016x}", loader_anchor_hash(input.as_bytes()))
}

/// Builds loader anchor hash from frozen inputs without changing canonical generation order.
pub(super) fn loader_anchor_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Builds stable hash from frozen inputs without changing canonical generation order.
pub(super) fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf_29ce_4842_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
