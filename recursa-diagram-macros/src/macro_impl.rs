//! Expansion logic for `#[railroad]`.
//!
//! The macro parses the annotated item, builds a layout tree from
//! `recursa_diagram_core::layout`, renders it to SVG via
//! `recursa_diagram_core::render`, and emits the original item with the SVG
//! attached as a doc comment.

use proc_macro2::TokenStream;
use quote::quote;
use recursa_diagram_core::layout::{
    Choice, NonTerminal, OneOrMore, Optional, Sequence, Terminal, Token,
};
use recursa_diagram_core::{layout::Node, render};
use syn::parse::Parser;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Fields, GenericArgument, Ident, Lit, LitStr,
    MetaNameValue, PathArguments, Type, parse2, punctuated::Punctuated, token::Comma,
};

/// Maximum single-row width (in SVG user units) for the top-level `Sequence`
/// of a struct rendered via `#[railroad]`. When the natural single-row layout
/// exceeds this, the sequence is broken into multiple rows joined by wrap
/// connectors. Chosen so that typical rustdoc pages do not require horizontal
/// scrolling while still giving each row enough space for ~10-15 clauses.
///
/// **Soft cap, not a hard limit.** A single child wider than this value
/// gets its own row (the greedy packer admits it to avoid an infinite loop),
/// so the final rendered width can exceed `DEFAULT_MAX_WIDTH` by up to the
/// width of the widest single child plus `CHOICE_RAIL_WIDTH` of back-rail
/// margin. Observed in practice: `SelectStmt` wraps to 1226px. If tighter
/// bounds become necessary, either shrink the cap or add a `#[railroad(...)]`
/// attribute to override it per-type.
const DEFAULT_MAX_WIDTH: u32 = 1200;

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let attrs = parse_type_attrs(attr)?;

    let node = build_node(&input, &attrs)?;
    let svg = render(&node);
    // Rustdoc parses doc attributes as CommonMark. A raw `<svg>` is recognized
    // as an HTML block, but HTML blocks terminate at the first blank line —
    // and the railroad crate's default stylesheet contains blank lines inside
    // its `<style>` element. Without this, rustdoc re-enters markdown mode
    // partway through the SVG and wraps CSS rules in `<p>` tags, corrupting
    // the diagram. Collapse blank lines so the SVG stays a single HTML block.
    let svg = svg
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let doc = format!("\n\n{svg}\n\n");

    let body = strip_body(&input);

    Ok(quote! {
        #[doc = #doc]
        #body
    })
}

#[derive(Debug, Default)]
struct TypeAttrs {
    label: Option<String>,
    /// Base URL prefix used to construct hrefs for fallthrough
    /// `NonTerminal` references. When set, a field of type `Foo` is
    /// rendered as a `NonTerminal` whose href is
    /// `{crate_path}/{Foo}.html`. Without this, hrefs are omitted —
    /// proc macros can't resolve the calling module path themselves,
    /// so the user must supply one explicitly when they want clickable
    /// navigation between productions in rustdoc output.
    crate_path: Option<String>,
}

fn parse_type_attrs(attr: TokenStream) -> syn::Result<TypeAttrs> {
    let mut out = TypeAttrs::default();
    if attr.is_empty() {
        return Ok(out);
    }
    let nvs = Punctuated::<MetaNameValue, Comma>::parse_terminated.parse2(attr)?;
    for nv in nvs {
        let key = if nv.path.is_ident("label") {
            "label"
        } else if nv.path.is_ident("crate_path") {
            "crate_path"
        } else {
            return Err(syn::Error::new_spanned(nv.path, "unknown attribute key"));
        };
        let Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) = nv.value
        else {
            return Err(syn::Error::new_spanned(nv.path, "expected string literal"));
        };
        match key {
            "label" => {
                if out.label.is_some() {
                    return Err(syn::Error::new_spanned(nv.path, "duplicate `label`"));
                }
                out.label = Some(s.value());
            }
            "crate_path" => {
                if out.crate_path.is_some() {
                    return Err(syn::Error::new_spanned(nv.path, "duplicate `crate_path`"));
                }
                let mut v = s.value();
                while v.ends_with('/') {
                    v.pop();
                }
                out.crate_path = Some(v);
            }
            _ => unreachable!(),
        }
    }
    Ok(out)
}

