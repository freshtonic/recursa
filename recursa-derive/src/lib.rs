//! Derive macros for the recursa parser framework.

mod scan_derive;

use proc_macro::TokenStream;

#[proc_macro_derive(Scan, attributes(scan))]
pub fn derive_scan(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match scan_derive::derive_scan(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(Parse, attributes(parse))]
pub fn derive_parse(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
