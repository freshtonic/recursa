//! Proc-macro attribute for `recursa-diagram`.
//!
//! Provides the `#[railroad]` attribute macro that generates railroad syntax
//! diagrams for recursa-derived AST types. Runtime layout and SVG rendering
//! live in the sibling `recursa-diagram-core` crate.

use proc_macro::TokenStream;

mod macro_impl;

#[proc_macro_attribute]
pub fn railroad(attr: TokenStream, item: TokenStream) -> TokenStream {
    macro_impl::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
