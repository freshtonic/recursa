/// Common AST building blocks shared across multiple statement kinds.
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{keyword, literal, punct};
use recursa_diagram::railroad;

/// `CASCADE` drop behavior.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct CascadeKw(pub PhantomData<keyword::Cascade>);

/// `RESTRICT` drop behavior.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct RestrictKw(pub PhantomData<keyword::Restrict>);

/// `CASCADE | RESTRICT` drop behavior.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum DropBehavior {
    Cascade(CascadeKw),
    Restrict(RestrictKw),
}

/// A dotted name: `name`, `schema.name`, or `catalog.schema.name`.
///
/// This is the usual shape for table/view/sequence/type references in SQL.
/// Must NOT collide with `Expr::QualRef` at the Pratt level because
/// `QualifiedName` is only used in non-expression positions (FROM targets,
/// DROP targets, ALTER targets, etc.).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct QualifiedName<'input> {
    pub parts: Seq<literal::Ident<'input>, punct::Dot>,
}

impl<'input> QualifiedName<'input> {
    /// Returns the final (object) name part.
    pub fn object(&self) -> &str {
        self.parts
            .iter()
            .last()
            .map(|i| i.text())
            .unwrap_or_default()
    }
}
