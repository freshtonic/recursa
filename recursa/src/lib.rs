//! Recursa — an AST-first parser generator. The AST is the grammar.
//!
//! This facade exports the common runtime contracts and authoring macros.
//! Grammar-bound lexical and input types are generated in each grammar module.

#[cfg(feature = "arbitrary")]
pub use recursa_core::__private::arbitrary::Arbitrary;
#[cfg(feature = "explorer")]
pub use recursa_core::explorer;
#[cfg(feature = "explorer")]
pub use recursa_core::framing;
pub use recursa_core::{
    ElementMetadata, ElementRole, ElementView, Elements, HasSpan, Node, NodeView, OffsetOverflow,
    Parse, ParseError, ParseErrorKind, Parsed, Pretty, PrettyConfig, PrettyContext, PrettySink,
    Repeated, RepeatedIter, SourceMap, SourceMapEntry, Span, TotalVisitor, TotalVisitorMut, Vec1,
    Visit, VisitBreak, VisitMut, Visitor, VisitorMut,
};
pub use recursa_macros::{Node, TotalVisitor, TotalVisitorMut, grammar, tokens};

/// Doc-time railroad diagram attachment reserved for Recursa-generated code.
///
/// Generated items carry this attribute under `cfg(doc)`; it is a matching-version
/// ABI, not a public documentation API.
#[cfg(feature = "diagrams")]
#[doc(hidden)]
pub use recursa_macros::__recursa_diagram_doc;

/// Runtime operations reserved for Recursa-generated code.
///
/// This is a matching-version ABI and is not a general-purpose parsing API.
#[doc(hidden)]
pub mod __private {
    pub use recursa_core::__private::*;
}

#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn facade_reexports_the_exact_release_and_private_protocol_capability() {
        let _: super::__private::FacadeCompatibility<
            { super::__private::release_stamp("0.1.0") },
            19,
        > = super::__private::FACADE_COMPATIBILITY;
    }
}