fn build_node(input: &DeriveInput, attrs: &TypeAttrs) -> syn::Result<Node> {
    if let Some(label) = &attrs.label {
        return Ok(Node::Terminal(Terminal::new(label)));
    }
    let cx = Ctx {
        crate_path: attrs.crate_path.as_deref(),
    };
    match &input.data {
        Data::Struct(s) => match &s.fields {
            // Bare `#[railroad]` on `struct Foo;` has nothing structural to
            // render, so fall back to a NonTerminal bearing the type name.
            // This also gives the user a visible label in the rendered diagram.
            Fields::Unit => Ok(Node::NonTerminal(NonTerminal::new(
                input.ident.to_string(),
                None,
            ))),
            _ => {
                // Only the *top-level* Sequence wraps. Nested sequences inside
                // Surrounded/OneOrMore/etc. continue to use `Sequence::new`,
                // because wrapping them would produce visually confusing
                // nested row structures. We rebuild a wrapped Sequence from
                // the children of the flat Sequence returned by
                // `build_from_fields`.
                let inner = build_from_fields(&s.fields, cx)?;
                if let Node::Sequence(flat) = inner {
                    Ok(Node::Sequence(Sequence::wrapped(
                        flat.children,
                        DEFAULT_MAX_WIDTH,
                    )))
                } else {
                    Ok(inner)
                }
            }
        },
        Data::Enum(e) => {
            if e.variants.is_empty() {
                return Err(syn::Error::new_spanned(
                    input,
                    "empty enum has nothing to render",
                ));
            }
            // Per CLAUDE.md, recursa enum variants are single-field tuple
            // variants wrapping a `Parse`-implementing type. We recognise that
            // shape by recursing into the inner type. A `#[railroad(label =
            // "...")]` on the variant overrides the default rendering with a
            // literal terminal carrying the supplied label. Anything else
            // (unit, multi-field tuple, named) is non-conforming, but we
            // still produce a defensive `NonTerminal(VariantIdent)` rather
            // than failing.
            let mut children: Vec<Node> = Vec::with_capacity(e.variants.len());
            for v in &e.variants {
                let variant_attrs = parse_variant_attrs(&v.attrs)?;
                let node = if let Some(label) = variant_attrs.label {
                    Node::Terminal(Terminal::new(label))
                } else {
                    match &v.fields {
                        Fields::Unnamed(u) if u.unnamed.len() == 1 => {
                            recognize(&u.unnamed[0].ty, cx)
                        }
                        _ => Node::NonTerminal(NonTerminal::new(v.ident.to_string(), None)),
                    }
                };
                children.push(node);
            }
            // Default to the first declared variant; we have no semantic
            // information to choose otherwise.
            Ok(Node::Choice(Choice::new(0, children)))
        }
        Data::Union(_) => Err(syn::Error::new_spanned(input, "unions are not supported")),
    }
}

/// Carries type-level attribute context down through the recursive node
/// builders. Currently only the optional `crate_path` for href resolution.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    crate_path: Option<&'a str>,
}

fn build_from_fields(fields: &Fields, cx: Ctx<'_>) -> syn::Result<Node> {
    let iter: Box<dyn Iterator<Item = &syn::Field>> = match fields {
        Fields::Named(n) => Box::new(n.named.iter()),
        Fields::Unnamed(u) => Box::new(u.unnamed.iter()),
        Fields::Unit => unreachable!("Fields::Unit handled in build_node"),
    };
    let mut children = Vec::new();
    for field in iter {
        let field_attrs = parse_field_attrs(&field.attrs)?;
        if field_attrs.skip {
            continue;
        }
        children.push(node_for_field_type(&field.ty, &field_attrs, cx));
    }
    Ok(Node::Sequence(Sequence::new(children)))
}

