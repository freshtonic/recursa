//! Derive macros for the recursa parser framework.

use proc_macro::TokenStream;

#[proc_macro_derive(Scan, attributes(scan))]
pub fn derive_scan(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

#[proc_macro_derive(Parse, attributes(parse))]
pub fn derive_parse(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
