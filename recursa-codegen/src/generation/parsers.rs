//! The parsers one generation renders: the marker, tables and module each declared parser is
//! spelled through, and the `parsers::<name>` modules of `root.rs`.
//!
//! One generation emits one lexer, one `TokenKind`, one AST and, per declared parser, one
//! `GeneratedParse` impl per node and content token. The marker that distinguishes those impls
//! is the style's own: the recursive-descent bodies recurse through facade helpers bound to
//! the trait-default `RecursiveDescent` marker, so a recursive-descent parser always carries
//! it, and the table-driven loop is marker-agnostic, so `TableDriven` names the other. Two
//! parsers of one style would collide on that marker and on the per-node reduce helpers, which
//! is why the configuration refuses a repeated style (RCA1017) and why no per-parser marker
//! type is generated.
//!
//! The default parser owns the unqualified surface unchanged: the root `Input`,
//! `lexed.input()`, the inherent `parse` and `parse_spanned`, document framing and the explorer
//! adapter. Every extra parser gets `parsers::<name>` in `root.rs` with its own `Input` newtype
//! naming the parser's marker, `input` and `input_with_builder` constructors over the shared
//! `LexResult` and, when table-driven, its `__recursa_lr` tables and dispatcher. The sealed
//! `Parse<I>` facade selects the parser by input type, and so do the inherent `parse` and
//! `parse_spanned` of every node, which accept every input type of the generation once an
//! extra parser is declared: `Root::parse(&mut parsers::lr::input(&lexed))` runs the `lr`
//! parser and nothing else changes.

use super::*;