#[derive(Default)]
struct FieldAttrs {
    label: Option<String>,
    skip: bool,
}

fn parse_field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs::default();
    for a in attrs {
        if !a.path().is_ident("railroad") {
            continue;
        }
        a.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                if out.label.is_some() {
                    return Err(meta.error("duplicate `label`"));
                }
                let s: LitStr = meta.value()?.parse()?;
                out.label = Some(s.value());
            } else if meta.path.is_ident("skip") {
                if out.skip {
                    return Err(meta.error("duplicate `skip`"));
                }
                out.skip = true;
            } else {
                return Err(meta.error("unknown key"));
            }
            Ok(())
        })?;
    }
    if out.skip && out.label.is_some() {
        return Err(syn::Error::new_spanned(
            &attrs[0],
            "`skip` and `label` are mutually exclusive",
        ));
    }
    Ok(out)
}

#[derive(Default)]
struct VariantAttrs {
    label: Option<String>,
}

fn parse_variant_attrs(attrs: &[Attribute]) -> syn::Result<VariantAttrs> {
    let mut out = VariantAttrs::default();
    for a in attrs {
        if !a.path().is_ident("railroad") {
            continue;
        }
        a.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                if out.label.is_some() {
                    return Err(meta.error("duplicate `label`"));
                }
                let s: LitStr = meta.value()?.parse()?;
                out.label = Some(s.value());
            } else {
                return Err(meta.error("unknown key"));
            }
            Ok(())
        })?;
    }
    Ok(out)
}

fn node_for_field_type(ty: &Type, field_attrs: &FieldAttrs, cx: Ctx<'_>) -> Node {
    if let Some(label) = &field_attrs.label {
        return Node::NonTerminal(NonTerminal::new(label, None));
    }
    recognize(ty, cx)
}

fn recognize(ty: &Type, cx: Ctx<'_>) -> Node {
    // Tuple types like `(keyword::If, keyword::Not, keyword::Exists)` are how
    // multi-token keyword sequences are spelled in the AST. Render each
    // element through `recognize` and wrap them in a flat `Sequence` so the
    // diagram shows `IF NOT EXISTS` instead of the stringified
    // `(IF, NOT, EXISTS)` that the fallthrough produces.
    if let Type::Tuple(t) = ty {
        let children: Vec<Node> = t.elems.iter().map(|e| recognize(e, cx)).collect();
        return Node::Sequence(Sequence::new(children));
    }
    // Direct reference to a `keyword::*` type (no PhantomData wrapper). Common
    // shape inside enum variants like `JoinType::Left(keyword::Left)`. We
    // render these as terminals with the uppercased last-segment ident, same
    // convention as the `PhantomData<keyword::T>` case below.
    if is_keyword_path(ty) {
        let label = type_label(ty).to_uppercase();
        return Node::Terminal(Terminal::new(label));
    }
    // Punctuation/operator tokens live in `punct::*` (see `pg-sql/src/tokens.rs`).
    // Render as `Token` so EXTRA_CSS in svg.rs colours them distinctly from
    // SQL keywords. We use the punctuation's ident (`Comma`, `LParen`, ...)
    // as the label rather than the underlying glyph since the latter is not
    // available at macro-expansion time.
    if is_punct_path(ty) {
        return Node::Token(Token::new(type_label(ty)));
    }
    if let Some((ident, args)) = outer_generic(ty) {
        match ident.to_string().as_str() {
            "Option" if args.len() == 1 => {
                return Node::Optional(Optional::new(recognize(&args[0], cx)));
            }
            "Seq" | "Punctuated" if args.len() == 2 => {
                let child = recognize(&args[0], cx);
                let sep = recognize(&args[1], cx);
                return Node::OneOrMore(OneOrMore::new(child, Some(sep)));
            }
            "Vec" if args.len() == 1 => {
                return Node::OneOrMore(OneOrMore::new(recognize(&args[0], cx), None));
            }
            "Surrounded" if args.len() == 3 => {
                return Node::Sequence(Sequence::new(vec![
                    recognize(&args[0], cx),
                    recognize(&args[1], cx),
                    recognize(&args[2], cx),
                ]));
            }
            "Box" | "Rc" | "Arc" if args.len() == 1 => {
                return recognize(&args[0], cx);
            }
            // `PhantomData<T>` is the convention pg-sql uses for keyword
            // markers: a parsed-and-discarded token that carries no runtime
            // data. Render it as a literal terminal box. We uppercase the
            // type's last-segment ident because keyword types use TitleCase
            // identifiers (e.g. `keyword::Drop`) whereas the SQL keyword
            // they represent is uppercase (`DROP`). The convention is
            // codebase-specific; if a non-keyword `PhantomData<T>` ever
            // appears its terminal label will still be readable, just
            // shoutier than expected.
            "PhantomData" if args.len() == 1 => {
                let label = type_label(&args[0]).to_uppercase();
                return Node::Terminal(Terminal::new(label));
            }
            _ => {}
        }
    }
    // Fallthrough: treat as a reference to another production. When a
    // `crate_path` was supplied via `#[railroad(crate_path = "...")]`, build
    // an href like `{crate_path}/{TypeName}.html` so the rendered SVG box
    // becomes a clickable link to the referenced type's rustdoc page.
    let name = type_label(ty);
    let href = cx.crate_path.map(|base| format!("{base}/{name}.html"));
    Node::NonTerminal(NonTerminal::new(name, href))
}

