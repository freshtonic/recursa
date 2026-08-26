//! Structural loaders for matching-version Recursa generated output.

use proc_macro::TokenStream;
use quote::quote;
use std::path::{Component, Path, PathBuf};
use syn::ext::IdentExt as _;
use syn::{DeriveInput, parse_macro_input};

const PRIVATE_PROTOCOL: u32 = 1;

/// Marks a source declaration as a grammar Node and loads its generated body.
///
/// The derive deliberately interprets no fields or grammar helper attributes.
#[proc_macro_derive(Node, attributes(lex, tok, sep, parse, pratt))]
pub fn node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;
    let span = ident.span();
    let start = span.start();
    let file = span
        .local_file()
        .and_then(|path| path.canonicalize().ok())
        .unwrap_or_else(|| Path::new(&span.file()).to_path_buf());
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    let relative_file = manifest
        .as_deref()
        .and_then(|manifest| file.strip_prefix(manifest).ok())
        .unwrap_or(&file);
    let source_identity = normalized_source_identity(relative_file);
    let anchor = format!(
        "{}:{}:{}:{}",
        source_identity,
        start.line,
        start.column + 1,
        ident.unraw(),
    );
    let loader_anchor_text = format!("{:016x}", stable_hash(anchor.as_bytes()));
    let loader_anchor = syn::LitStr::new(&loader_anchor_text, proc_macro2::Span::call_site());
    quote! {
        __recursa_load_node!(#ident, #loader_anchor);
    }
    .into()
}

/// Loads the complete Generation selected by the build script.
///
/// Only `recursa-codegen` interprets this macro's grammar configuration.
#[proc_macro]
pub fn grammar(_input: TokenStream) -> TokenStream {
    let release = format!("release_{}", env!("CARGO_PKG_VERSION")).replace(['.', '-'], "_");
    let release = syn::Ident::new(&release, proc_macro2::Span::call_site());
    let protocol = syn::Ident::new(
        &format!("protocol_{PRIVATE_PROTOCOL}"),
        proc_macro2::Span::call_site(),
    );
    quote! {
        include!(concat!(env!("OUT_DIR"), "/recursa-selected.rs"));
        __recursa_root_handshake!(#release, #protocol, core);
    }
    .into()
}

/// Structural placeholder for token declarations consumed by the analyzer.
#[proc_macro]
pub fn tokens(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn normalized_source_identity(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
            Component::ParentDir => Some("..".to_owned()),
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
        })
        .collect::<Vec<_>>()
        .join("/")
}
