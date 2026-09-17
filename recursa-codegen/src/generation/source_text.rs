//! Line-broken text for generated Rust sources.
//!
//! `TokenStream::to_string` renders a whole stream on one line. rustdoc resolves the source
//! column of every documented item by walking that item's line from its start, so a generated
//! file that is one multi-megabyte line makes `cargo doc` cost roughly items × line length; a
//! pg-sql sized grammar took over twenty-five minutes. Breaking the line after `;`, around
//! brace-delimited groups, and after outer attributes keeps each walk short without changing a
//! single token, so the text stays the same Rust the stream described.

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

/// Renders a token stream as Rust source text with one item or statement per line.
pub(super) fn render_source(tokens: &TokenStream) -> String {
    let mut out = String::new();
    write_stream(tokens, &mut out);
    trim_trailing_spaces(&mut out);
    out
}

/// Appends one stream, recursing into brace groups so nested items break lines too.
fn write_stream(tokens: &TokenStream, out: &mut String) {
    let mut attribute_prefix = false;
    for tree in tokens.clone() {
        match &tree {
            TokenTree::Group(group) => match group.delimiter() {
                Delimiter::Brace => {
                    out.push_str("{\n");
                    write_stream(&group.stream(), out);
                    end_line(out);
                    out.push_str("}\n");
                }
                Delimiter::Bracket if attribute_prefix => {
                    out.push_str(&group.to_string());
                    out.push('\n');
                }
                _ => {
                    out.push_str(&group.to_string());
                    out.push(' ');
                }
            },
            TokenTree::Punct(punct) => {
                out.push(punct.as_char());
                if punct.as_char() == ';' {
                    out.push('\n');
                } else if punct.spacing() == Spacing::Alone {
                    out.push(' ');
                }
            }
            TokenTree::Ident(_) | TokenTree::Literal(_) => {
                out.push_str(&tree.to_string());
                out.push(' ');
            }
        }
        attribute_prefix = match &tree {
            TokenTree::Punct(punct) if punct.as_char() == '#' => true,
            TokenTree::Punct(punct) if punct.as_char() == '!' => attribute_prefix,
            _ => false,
        };
    }
}

/// Ends the current line unless the text already ends one.
fn end_line(out: &mut String) {
    trim_trailing_spaces(out);
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Drops spaces left by token separators at the end of the text.
fn trim_trailing_spaces(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::render_source;
    use quote::quote;

    #[test]
    fn breaks_items_statements_and_attributes_onto_their_own_lines() {
        let tokens = quote! {
            #[doc = "A node."]
            pub struct Node<'a> { value: &'a str }
            impl<'a> Node<'a> {
                fn get(&self) -> &'a str { let v = self.value; v }
            }
            fn f() -> Result<(), String>;
        };
        let text = render_source(&tokens);
        assert_eq!(
            text,
            "# [doc = \"A node.\"]\npub struct Node < 'a > {\nvalue : & 'a str\n}\nimpl < 'a > Node < 'a > {\nfn get (& self) -> & 'a str {\nlet v = self . value ;\nv\n}\n}\nfn f () -> Result < () , String > ;\n"
        );
    }

    #[test]
    fn keeps_joint_punctuation_adjacent() {
        let tokens = quote! { a::b::<T>() => x..=y };
        assert_eq!(render_source(&tokens), "a :: b :: < T > () => x ..= y");
    }

    #[test]
    fn stays_equivalent_rust_and_bounds_line_length() {
        let tokens = quote! {
            #![allow(dead_code)]
            mod m {
                #[derive(Debug)]
                pub enum E { A, B(u8) }
                pub fn g(e: E) -> u8 { match e { E::A => 0, E::B(n) => n } }
            }
        };
        let text = render_source(&tokens);
        let parsed = syn::parse_file(&text).expect("line-broken text parses");
        let original = syn::parse2::<syn::File>(tokens).expect("original parses");
        assert_eq!(parsed, original);
        assert!(text.lines().all(|line| line.len() < 60), "{text}");
        assert!(text.starts_with("# ! [allow (dead_code)]\n"), "{text}");
    }
}