/// Detect a type path whose second-to-last segment is `keyword`, e.g.
/// `keyword::Left`, `pg_sql::keyword::Drop`, or `crate::keyword::Where`.
/// Used by `recognize` to render keyword references as literal terminals.
fn is_keyword_path(ty: &Type) -> bool {
    is_module_path(ty, "keyword")
}

/// Detect a type path whose second-to-last segment is `punct` (the
/// pg-sql convention for punctuation/operator tokens — see
/// `pg-sql/src/tokens.rs`'s `pub mod punct`). Used by `recognize` to
/// render these as `Token` nodes.
fn is_punct_path(ty: &Type) -> bool {
    is_module_path(ty, "punct")
}

fn is_module_path(ty: &Type, module: &str) -> bool {
    let Type::Path(p) = ty else {
        return false;
    };
    let segs = &p.path.segments;
    if segs.len() < 2 {
        return false;
    }
    segs[segs.len() - 2].ident == module
}

fn outer_generic(ty: &Type) -> Option<(&Ident, Vec<Type>)> {
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && let PathArguments::AngleBracketed(ab) = &seg.arguments
    {
        let args: Vec<_> = ab
            .args
            .iter()
            .filter_map(|a| {
                if let GenericArgument::Type(t) = a {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .collect();
        return Some((&seg.ident, args));
    }
    None
}

fn type_label(ty: &Type) -> String {
    if let Type::Path(p) = ty
        && let Some(last) = p.path.segments.last()
    {
        return last.ident.to_string();
    }
    quote!(#ty).to_string()
}

fn strip_body(input: &DeriveInput) -> TokenStream {
    let mut clone = input.clone();
    clone.attrs.retain(|a| !a.path().is_ident("railroad"));
    // Also strip `#[railroad(...)]` attributes from fields and variants —
    // they are macro-internal metadata and the compiler would otherwise
    // reject them as unknown attributes on the emitted item.
    let strip = |attrs: &mut Vec<Attribute>| attrs.retain(|a| !a.path().is_ident("railroad"));
    match &mut clone.data {
        Data::Struct(s) => match &mut s.fields {
            Fields::Named(n) => n.named.iter_mut().for_each(|f| strip(&mut f.attrs)),
            Fields::Unnamed(u) => u.unnamed.iter_mut().for_each(|f| strip(&mut f.attrs)),
            Fields::Unit => {}
        },
        Data::Enum(e) => {
            for v in &mut e.variants {
                strip(&mut v.attrs);
                match &mut v.fields {
                    Fields::Named(n) => n.named.iter_mut().for_each(|f| strip(&mut f.attrs)),
                    Fields::Unnamed(u) => u.unnamed.iter_mut().for_each(|f| strip(&mut f.attrs)),
                    Fields::Unit => {}
                }
            }
        }
        Data::Union(_) => {}
    }
    quote! { #clone }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn node_for(tokens: TokenStream) -> Node {
        let input: DeriveInput = parse2(tokens).unwrap();
        build_node(&input, &TypeAttrs::default()).unwrap()
    }

    fn node_for_with(tokens: TokenStream, attrs: TypeAttrs) -> Node {
        let input: DeriveInput = parse2(tokens).unwrap();
        build_node(&input, &attrs).unwrap()
    }

    #[test]
    fn tuple_type_renders_as_inline_sequence_of_keywords() {
        // (keyword::If, keyword::Not, keyword::Exists) should become a
        // three-element Sequence of Terminals — not the stringified
        // NonTerminal `(If, Not, Exists)` produced by the fallthrough.
        let node = node_for(quote! {
            pub struct S { ine: (keyword::If, keyword::Not, keyword::Exists) }
        });
        match node {
            Node::Sequence(outer) => match &outer.children[0] {
                Node::Sequence(inner) => {
                    assert_eq!(inner.children.len(), 3);
                    let labels: Vec<_> = inner
                        .children
                        .iter()
                        .map(|c| match c {
                            Node::Terminal(t) => t.text.clone(),
                            other => panic!("expected Terminal, got {other:?}"),
                        })
                        .collect();
                    assert_eq!(labels, vec!["IF", "NOT", "EXISTS"]);
                }
                other => panic!("expected inner Sequence, got {other:?}"),
            },
            other => panic!("expected outer Sequence, got {other:?}"),
        }
    }

    #[test]
    fn punct_path_renders_as_token() {
        // A `punct::Comma` field becomes a Token (rounded-rect rendered
        // with the `terminal token` CSS class), distinct from a keyword.
        let node = node_for(quote! { pub struct S { c: punct::Comma } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::Token(t) => assert_eq!(t.text, "Comma"),
                other => panic!("expected Token, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn crate_path_adds_href_to_fallthrough_non_terminal() {
        let node = node_for_with(
            quote! { pub struct S { x: Foo } },
            TypeAttrs {
                label: None,
                crate_path: Some("../foo".to_owned()),
            },
        );
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "Foo");
                    assert_eq!(nt.href.as_deref(), Some("../foo/Foo.html"));
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn crate_path_does_not_add_href_to_keyword_or_token() {
        // Keywords and tokens are literal text, not references — they
        // should never get an href even when crate_path is supplied.
        let node = node_for_with(
            quote! { pub struct S { kw: keyword::Select, c: punct::Comma } },
            TypeAttrs {
                label: None,
                crate_path: Some("../foo".to_owned()),
            },
        );
        match node {
            Node::Sequence(seq) => {
                assert!(matches!(seq.children[0], Node::Terminal(_)));
                assert!(matches!(seq.children[1], Node::Token(_)));
            }
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn variant_label_overrides_inner_type_render() {
        let node = node_for(quote! {
            pub enum E {
                #[railroad(label = "ASC")]
                Asc(AscKw),
                #[railroad(label = "DESC")]
                Desc(DescKw),
            }
        });
        match node {
            Node::Choice(ch) => {
                assert_eq!(ch.children.len(), 2);
                let labels: Vec<_> = ch
                    .children
                    .iter()
                    .map(|c| match c {
                        Node::Terminal(t) => t.text.clone(),
                        other => panic!("expected Terminal, got {other:?}"),
                    })
                    .collect();
                assert_eq!(labels, vec!["ASC", "DESC"]);
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_crate_path_is_rejected() {
        let attr: TokenStream = quote! { crate_path = "a", crate_path = "b" };
        let err = parse_type_attrs(attr).expect_err("expected duplicate-crate_path error");
        assert!(
            err.to_string().contains("duplicate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn variant_railroad_attrs_are_stripped_from_emitted_body() {
        // The emitted item must not retain `#[railroad(...)]` on variants
        // or fields, otherwise rustc rejects them as unknown attributes.
        let item: TokenStream = quote! {
            pub enum E {
                #[railroad(label = "ASC")]
                Asc(AscKw),
            }
        };
        let out = expand(quote! {}, item).unwrap().to_string();
        // The SVG embedded in the doc attribute legitimately contains the
        // word "railroad" (class names), so look for the attribute syntax
        // `# [railroad` which only appears as a leftover variant/field attr.
        assert!(
            !out.contains("# [railroad"),
            "expected railroad attrs to be stripped from emitted body, got: {out}"
        );
    }

    #[test]
    fn named_struct_fields_render_as_sequence() {
        let node = node_for(quote! {
            pub struct ArgList { name: Ident, comma: Comma, value: Ident }
        });
        match node {
            Node::Sequence(seq) => assert_eq!(seq.children.len(), 3),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn tuple_struct_fields_render_as_sequence() {
        let node = node_for(quote! { pub struct Pair(Ident, Ident); });
        match node {
            Node::Sequence(seq) => assert_eq!(seq.children.len(), 2),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn unit_struct_falls_back_to_non_terminal_with_ident() {
        let node = node_for(quote! { pub struct Empty; });
        match node {
            Node::NonTerminal(nt) => {
                assert_eq!(nt.text, "Empty");
                assert!(nt.href.is_none());
            }
            other => panic!("expected NonTerminal, got {other:?}"),
        }
    }

    #[test]
    fn vec_renders_as_one_or_more_without_separator() {
        let node = node_for(quote! { pub struct S { xs: Vec<Foo> } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::OneOrMore(om) => assert!(om.separator.is_none()),
                other => panic!("expected OneOrMore, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn punctuated_renders_as_one_or_more_with_separator() {
        let node = node_for(quote! { pub struct S { xs: Punctuated<Foo, Comma> } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::OneOrMore(om) => assert!(om.separator.is_some()),
                other => panic!("expected OneOrMore, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_type_label_is_rejected() {
        let attr: TokenStream = quote! { label = "A", label = "B" };
        let err = parse_type_attrs(attr).expect_err("expected duplicate-label error");
        assert!(
            err.to_string().contains("duplicate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_field_label_is_rejected() {
        // We can't easily call parse_field_attrs through build_node because
        // duplicate label will fail before any node is built; assert via the
        // expand entry instead.
        let item: TokenStream = quote! {
            pub struct S {
                #[railroad(label = "A", label = "B")]
                f: Foo,
            }
        };
        let err = expand(quote! {}, item).expect_err("expected duplicate-label error");
        assert!(
            err.to_string().contains("duplicate"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn option_renders_as_optional() {
        let node = node_for(quote! { pub struct S { x: Option<Foo> } });
        match node {
            Node::Sequence(seq) => {
                assert_eq!(seq.children.len(), 1);
                assert!(matches!(seq.children[0], Node::Optional(_)));
            }
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn seq_renders_as_one_or_more() {
        let node = node_for(quote! { pub struct S { xs: Seq<Foo, Comma> } });
        match node {
            Node::Sequence(seq) => assert!(matches!(seq.children[0], Node::OneOrMore(_))),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn surrounded_renders_as_sequence_of_three() {
        let node = node_for(quote! { pub struct S { g: Surrounded<LParen, Foo, RParen> } });
        match node {
            Node::Sequence(outer) => match &outer.children[0] {
                Node::Sequence(inner) => assert_eq!(inner.children.len(), 3),
                other => panic!("expected inner Sequence, got {other:?}"),
            },
            other => panic!("expected outer Sequence, got {other:?}"),
        }
    }

    #[test]
    fn box_is_transparent() {
        let node = node_for(quote! { pub struct S { b: Box<Foo> } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => assert_eq!(nt.text, "Foo"),
                other => panic!("expected unwrapped Foo, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn direct_keyword_path_renders_as_uppercased_terminal() {
        // Common shape inside enum variants like `JoinType::Left(keyword::Left)`.
        // No PhantomData wrapper — recognize must detect the `keyword::` prefix
        // on the type path itself.
        let node = node_for(quote! {
            pub enum JoinType {
                Left(keyword::Left),
                Right(keyword::Right),
            }
        });
        match node {
            Node::Choice(ch) => {
                assert_eq!(ch.children.len(), 2);
                match &ch.children[0] {
                    Node::Terminal(t) => assert_eq!(t.text, "LEFT"),
                    other => panic!("expected Terminal, got {other:?}"),
                }
                match &ch.children[1] {
                    Node::Terminal(t) => assert_eq!(t.text, "RIGHT"),
                    other => panic!("expected Terminal, got {other:?}"),
                }
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn keyword_path_in_struct_field_also_works() {
        let node = node_for(quote! {
            pub struct S { kw: keyword::Select }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::Terminal(t) => assert_eq!(t.text, "SELECT"),
                other => panic!("expected Terminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn non_keyword_path_is_not_uppercased() {
        // A type like `ast::expr::BinaryOp` should not be miscategorized as
        // a keyword just because it has a multi-segment path.
        let node = node_for(quote! {
            pub struct S { op: ast::expr::BinaryOp }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => assert_eq!(nt.text, "BinaryOp"),
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn phantom_data_renders_as_uppercased_terminal() {
        // PhantomData<T> in pg-sql means "T is a keyword marker". Render it
        // as a literal terminal with the ident uppercased so `DROP`/`TABLE`
        // appear as terminals instead of opaque PhantomData boxes.
        let node = node_for(quote! {
            pub struct S { _drop: PhantomData<keyword::Drop> }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::Terminal(t) => assert_eq!(t.text, "DROP"),
                other => panic!("expected Terminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn enum_renders_as_choice() {
        let node = node_for(quote! {
            pub enum Stmt {
                Select(SelectStmt),
                Insert(InsertStmt),
                Update(UpdateStmt),
            }
        });
        match node {
            Node::Choice(ch) => {
                assert_eq!(ch.children.len(), 3);
                assert_eq!(ch.default_idx, 0);
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn enum_variant_unwraps_inner_type() {
        let node = node_for(quote! {
            pub enum E {
                Wrapped(Box<Foo>),
            }
        });
        match node {
            Node::Choice(ch) => match &ch.children[0] {
                Node::NonTerminal(nt) => assert_eq!(nt.text, "Foo"),
                other => panic!("expected NonTerminal Foo, got {other:?}"),
            },
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn unit_variant_falls_back_to_variant_name() {
        let node = node_for(quote! {
            pub enum E { Bare }
        });
        match node {
            Node::Choice(ch) => match &ch.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "Bare");
                    assert!(nt.href.is_none());
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Choice, got {other:?}"),
        }
    }

    #[test]
    fn empty_enum_is_rejected() {
        let input: DeriveInput = parse2(quote! { pub enum Empty {} }).unwrap();
        let err = build_node(&input, &TypeAttrs::default()).expect_err("expected empty-enum error");
        assert!(
            err.to_string().contains("empty enum"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn field_skip_removes_from_sequence() {
        let node = node_for(quote! {
            pub struct S {
                a: Ident,
                #[railroad(skip)]
                b: Ident,
                c: Ident,
            }
        });
        match node {
            Node::Sequence(seq) => assert_eq!(seq.children.len(), 2),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn fallthrough_type_has_no_href() {
        // Hrefs were dropped because proc macros lack the calling-module
        // context to resolve them correctly. Every NonTerminal produced by
        // the fallthrough path should have href == None.
        let node = node_for(quote! { pub struct S { x: crate::ast::Thing } });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "Thing");
                    assert!(nt.href.is_none());
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn field_label_overrides_type_name() {
        let node = node_for(quote! {
            pub struct S {
                #[railroad(label = "SELECT")]
                kw: SelectKw,
            }
        });
        match node {
            Node::Sequence(seq) => match &seq.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "SELECT");
                    // Label passes through unchanged; href is always None now
                    // (hrefs were dropped as part of the broken-link fix).
                    assert!(nt.href.is_none());
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn split_attribute_label_skip_conflict_is_rejected() {
        // The conflict check fires after both attributes have been folded
        // into the same FieldAttrs. Verify it works when label and skip
        // arrive in separate `#[railroad(...)]` attrs, not just the same one.
        let item: TokenStream = quote! {
            pub struct S {
                #[railroad(skip)]
                #[railroad(label = "X")]
                f: Foo,
            }
        };
        let err = expand(quote! {}, item).expect_err("expected conflict error");
        assert!(
            err.to_string().contains("mutually exclusive"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn wide_struct_top_level_sequence_is_wrapped() {
        // Many narrow fields — enough width to blow past DEFAULT_MAX_WIDTH
        // (1200). Each NonTerminal rendered from `TypeN` has width
        // (name_len*8 + 40). Twenty fields easily exceed 1200 px.
        let node = node_for(quote! {
            pub struct Wide {
                a: TypeAAAAAAAAAA,
                b: TypeBBBBBBBBBB,
                c: TypeCCCCCCCCCC,
                d: TypeDDDDDDDDDD,
                e: TypeEEEEEEEEEE,
                f: TypeFFFFFFFFFF,
                g: TypeGGGGGGGGGG,
                h: TypeHHHHHHHHHH,
                i: TypeIIIIIIIIII,
                j: TypeJJJJJJJJJJ,
                k: TypeKKKKKKKKKK,
                l: TypeLLLLLLLLLL,
                m: TypeMMMMMMMMMM,
                n: TypeNNNNNNNNNN,
                o: TypeOOOOOOOOOO,
                p: TypePPPPPPPPPP,
            }
        });
        match node {
            Node::Sequence(seq) => assert!(
                !seq.rows.is_empty(),
                "expected wide struct to wrap; rows was empty. width={}",
                seq.width
            ),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn narrow_struct_top_level_sequence_is_not_wrapped() {
        // A narrow struct stays single-row.
        let node = node_for(quote! {
            pub struct Narrow { a: Foo, b: Bar }
        });
        match node {
            Node::Sequence(seq) => assert!(
                seq.rows.is_empty(),
                "narrow struct should not wrap; rows={:?}",
                seq.rows
            ),
            other => panic!("expected Sequence, got {other:?}"),
        }
    }

    #[test]
    fn multi_field_tuple_variant_falls_back_to_variant_name() {
        // Non-conforming per CLAUDE.md (variants must be single-field tuples)
        // but the macro defends against it with a NonTerminal fallback.
        let node = node_for(quote! {
            pub enum E {
                Pair(Foo, Bar),
            }
        });
        match node {
            Node::Choice(ch) => match &ch.children[0] {
                Node::NonTerminal(nt) => {
                    assert_eq!(nt.text, "Pair");
                    assert!(nt.href.is_none());
                }
                other => panic!("expected NonTerminal, got {other:?}"),
            },
            other => panic!("expected Choice, got {other:?}"),
        }
    }
}
