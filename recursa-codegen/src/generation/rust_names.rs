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

/// Projects declared generic parameters into application arguments in declaration order.
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

/// Parses a generated Rust identifier, falling back to raw-identifier syntax for keywords.
pub(super) fn emitted_ident(name: &str) -> Result<Ident, String> {
    syn::parse_str::<Ident>(name)
        .or_else(|_| syn::parse_str::<Ident>(&format!("r#{name}")))
        .map_err(|error| format!("cannot emit Rust identifier `{name}`: {error}"))
}

/// Constructs a `crate`-relative path from resolved module segments.
pub(super) fn emitted_crate_path(segments: &[String]) -> Result<syn::Path, String> {
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    Ok(path)
}

/// Constructs a `crate`-relative path from a resolved `::`-separated semantic key.
pub(super) fn emitted_semantic_path(semantic_key: &str) -> Result<syn::Path, String> {
    let segments = semantic_key
        .split("::")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if segments.iter().any(String::is_empty) {
        return Err(format!(
            "cannot emit empty segment in semantic key `{semantic_key}`",
        ));
    }
    emitted_crate_path(&segments)
}

/// Appends a generator-owned suffix to an authored identifier without retaining raw syntax.
pub(super) fn sibling_ident(ident: &Ident, suffix: &str) -> Ident {
    Ident::new(
        &format!("{}{suffix}", ident.unraw()),
        proc_macro2::Span::call_site(),
    )
}

/// Converts a semantic name to the stable snake-case spelling used by generated methods.
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

/// Converts a validated UTF-8 byte offset to one-based source line and column coordinates.
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

/// Normalizes a relative source path to slash-separated UTF-8 identity text.
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

/// Hashes a declaration's source location and name into its structural-loader anchor.
pub(super) fn declaration_loader_anchor(
    file: &str,
    line: usize,
    column: usize,
    name: &str,
) -> String {
    let input = format!("{file}:{line}:{column}:{name}");
    format!("{:016x}", loader_anchor_hash(input.as_bytes()))
}

/// Applies the loader-anchor hash algorithm fixed by the private generation protocol.
pub(super) fn loader_anchor_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Applies the stable identity hash used for canonical generation-plan content.
pub(super) fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf_29ce_4842_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
