//! The sole LR parser rendered by one generation.
//!
//! One generation owns one lexer, one `TokenKind`, one AST, one grammar-local
//! `Input`, and one grammar-wide LALR table set. Parser selection and named
//! parser modules deliberately do not exist.

use super::*;

/// The parser rendered by one generation.
pub(super) struct RenderedParser {
    /// The grammar-wide automaton and packed tables.
    tables: TableDrivenRendering,
}

impl RenderedParser {
    /// Returns the grammar-wide automaton and packed tables.
    pub(super) const fn tables(&self) -> Option<&TableDrivenRendering> {
        Some(&self.tables)
    }

    /// Returns the root module holding the sole parser's tables.
    pub(super) fn tables_module(&self, plan: &EmissionPlan) -> proc_macro2::TokenStream {
        let module_anchor = &plan.module_anchor;
        quote!(#module_anchor::__recursa_lr)
    }
}

/// The one parser emitted by a generation.
pub(super) struct ParserSet {
    /// The sole parser.
    parser: RenderedParser,
}

impl ParserSet {
    /// Builds the sole LR parser and fails on any unresolved conflict.
    pub(super) fn declared(
        _plan: &EmissionPlan,
        lowered: &crate::materialization::LoweredGrammar,
    ) -> Result<Self, LrBuildError> {
        Ok(Self {
            parser: RenderedParser {
                tables: TableDrivenRendering::build(lowered)?,
            },
        })
    }

    /// Returns the parser that owns the unqualified generated surface.
    pub(super) const fn default_parser(&self) -> &RenderedParser {
        &self.parser
    }

    /// Iterates the sole parser.
    pub(super) fn iter(&self) -> impl Iterator<Item = &RenderedParser> {
        std::iter::once(&self.parser)
    }

    /// Returns the sole table-driven parser.
    pub(super) const fn table_driven(&self) -> Option<&TableDrivenRendering> {
        Some(&self.parser.tables)
    }
}

/// Named parser modules were removed with parser selection.
pub(super) fn render_parser_modules(
    _plan: &EmissionPlan,
    _parsers: &ParserSet,
) -> Result<proc_macro2::TokenStream, String> {
    Ok(quote!())
}
