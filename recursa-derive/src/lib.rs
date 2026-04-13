//! Derive macros for the recursa parser framework.

mod format_tokens_derive;
mod parse_derive;
mod scan_derive;
mod total_visitor_derive;
mod visit_derive;

use proc_macro::TokenStream;

/// Derive `Scan` and `Parse` for token types.
///
/// # Unit structs (keywords)
///
/// ```ignore
/// #[derive(Scan)]
/// #[scan(pattern = "let")]
/// struct LetKw;
/// ```
///
/// # Tuple structs (capturing tokens)
///
/// With a lifetime parameter, captures `&'input str`:
/// ```ignore
/// #[derive(Scan)]
/// #[scan(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*")]
/// struct Ident<'input>(&'input str);
/// ```
///
/// Without a lifetime parameter, captures `String`:
/// ```ignore
/// #[derive(Scan)]
/// #[scan(pattern = r"[0-9]+")]
/// struct IntLit(String);
/// ```
///
/// # Enums (combined scanner)
///
/// All variants must be single-field tuple variants wrapping a `Scan` type:
/// ```ignore
/// #[derive(Scan)]
/// enum Keyword {
///     Let(LetKw),
///     If(IfKw),
/// }
/// ```
///
/// # Attributes
///
/// - `#[scan(pattern = "...")]` — regex pattern for the token
/// - `#[scan(case_insensitive)]` — wraps pattern in `(?i:...)`
#[proc_macro_derive(Scan, attributes(scan))]
pub fn derive_scan(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match scan_derive::derive_scan(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive `Parse` for AST types.
///
/// # Structs (sequence parsing)
///
/// Parses each field in declaration order, calling `consume_ignored` between fields:
/// ```ignore
/// #[derive(Parse)]
/// #[parse(rules = MyRules)]
/// struct LetBinding {
///     let_kw: LetKw,
///     name: Ident,
///     eq: Eq,
///     value: Expr,
///     semi: Semi,
/// }
/// ```
///
/// # Enums (choice parsing)
///
/// Each variant must be a single-field tuple variant wrapping a type that implements
/// `Parse`. The derive uses multi-token lookahead to dispatch to the correct variant:
/// ```ignore
/// #[derive(Parse)]
/// #[parse(rules = MyRules)]
/// enum Statement {
///     Select(SelectStmt),
///     Insert(InsertStmt),
/// }
/// ```
///
/// Struct-like variants (with named fields) are **not supported**. Wrap multiple
/// fields in a separate struct that derives `Parse`.
///
/// # Pratt parsing (expressions)
///
/// Use `#[parse(pratt)]` for expression enums with operator precedence:
/// ```ignore
/// #[derive(Parse)]
/// #[parse(rules = MyRules, pratt)]
/// enum Expr {
///     #[parse(atom)]
///     Lit(IntLit),
///
///     #[parse(prefix, bp = 15)]
///     Neg(Minus, Box<Expr>),
///
///     #[parse(infix, bp = 5)]
///     Add(Box<Expr>, Plus, Box<Expr>),
///
///     #[parse(postfix, bp = 20)]
///     Cast(Box<Expr>, ColonColon, TypeName),
/// }
/// ```
///
/// Variant kinds:
/// - `atom` — single-field, no recursion
/// - `prefix` — `(operator, Box<Self>)`
/// - `infix` — `(Box<Self>, operator, Box<Self>)`, optional `assoc = "right"`
/// - `postfix` — `(Box<Self>, operator, ...remaining_fields)`
///
/// `bp` (binding power) controls precedence — higher binds tighter.
#[proc_macro_derive(Parse, attributes(parse))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match parse_derive::derive_parse(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive `Visit` for AST traversal via the visitor pattern.
///
/// Generates an `accept` method that calls `visitor.enter(self)`, visits all
/// children, then calls `visitor.exit(self)`. Also derives `AsNodeKey`.
///
/// # Structs
///
/// Visits each field in declaration order:
/// ```ignore
/// #[derive(Visit)]
/// struct LetBinding {
///     let_kw: LetKw,
///     name: Ident,
///     value: Expr,
/// }
/// ```
///
/// # Enums
///
/// Delegates to whichever variant is present:
/// ```ignore
/// #[derive(Visit)]
/// enum Statement {
///     Select(SelectStmt),
///     Insert(InsertStmt),
/// }
/// ```
#[proc_macro_derive(Visit, attributes(visit))]
pub fn derive_visit(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match visit_derive::derive_visit(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive `TotalVisitor` for type-safe visitor dispatch.
///
/// Generates a `TotalVisitor` impl that dispatches `total_enter`/`total_exit`
/// to your `Visitor<N>` impls based on `TypeId`.
///
/// # Example
///
/// ```text
/// #[derive(TotalVisitor)]
/// #[total_visitor(dispatch = [SelectStmt, Expr], error = MyError)]
/// struct MyVisitor { ... }
///
/// impl Visitor<SelectStmt> for MyVisitor {
///     type Error = MyError;
///     fn enter(&mut self, node: &SelectStmt) -> ControlFlow<Break<MyError>> {
///         // type-safe, no downcast needed
///     }
/// }
/// ```
#[proc_macro_derive(TotalVisitor, attributes(total_visitor))]
pub fn derive_total_visitor(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match total_visitor_derive::derive_total_visitor(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive `FormatTokens` for pretty-printer token emission.
///
/// Emits `Token::String` for each field by default. Use `#[format_tokens(...)]`
/// attributes to add structural tokens (groups, breaks, indentation).
///
/// # Attributes
///
/// ## Struct-level
/// - `#[format_tokens(group(consistent))]` — wrap all tokens in `Begin`/`End`
/// - `#[format_tokens(group(inconsistent))]` — same, with inconsistent breaking
///
/// ## Field-level
/// - `#[format_tokens(break(flat = " ", broken = "\n"))]` — emit `Break` before field
/// - `#[format_tokens(indent)]` — wrap field in `Indent`/`Dedent`
/// - `#[format_tokens(group(consistent))]` — wrap field in `Begin`/`End`
#[proc_macro_derive(FormatTokens, attributes(format_tokens))]
pub fn derive_format_tokens(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match format_tokens_derive::derive_format_tokens(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