/// Spells one generated parser marker without exposing parser-set construction to capability
/// renderers that only need the selected default.
fn marker_for_style(plan: &EmissionPlan, style: ParserStyle) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    match style {
        ParserStyle::RecursiveDescent => quote!(#facade::__private::RecursiveDescent),
        ParserStyle::TableDriven => quote!(#facade::__private::TableDriven),
    }
}

/// Spells the marker selected by the generation's unqualified `Input`.
pub(super) fn default_parser_marker(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    marker_for_style(plan, plan.parsers.default_parser().style)
}

/// One declared parser as the renderer spells it.
pub(super) struct RenderedParser {
    /// Author-chosen name; `None` for the single `parser_style` parser.
    name: Option<Ident>,
    /// Recognition strategy, which decides the marker and the bodies.
    style: ParserStyle,
    /// Whether this parser owns the unqualified surface.
    is_default: bool,
    /// The automaton, present for the table-driven style.
    tables: Option<TableDrivenRendering>,
}

impl RenderedParser {
    /// The automaton when this parser is table-driven.
    pub(super) fn tables(&self) -> Option<&TableDrivenRendering> {
        self.tables.as_ref()
    }

    /// The module under `parsers` this parser is rendered in; `None` for the default parser,
    /// which owns the unqualified surface.
    pub(super) fn module_name(&self) -> Option<&Ident> {
        if self.is_default {
            None
        } else {
            self.name.as_ref()
        }
    }

    /// The marker type this parser's `GeneratedParse` impls and its `Input` carry.
    pub(super) fn marker(&self, plan: &EmissionPlan) -> proc_macro2::TokenStream {
        marker_for_style(plan, self.style)
    }

    /// The explicit marker argument of a `Parse<'input, A, P>` bound: empty for recursive
    /// descent, the trait's default, so that output keeps its defaulted spelling.
    pub(super) fn marker_argument(&self, plan: &EmissionPlan) -> proc_macro2::TokenStream {
        let facade = &plan.facade;
        match self.style {
            ParserStyle::RecursiveDescent => quote!(),
            ParserStyle::TableDriven => quote!(, #facade::__private::TableDriven),
        }
    }

    /// The path of the `__recursa_lr` module holding this parser's tables and dispatcher: at
    /// the grammar module for the default parser, under `parsers::<name>` otherwise.
    pub(super) fn tables_module(&self, plan: &EmissionPlan) -> proc_macro2::TokenStream {
        let module_anchor = &plan.module_anchor;
        match self.module_name() {
            Some(name) => quote!(#module_anchor::parsers::#name::__recursa_lr),
            None => quote!(#module_anchor::__recursa_lr),
        }
    }
}

/// The parsers of one generation in declared order, the default first.
pub(super) struct ParserSet {
    /// The declared parsers; never empty.
    parsers: Vec<RenderedParser>,
}

impl ParserSet {
    /// The single implicit or explicit recursive-descent parser, which needs no analysis.
    pub(super) fn recursive_descent() -> Self {
        Self {
            parsers: vec![RenderedParser {
                name: None,
                style: ParserStyle::RecursiveDescent,
                is_default: true,
                tables: None,
            }],
        }
    }

    /// Builds the plan's declared parsers, running the LALR(1) analysis for a table-driven one
    /// and failing on any conflict.
    ///
    /// Document framing and the explorer adapter drive the same marker selected by the default
    /// grammar-local input, so either parser style may own the unqualified surface.
    pub(super) fn declared(
        plan: &EmissionPlan,
        lowered: &crate::materialization::LoweredGrammar,
    ) -> Result<Self, String> {
        let declarations = &plan.parsers;
        let mut parsers = Vec::new();
        for (index, declaration) in declarations.iter().enumerate() {
            let tables = match declaration.style {
                ParserStyle::RecursiveDescent => None,
                ParserStyle::TableDriven => Some(TableDrivenRendering::build(lowered)?),
            };
            parsers.push(RenderedParser {
                name: declaration.name.clone(),
                style: declaration.style,
                is_default: index == 0,
                tables,
            });
        }
        Ok(Self { parsers })
    }

    /// The parser that owns the unqualified surface.
    pub(super) fn default_parser(&self) -> &RenderedParser {
        &self.parsers[0]
    }

    /// Iterates every parser in declared order, the default first.
    pub(super) fn iter(&self) -> impl Iterator<Item = &RenderedParser> {
        self.parsers.iter()
    }

    /// Iterates the parsers rendered under `parsers::<name>`.
    pub(super) fn extras(&self) -> impl Iterator<Item = &RenderedParser> {
        self.parsers.iter().filter(|parser| !parser.is_default)
    }

    /// Whether a recursive-descent parser renders, which consumes the frozen dispatch trie.
    pub(super) fn renders_recursive_descent(&self) -> bool {
        self.parsers
            .iter()
            .any(|parser| parser.style == ParserStyle::RecursiveDescent)
    }

    /// The table-driven parser, whose automaton dump the debug bundle carries.
    pub(super) fn table_driven(&self) -> Option<&TableDrivenRendering> {
        self.parsers.iter().find_map(RenderedParser::tables)
    }
}

/// Renders the `parsers` module of `root.rs`, one module per extra parser; nothing when the
/// generation has only its default parser.
pub(super) fn render_parser_modules(
    plan: &EmissionPlan,
    parsers: &ParserSet,
) -> Result<proc_macro2::TokenStream, String> {
    let modules = parsers
        .extras()
        .map(|parser| render_parser_module(plan, parser))
        .collect::<Result<Vec<_>, String>>()?;
    if modules.is_empty() {
        return Ok(quote!());
    }
    Ok(quote! {
        /// The parsers declared beside the default parser, one module per name.
        pub mod parsers {
            #(#modules)*
        }
    })
}

/// Renders one extra parser's module: its `Input`, the input constructors and, when
/// table-driven, its tables and dispatcher.
fn render_parser_module(
    plan: &EmissionPlan,
    parser: &RenderedParser,
) -> Result<proc_macro2::TokenStream, String> {
    let module_anchor = &plan.module_anchor;
    let name = parser
        .module_name()
        .expect("every extra parser is declared with a name");
    let input_newtype = render_input_newtype(
        plan,
        &quote! {
            /// A cursor bound to this generated grammar's token identities that selects this parser.
        },
        &parser.marker(plan),
    );
    let lr_items = parser
        .tables()
        .map(|lr| render_root_items(plan, lr))
        .transpose()?;
    let module_doc = format!(
        "The `{name}` parser ({}): its input type selects it, `Root::parse(&mut {name}::input(&lexed))`.",
        parser.style.as_str()
    );
    Ok(quote! {
        #[doc = #module_doc]
        pub mod #name {
            #input_newtype

            /// Builds this parser's input over a lexing result of the grammar.
            pub fn input<'input>(lexed: &#module_anchor::LexResult<'input>) -> Input<'input> {
                Input { inner: lexed.input().inner }
            }

            /// Builds this parser's input over a lexing result with a builder.
            pub fn input_with_builder<'input, A>(
                lexed: &#module_anchor::LexResult<'input>,
                builder: A,
            ) -> Input<'input, A> {
                Input { inner: lexed.input_with_builder(builder).inner }
            }

            #lr_items
        }
    })
}
