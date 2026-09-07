#![allow(
    dead_code,
    reason = "private source analysis source-configuration seam is consumed by later pipeline tickets"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Parsing of source-level grammar configuration and Rust syntax fragments.
//!
//! These helpers normalize only generator-owned syntax. Rust paths and token
//! fragments outside the grammar-semantic reference set remain structured but
//! unresolved, preserving the boundary between source analysis and rustc.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::ops::Range;

use proc_macro2::Span;
use syn::ext::IdentExt as _;
use syn::parse::{Parse, ParseStream, Parser as _};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{Attribute, Expr, Item, Lit, LitInt, Meta, Path, Token};

use crate::TextEquivalence;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Canonical active Cargo configuration used to filter authored syntax.
pub(crate) struct CargoCfgSnapshot {
    /// Enabled single-name predicates such as target-family flags.
    flags: BTreeSet<String>,
    /// Enabled values grouped by Cargo configuration key.
    values: BTreeMap<String, BTreeSet<String>>,
}

impl CargoCfgSnapshot {
    /// Adds an enabled flag to a snapshot under construction.
    pub(crate) fn with_flag(mut self, name: impl Into<String>) -> Self {
        self.flags.insert(name.into());
        self
    }

    /// Adds one enabled value under its Cargo configuration key.
    pub(crate) fn with_value(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.values
            .entry(name.into())
            .or_default()
            .insert(value.into());
        self
    }

    /// Captures Cargo's active `cfg` flags and values for source-tree filtering.
    pub(crate) fn from_cargo_environment<I, K, V>(variables: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        let mut snapshot = Self::default();
        for (name, value) in variables {
            let Ok(name) = name.into().into_string() else {
                continue;
            };
            if let Some(cfg_name) = name.strip_prefix("CARGO_CFG_") {
                let Ok(value) = value.into().into_string() else {
                    continue;
                };
                let cfg_name = cfg_name.to_ascii_lowercase();
                if value.is_empty() && !empty_value_cfg(&cfg_name) {
                    snapshot.flags.insert(cfg_name);
                } else {
                    for value in value
                        .split(',')
                        .filter(|value| !value.is_empty() || empty_value_cfg(&cfg_name))
                    {
                        snapshot
                            .values
                            .entry(cfg_name.clone())
                            .or_default()
                            .insert(value.to_owned());
                    }
                }
            }
        }
        snapshot
    }

    /// Renders facts in canonical order.
    pub(crate) fn canonical_facts(&self) -> Vec<String> {
        let mut facts = self.flags.iter().cloned().collect::<Vec<_>>();
        for (name, values) in &self.values {
            facts.extend(values.iter().map(|value| format!("{name}=\"{value}\"")));
        }
        facts.sort();
        facts
    }

    /// Evaluates a flag while rejecting invocation-only configuration names.
    fn flag(&self, name: &str) -> Result<bool, CfgLookupFailure> {
        if invocation_only_cfg(name) {
            return Err(CfgLookupFailure::InvocationOnly);
        }
        if self.flags.contains(name) {
            Ok(true)
        } else if self.values.contains_key(name) || compiler_owned_cfg(name) {
            Ok(false)
        } else {
            Err(CfgLookupFailure::Unknown)
        }
    }

    /// Evaluates a key-value predicate with Cargo's empty-value exceptions.
    fn value(&self, name: &str, value: &str) -> Result<bool, CfgLookupFailure> {
        if invocation_only_cfg(name) {
            return Err(CfgLookupFailure::InvocationOnly);
        }
        if let Some(values) = self.values.get(name) {
            Ok(values.contains(value))
        } else if self.flags.contains(name) || compiler_owned_cfg(name) {
            Ok(false)
        } else {
            Err(CfgLookupFailure::Unknown)
        }
    }
}

/// Reports configuration names whose truth is owned by this macro invocation.
fn invocation_only_cfg(name: &str) -> bool {
    matches!(name, "test" | "doc" | "doctest")
}

/// Reports Cargo keys for which an empty value is still an observed value.
fn empty_value_cfg(name: &str) -> bool {
    matches!(name, "target_abi" | "target_env")
}

/// Reports configuration names defined by rustc rather than build-script features.
fn compiler_owned_cfg(name: &str) -> bool {
    matches!(
        name,
        "contract_checks"
            | "debug_assertions"
            | "feature"
            | "miri"
            | "overflow_checks"
            | "panic"
            | "proc_macro"
            | "relocation_model"
            | "sanitize"
            | "target_abi"
            | "target_arch"
            | "target_endian"
            | "target_env"
            | "target_family"
            | "target_feature"
            | "target_has_atomic"
            | "target_has_atomic_equal_alignment"
            | "target_has_atomic_load_store"
            | "target_os"
            | "target_pointer_width"
            | "target_thread_local"
            | "target_vendor"
            | "ub_checks"
            | "unix"
            | "windows"
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
/// Generation-time recognition strategy of one parser over the grammar.
///
/// The style changes only the bodies behind the generated-parse protocol;
/// grammar types, lexing, framing and derived traits are identical across
/// styles.
pub(crate) enum ParserStyle {
    /// One generated Rust function per Node, the default and today's only renderer.
    RecursiveDescent,
    /// One grammar-wide LALR(1) automaton with a start state per grammar type.
    TableDriven,
}

impl ParserStyle {
    /// Resolves an authored style identifier, rejecting anything outside the closed set.
    fn parse_ident(ident: &syn::Ident) -> syn::Result<Self> {
        if ident == "recursive_descent" {
            Ok(Self::RecursiveDescent)
        } else if ident == "table_driven" {
            Ok(Self::TableDriven)
        } else {
            Err(syn::Error::new(
                ident.span(),
                format!(
                    "unsupported parser style `{ident}`; expected `recursive_descent` or `table_driven`"
                ),
            ))
        }
    }

    /// Returns the authored spelling, used in generation identity and diagnostics.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RecursiveDescent => "recursive_descent",
            Self::TableDriven => "table_driven",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
/// One parser a generation emits over the grammar.
pub(crate) struct ParserDeclaration {
    /// Author-chosen name under `parsers(...)`; `None` for the single `parser_style` parser.
    pub(crate) name: Option<syn::Ident>,
    /// Recognition strategy of this parser.
    pub(crate) style: ParserStyle,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
/// The parsers one generation emits, in authored order.
///
/// The first declaration is the default parser and owns the unqualified
/// generated surface. The value is never empty: an omitted configuration is
/// the implicit default, one unnamed recursive-descent parser.
pub(crate) struct ParserDeclarations(Vec<ParserDeclaration>);

impl Default for ParserDeclarations {
    fn default() -> Self {
        Self::single(ParserStyle::RecursiveDescent)
    }
}

impl ParserDeclarations {
    /// Declares the one unnamed parser selected by `parser_style`.
    fn single(style: ParserStyle) -> Self {
        Self(vec![ParserDeclaration { name: None, style }])
    }

    /// Declares the named parsers of `parsers(...)`, rejecting an empty list, repeated names
    /// and repeated styles.
    ///
    /// A style is declared at most once because the generated parsers of one style share
    /// their marker type and their per-node helpers: the recursive-descent bodies recurse
    /// through facade helpers bound to the `RecursiveDescent` marker, and the table-driven
    /// reduce actions are one inherent method per node. Two parsers of one style would be
    /// the same parser twice.
    fn named(parsers: Vec<ParserDeclaration>, span: Span) -> syn::Result<Self> {
        if parsers.is_empty() {
            return Err(syn::Error::new(
                span,
                "parsers requires at least one named parser",
            ));
        }
        for (index, parser) in parsers.iter().enumerate() {
            let name = parser.name.as_ref().expect("named parsers carry names");
            if parsers[..index]
                .iter()
                .any(|earlier| earlier.name.as_ref() == Some(name))
            {
                return Err(syn::Error::new(
                    name.span(),
                    format!("duplicate parser name `{name}`"),
                ));
            }
            if let Some(earlier) = parsers[..index]
                .iter()
                .find(|earlier| earlier.style == parser.style)
            {
                let earlier = earlier.name.as_ref().expect("named parsers carry names");
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "parser `{name}` repeats the `{}` style of `{earlier}`; one generation emits one parser per style",
                        parser.style.as_str()
                    ),
                ));
            }
        }
        Ok(Self(parsers))
    }

    /// Returns the parser that owns the unqualified generated surface.
    pub(crate) fn default_parser(&self) -> &ParserDeclaration {
        &self.0[0]
    }

    /// Iterates the declared parsers in authored order, default first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ParserDeclaration> {
        self.0.iter()
    }

    /// Reports whether this generation emits a recursive-descent parser.
    ///
    /// The source-independent grammar analysis is shared by all parser styles. A
    /// shared-prefix diagnostic is an error for recursive descent, but it is not
    /// by itself a reason to reject a table-driven-only generation. Keeping this
    /// fact on the parser declaration set gives that policy one source of truth
    /// for implicit, single-style, and named-parser configurations.
    pub(crate) fn renders_recursive_descent(&self) -> bool {
        self.0
            .iter()
            .any(|parser| parser.style == ParserStyle::RecursiveDescent)
    }

    /// Reports whether this is the implicit default of an omitted configuration.
    ///
    /// An explicit `parser_style = recursive_descent` states the same fact, so
    /// generation identity treats the two alike.
    pub(crate) fn is_implicit_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Parsed root `grammar!` configuration before path and capability resolution.
pub(crate) struct GrammarConfigSyntax {
    /// Selected-root logical module anchor.
    pub(crate) module: Path,
    /// Crate identifier used for Recursa semantic paths and generated protocol calls.
    pub(crate) facade: syn::Ident,
    /// Whether the author explicitly supplied the facade mapping.
    pub(crate) facade_explicit: bool,
    /// Grammar-wide keyword matching relation.
    pub(crate) text_equivalence: TextEquivalence,
    /// Unsupported authored keyword policy retained for a structured diagnostic.
    pub(crate) invalid_keyword_matching: Option<InvalidKeywordMatchingSyntax>,
    /// Maximum predictive dispatch depth selected by the grammar.
    pub(crate) max_lookahead: usize,
    /// Default occurrence-span capture policy.
    pub(crate) spans: CaptureMode,
    /// Grammar-wide configured-trait selections in authored order.
    pub(crate) derives: Vec<Path>,
    /// Whether generation should publish railroad diagrams.
    pub(crate) diagrams: bool,
    /// Grammar-wide Pretty layout defaults.
    pub(crate) pretty: PrettyDefaults,
    /// Optional generated exact-source document framing declaration.
    pub(crate) framing: Option<FramingSyntax>,
    /// Parsers this generation emits; the first is the default parser.
    pub(crate) parsers: ParserDeclarations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Compile-time document framing declaration before semantic identity resolution.
pub(crate) struct FramingSyntax {
    /// Generated Node application parsed as each strict typed island.
    pub(crate) island: Path,
    /// Fixed token separating adjacent island roots.
    pub(crate) boundary: syn::Ident,
    /// Closed physical-line token kinds owned as atomic raw regions.
    pub(crate) line: Vec<syn::Ident>,
    /// Closed same- or nested-delimiter token kinds owned as atomic raw regions.
    pub(crate) delimited: Vec<syn::Ident>,
    /// Optional fully committed island variant that opens a payload region.
    pub(crate) payload: Option<FramingPayloadSyntax>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Compile-time payload transition attached to one complete island variant.
pub(crate) struct FramingPayloadSyntax {
    /// Root-and-variant path whose complete strict occurrence enters payload mode.
    pub(crate) after: Path,
    /// Exact text required as one standalone physical line to terminate payload data.
    pub(crate) terminator: syn::LitStr,
}

impl Parse for FramingSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut island = None;
        let mut boundary = None;
        let mut line = None;
        let mut delimited = None;
        let mut payload = None;
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            if key == "payload" {
                reject_duplicate(&payload, key.span(), "framing payload")?;
                let content;
                syn::parenthesized!(content in input);
                payload = Some(content.parse()?);
            } else {
                input.parse::<Token![=]>()?;
                if key == "island" {
                    reject_duplicate(&island, key.span(), "framing island")?;
                    island = Some(Path::parse_mod_style(input)?);
                } else if key == "boundary" {
                    reject_duplicate(&boundary, key.span(), "framing boundary")?;
                    boundary = Some(input.call(syn::Ident::parse_any)?);
                } else if key == "line" {
                    reject_duplicate(&line, key.span(), "framing line")?;
                    line = Some(parse_ident_list(input)?);
                } else if key == "delimited" {
                    reject_duplicate(&delimited, key.span(), "framing delimited")?;
                    delimited = Some(parse_ident_list(input)?);
                } else {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unsupported framing setting `{key}`"),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            island: island.ok_or_else(|| input.error("framing requires island"))?,
            boundary: boundary.ok_or_else(|| input.error("framing requires boundary"))?,
            line: line.unwrap_or_default(),
            delimited: delimited.unwrap_or_default(),
            payload,
        })
    }
}

impl Parse for FramingPayloadSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut after = None;
        let mut terminator = None;
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "after" {
                reject_duplicate(&after, key.span(), "payload after")?;
                after = Some(Path::parse_mod_style(input)?);
            } else if key == "terminator" {
                reject_duplicate(&terminator, key.span(), "payload terminator")?;
                terminator = Some(input.parse()?);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unsupported payload setting `{key}`"),
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let terminator: syn::LitStr =
            terminator.ok_or_else(|| input.error("payload requires terminator"))?;
        if terminator.value().is_empty() || terminator.value().contains(['\r', '\n']) {
            return Err(syn::Error::new(
                terminator.span(),
                "payload terminator must be nonempty single-line text",
            ));
        }
        Ok(Self {
            after: after.ok_or_else(|| input.error("payload requires after"))?,
            terminator,
        })
    }
}

/// Parses one bracketed, comma-separated fixed-token name list.
fn parse_ident_list(input: ParseStream<'_>) -> syn::Result<Vec<syn::Ident>> {
    let content;
    syn::bracketed!(content in input);
    Ok(
        Punctuated::<syn::Ident, Token![,]>::parse_terminated_with(
            &content,
            syn::Ident::parse_any,
        )?
        .into_iter()
        .collect(),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Validated grammar-wide defaults for generated Pretty implementations.
pub(crate) struct PrettyDefaults {
    /// Preferred maximum rendered line width.
    pub(crate) max_width: usize,
    /// Spaces added for each indentation level.
    pub(crate) indent: usize,
}

impl Default for PrettyDefaults {
    fn default() -> Self {
        Self {
            max_width: 88,
            indent: 2,
        }
    }
}

impl Parse for PrettyDefaults {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut max_width = None;
        let mut indent = None;
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitInt = input.parse()?;
            let parsed = value.base10_parse::<usize>()?;
            if key == "max_width" {
                reject_duplicate(&max_width, key.span(), "max_width Pretty")?;
                max_width = Some(parsed);
            } else if key == "indent" {
                reject_duplicate(&indent, key.span(), "indent Pretty")?;
                indent = Some(parsed);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unsupported Pretty setting `{key}`"),
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let defaults = Self::default();
        Ok(Self {
            max_width: max_width.unwrap_or(defaults.max_width),
            indent: indent.unwrap_or(defaults.indent),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the capture mode syntax forms accepted before semantic resolution.
pub(crate) enum CaptureMode {
    /// Identifies the off authored syntax form.
    Off,
    /// Identifies the optional authored syntax form.
    Optional,
    /// Identifies the mandatory authored syntax form.
    Mandatory,
}

impl CaptureMode {
    /// Parses the closed `off`, `optional`, and `mandatory` span-capture vocabulary.
    pub(crate) fn parse(value: syn::Ident) -> syn::Result<Self> {
        if value == "off" {
            Ok(Self::Off)
        } else if value == "optional" {
            Ok(Self::Optional)
        } else if value == "mandatory" {
            Ok(Self::Mandatory)
        } else {
            Err(syn::Error::new(
                value.span(),
                "span capture must be `off`, `optional`, or `mandatory`",
            ))
        }
    }
}

/// Derives the capture override from parsed Rust syntax without resolving semantic targets.
pub(crate) fn capture_override(metas: &[Meta]) -> syn::Result<Option<CaptureMode>> {
    let mut capture = None;
    for meta in metas.iter().filter(|meta| meta.path().is_ident("spanned")) {
        if capture.is_some() {
            return Err(syn::Error::new(
                meta.span(),
                "duplicate `spanned` attribute",
            ));
        }
        let Meta::List(list) = meta else {
            return Err(syn::Error::new(
                meta.span(),
                "`spanned` requires one of `off`, `optional`, or `mandatory`",
            ));
        };
        capture = Some(CaptureMode::parse(list.parse_args()?)?);
    }
    Ok(capture)
}

#[cfg(test)]
mod capture_tests {
    use super::{CaptureMode, GrammarConfigSyntax, ParserStyle, capture_override, path_text};

    #[test]
    fn framing_requires_a_complete_island_variant_payload_transition() {
        let syntax: GrammarConfigSyntax = syn::parse_str(
            r#"module = crate, framing(
                island = Entry,
                boundary = SEMI,
                line = [MetaLine],
                delimited = [RawBlock],
                payload(after = Entry::CopyFromStdin, terminator = "\\.")
            )"#,
        )
        .expect("complete-variant framing declaration parses");
        let framing = syntax.framing.expect("framing retained");
        assert_eq!(path_text(&framing.island), "Entry");
        assert_eq!(framing.boundary, "SEMI");
        assert_eq!(framing.line, ["MetaLine"]);
        assert_eq!(framing.delimited, ["RawBlock"]);
        let payload = framing.payload.expect("payload retained");
        assert_eq!(path_text(&payload.after), "Entry::CopyFromStdin");
        assert_eq!(payload.terminator.value(), "\\.");

        let token_trigger = syn::parse_str::<GrammarConfigSyntax>(
            r#"module = crate, framing(
                island = Entry,
                boundary = SEMI,
                payload(trigger = COPY, terminator = "\\.")
            )"#,
        )
        .unwrap_err();
        assert_eq!(
            token_trigger.to_string(),
            "unsupported payload setting `trigger`"
        );
    }

    #[test]
    fn grammar_span_modes_are_typed_and_the_compatibility_default_is_optional() {
        for (authored, expected) in [
            ("off", CaptureMode::Off),
            ("optional", CaptureMode::Optional),
            ("mandatory", CaptureMode::Mandatory),
        ] {
            let syntax: GrammarConfigSyntax =
                syn::parse_str(&format!("module = crate, spans = {authored}"))
                    .expect("supported span mode parses");
            assert_eq!(syntax.spans, expected);
        }
        let syntax: GrammarConfigSyntax =
            syn::parse_str("module = crate").expect("default grammar configuration parses");
        assert_eq!(syntax.spans, CaptureMode::Optional);
    }

    #[test]
    fn invalid_span_configuration_messages_are_pinned() {
        let error =
            syn::parse_str::<GrammarConfigSyntax>("module = crate, spans = sometimes").unwrap_err();
        assert_eq!(
            error.to_string(),
            "span capture must be `off`, `optional`, or `mandatory`"
        );

        let malformed: Vec<syn::Meta> = vec![syn::parse_quote!(spanned)];
        assert_eq!(
            capture_override(&malformed).unwrap_err().to_string(),
            "`spanned` requires one of `off`, `optional`, or `mandatory`"
        );

        let duplicate: Vec<syn::Meta> = vec![
            syn::parse_quote!(spanned(off)),
            syn::parse_quote!(spanned(optional)),
        ];
        assert_eq!(
            capture_override(&duplicate).unwrap_err().to_string(),
            "duplicate `spanned` attribute"
        );
    }

    #[test]
    fn diagrams_are_an_explicit_non_default_generation_request() {
        let core: GrammarConfigSyntax =
            syn::parse_str("module = crate").expect("core grammar configuration parses");
        assert!(!core.diagrams);

        let diagrams: GrammarConfigSyntax = syn::parse_str("module = crate, diagrams")
            .expect("diagram grammar configuration parses");
        assert!(diagrams.diagrams);

        let duplicate = syn::parse_str::<GrammarConfigSyntax>("module = crate, diagrams, diagrams")
            .unwrap_err();
        assert_eq!(duplicate.to_string(), "duplicate diagrams setting");
    }

    #[test]
    fn pretty_defaults_are_typed_and_caller_configuration_is_retained() {
        let defaults: GrammarConfigSyntax =
            syn::parse_str("module = crate").expect("default configuration parses");
        assert_eq!(defaults.pretty.max_width, 88);
        assert_eq!(defaults.pretty.indent, 2);

        let configured: GrammarConfigSyntax =
            syn::parse_str("module = crate, pretty(max_width = 44, indent = 4)")
                .expect("Pretty configuration parses");
        assert_eq!(configured.pretty.max_width, 44);
        assert_eq!(configured.pretty.indent, 4);
    }

    #[test]
    fn duplicate_or_unknown_pretty_settings_are_rejected() {
        let duplicate = syn::parse_str::<GrammarConfigSyntax>(
            "module = crate, pretty(max_width = 44, max_width = 88)",
        )
        .unwrap_err();
        assert_eq!(duplicate.to_string(), "duplicate max_width Pretty setting");

        let unknown =
            syn::parse_str::<GrammarConfigSyntax>("module = crate, pretty(tab_width = 4)")
                .unwrap_err();
        assert_eq!(
            unknown.to_string(),
            "unsupported Pretty setting `tab_width`"
        );
    }
    #[test]
    fn parser_style_accepts_exactly_the_two_styles_and_defaults_to_recursive_descent() {
        let omitted = syn::parse_str::<GrammarConfigSyntax>("module = crate").expect("omitted");
        assert!(omitted.parsers.is_implicit_default());
        assert_eq!(
            omitted.parsers.default_parser().style,
            ParserStyle::RecursiveDescent
        );
        assert_eq!(omitted.parsers.default_parser().name, None);

        let explicit = syn::parse_str::<GrammarConfigSyntax>(
            "module = crate, parser_style = recursive_descent",
        )
        .expect("explicit recursive descent");
        assert_eq!(explicit.parsers, omitted.parsers);
        assert!(explicit.parsers.is_implicit_default());

        let table =
            syn::parse_str::<GrammarConfigSyntax>("module = crate, parser_style = table_driven")
                .expect("table driven");
        assert!(!table.parsers.is_implicit_default());
        assert_eq!(
            table.parsers.default_parser().style,
            ParserStyle::TableDriven
        );
        assert_eq!(table.parsers.iter().count(), 1);

        let error = syn::parse_str::<GrammarConfigSyntax>("module = crate, parser_style = lalr")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "unsupported parser style `lalr`; expected `recursive_descent` or `table_driven`"
        );
        let error = syn::parse_str::<GrammarConfigSyntax>(
            "module = crate, parser_style = table_driven, parser_style = table_driven",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "duplicate parser_style setting");
    }

    #[test]
    fn named_parsers_keep_authored_order_and_the_first_is_the_default() {
        let config = syn::parse_str::<GrammarConfigSyntax>(
            "module = crate, parsers(rd = recursive_descent, lr = table_driven)",
        )
        .expect("named parsers");
        assert!(!config.parsers.is_implicit_default());
        let declared = config
            .parsers
            .iter()
            .map(|parser| (parser.name.as_ref().map(ToString::to_string), parser.style))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            vec![
                (Some("rd".to_string()), ParserStyle::RecursiveDescent),
                (Some("lr".to_string()), ParserStyle::TableDriven),
            ]
        );
        assert_eq!(
            config.parsers.default_parser().style,
            ParserStyle::RecursiveDescent
        );
        assert_eq!(
            config
                .parsers
                .default_parser()
                .name
                .as_ref()
                .map(ToString::to_string),
            Some("rd".to_string())
        );

        let single = syn::parse_str::<GrammarConfigSyntax>(
            "module = crate, parsers(rd = recursive_descent)",
        )
        .expect("one named parser");
        assert!(
            !single.parsers.is_implicit_default(),
            "a named parser is a distinct fact from the implicit default"
        );
    }

    #[test]
    fn invalid_parser_configurations_are_rejected() {
        for (source, message) in [
            (
                "module = crate, parser_style = table_driven, parsers(lr = table_driven)",
                "parser_style and parsers are mutually exclusive",
            ),
            (
                "module = crate, parsers(lr = table_driven), parser_style = table_driven",
                "parser_style and parsers are mutually exclusive",
            ),
            (
                "module = crate, parsers()",
                "parsers requires at least one named parser",
            ),
            (
                "module = crate, parsers(lr = table_driven, lr = recursive_descent)",
                "duplicate parser name `lr`",
            ),
            (
                "module = crate, parsers(lr = table_driven), parsers(rd = recursive_descent)",
                "duplicate parsers setting",
            ),
            (
                "module = crate, parsers(lr = glr)",
                "unsupported parser style `glr`; expected `recursive_descent` or `table_driven`",
            ),
            (
                "module = crate, parsers(a = recursive_descent, b = recursive_descent)",
                "parser `b` repeats the `recursive_descent` style of `a`; one generation emits one parser per style",
            ),
            (
                "module = crate, parsers(rd = recursive_descent, lr = table_driven, lalr = table_driven)",
                "parser `lalr` repeats the `table_driven` style of `lr`; one generation emits one parser per style",
            ),
        ] {
            let error = syn::parse_str::<GrammarConfigSyntax>(source).unwrap_err();
            assert_eq!(error.to_string(), message, "{source}");
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the invalid keyword matching syntax parsed from authored Rust syntax before semantic resolution.
pub(crate) struct InvalidKeywordMatchingSyntax {
    /// Supplies the authored consumed by later discovery and resolution phases.
    pub(crate) authored: String,
    /// Supplies the range consumed by later discovery and resolution phases.
    pub(crate) range: std::ops::Range<u32>,
}

impl Parse for GrammarConfigSyntax {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut module = None;
        let mut facade = None;
        let mut text_equivalence = None;
        let mut invalid_keyword_matching = None;
        let mut max_lookahead = None;
        let mut spans = None;
        let mut derives = None;
        let mut diagrams = false;
        let mut pretty = None;
        let mut framing = None;
        let mut parser_style = None;
        let mut parsers = None;
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            if key == "derives" {
                reject_duplicate(&derives, key.span(), "derives")?;
                let content;
                syn::parenthesized!(content in input);
                derives = Some(
                    syn::punctuated::Punctuated::<Path, Token![,]>::parse_terminated(&content)?
                        .into_iter()
                        .collect(),
                );
            } else if key == "diagrams" {
                if diagrams {
                    return Err(syn::Error::new(key.span(), "duplicate diagrams setting"));
                }
                diagrams = true;
            } else if key == "pretty" {
                reject_duplicate(&pretty, key.span(), "pretty")?;
                let content;
                syn::parenthesized!(content in input);
                pretty = Some(content.parse()?);
            } else if key == "framing" {
                reject_duplicate(&framing, key.span(), "framing")?;
                let content;
                syn::parenthesized!(content in input);
                framing = Some(content.parse()?);
            } else if key == "parsers" {
                reject_duplicate(&parsers, key.span(), "parsers")?;
                let content;
                syn::parenthesized!(content in input);
                let declared =
                    syn::punctuated::Punctuated::<ParserDeclaration, Token![,]>::parse_terminated(
                        &content,
                    )?
                    .into_iter()
                    .collect();
                parsers = Some(ParserDeclarations::named(declared, key.span())?);
            } else {
                input.parse::<Token![=]>()?;
                if key == "module" {
                    reject_duplicate(&module, key.span(), "module")?;
                    module = Some(Path::parse_mod_style(input)?);
                } else if key == "facade" {
                    reject_duplicate(&facade, key.span(), "facade")?;
                    facade = Some(input.parse()?);
                } else if key == "keyword_matching" {
                    reject_duplicate(&text_equivalence, key.span(), "keyword_matching")?;
                    let value: syn::Ident = input.parse()?;
                    text_equivalence = Some(if value == "sensitive" {
                        TextEquivalence::Sensitive
                    } else if value == "ascii_insensitive" {
                        TextEquivalence::AsciiInsensitive
                    } else {
                        invalid_keyword_matching = Some(InvalidKeywordMatchingSyntax {
                            authored: value.to_string(),
                            range: span_range(value.span()),
                        });
                        TextEquivalence::Sensitive
                    });
                } else if key == "max_lookahead" {
                    reject_duplicate(&max_lookahead, key.span(), "max_lookahead")?;
                    let value: LitInt = input.parse()?;
                    max_lookahead = Some(value.base10_parse()?);
                } else if key == "spans" {
                    reject_duplicate(&spans, key.span(), "spans")?;
                    spans = Some(CaptureMode::parse(input.parse()?)?);
                } else if key == "parser_style" {
                    reject_duplicate(&parser_style, key.span(), "parser_style")?;
                    let value: syn::Ident = input.parse()?;
                    parser_style = Some((ParserStyle::parse_ident(&value)?, key.span()));
                } else {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unsupported grammar setting `{key}`"),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let facade_explicit = facade.is_some();
        let parsers = match (parser_style, parsers) {
            (Some((_, span)), Some(_)) => {
                return Err(syn::Error::new(
                    span,
                    "parser_style and parsers are mutually exclusive",
                ));
            }
            (Some((style, _)), None) => ParserDeclarations::single(style),
            (None, Some(parsers)) => parsers,
            (None, None) => ParserDeclarations::default(),
        };
        Ok(Self {
            module: module.ok_or_else(|| input.error("grammar configuration requires module"))?,
            facade: facade.unwrap_or_else(|| syn::Ident::new("recursa", Span::call_site())),
            facade_explicit,
            text_equivalence: text_equivalence.unwrap_or(TextEquivalence::Sensitive),
            invalid_keyword_matching,
            max_lookahead: max_lookahead.unwrap_or(1),
            // Optional preserves the pre-configuration generated API behavior.
            spans: spans.unwrap_or(CaptureMode::Optional),
            derives: derives.unwrap_or_default(),
            diagrams,
            pretty: pretty.unwrap_or_default(),
            framing,
            parsers,
        })
    }
}

impl Parse for ParserDeclaration {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name: syn::Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let style: syn::Ident = input.parse()?;
        Ok(Self {
            name: Some(name),
            style: ParserStyle::parse_ident(&style)?,
        })
    }
}

/// Rejects a repeated configuration key at the second authored span.
fn reject_duplicate<T>(value: &Option<T>, span: Span, name: &str) -> syn::Result<()> {
    if value.is_some() {
        Err(syn::Error::new(span, format!("duplicate {name} setting")))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug)]
/// Carries the configured attributes parsed from authored Rust syntax before semantic resolution.
pub(crate) struct ConfiguredAttributes {
    /// Supplies the metas consumed by later discovery and resolution phases.
    pub(crate) metas: Vec<Meta>,
}

#[derive(Clone, Debug)]
/// Carries the configuration error parsed from authored Rust syntax before semantic resolution.
pub(crate) struct ConfigurationError {
    /// Supplies the kind consumed by later discovery and resolution phases.
    pub(crate) kind: ConfigurationErrorKind,
    /// Supplies the range consumed by later discovery and resolution phases.
    pub(crate) range: Range<u32>,
    /// Supplies the message consumed by later discovery and resolution phases.
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Classifies the configuration error kind syntax forms accepted before semantic resolution.
pub(crate) enum ConfigurationErrorKind {
    /// Identifies the unknown authored syntax form.
    Unknown,
    /// Identifies the invocation only authored syntax form.
    InvocationOnly,
    /// Identifies the malformed authored syntax form.
    Malformed,
}

/// Applies active `cfg` and `cfg_attr` expressions without expanding unrelated attributes.
pub(crate) fn configure_attributes(
    attributes: &[Attribute],
    cfg: &CargoCfgSnapshot,
) -> Result<Option<ConfiguredAttributes>, ConfigurationError> {
    let mut metas = Vec::new();
    let mut active = true;
    for attribute in attributes {
        apply_meta(&attribute.meta, cfg, &mut active, &mut metas)?;
        if !active {
            return Ok(None);
        }
    }
    Ok(Some(ConfiguredAttributes { metas }))
}

/// Derives the configured attribute metas from parsed Rust syntax without resolving semantic targets.
pub(crate) fn configured_attribute_metas(
    attributes: &[Attribute],
    cfg: &CargoCfgSnapshot,
) -> Result<Option<Vec<Meta>>, ConfigurationError> {
    configure_attributes(attributes, cfg)
        .map(|configured| configured.map(|configured| configured.metas))
}

/// Derives the item attributes from parsed Rust syntax without resolving semantic targets.
pub(crate) fn item_attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        Item::Verbatim(_) => &[],
        _ => &[],
    }
}

/// Derives the path text from parsed Rust syntax without resolving semantic targets.
pub(crate) fn path_text(path: &Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Evaluates one configuration meta item and emits any enabled nested attributes.
fn apply_meta(
    meta: &Meta,
    cfg: &CargoCfgSnapshot,
    active: &mut bool,
    output: &mut Vec<Meta>,
) -> Result<(), ConfigurationError> {
    if meta.path().is_ident("cfg") {
        let Meta::List(list) = meta else {
            return Err(configuration_error(meta.span(), "cfg must use parentheses"));
        };
        let predicate = syn::parse2::<Meta>(list.tokens.clone())
            .map_err(|error| configuration_error(error.span(), error.to_string()))?;
        *active &= evaluate_cfg(&predicate, cfg)?;
    } else if meta.path().is_ident("cfg_attr") {
        let Meta::List(list) = meta else {
            return Err(configuration_error(
                meta.span(),
                "cfg_attr must use parentheses",
            ));
        };
        let nested = Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| configuration_error(error.span(), error.to_string()))?;
        let Some(predicate) = nested.first() else {
            return Err(configuration_error(
                meta.span(),
                "cfg_attr requires a predicate",
            ));
        };
        if evaluate_cfg(predicate, cfg)? {
            for nested in nested.iter().skip(1) {
                apply_meta(nested, cfg, active, output)?;
                if !*active {
                    break;
                }
            }
        }
    } else {
        output.push(meta.clone());
    }
    Ok(())
}

/// Derives the configuration error from parsed Rust syntax without resolving semantic targets.
fn configuration_error(span: Span, message: impl Into<String>) -> ConfigurationError {
    ConfigurationError {
        kind: ConfigurationErrorKind::Malformed,
        range: span_range(span),
        message: message.into(),
    }
}

/// Evaluates the supported Cargo configuration predicate vocabulary.
fn evaluate_cfg(meta: &Meta, snapshot: &CargoCfgSnapshot) -> Result<bool, ConfigurationError> {
    let name = meta
        .path()
        .get_ident()
        .ok_or_else(|| configuration_error(meta.span(), "cfg predicate must be an identifier"))?
        .to_string();
    match meta {
        Meta::Path(_) => snapshot
            .flag(&name)
            .map_err(|failure| cfg_lookup_error(failure, meta.span(), &name)),
        Meta::NameValue(name_value) => {
            let Expr::Lit(expression) = &name_value.value else {
                return Err(configuration_error(
                    name_value.value.span(),
                    "cfg value must be a string literal",
                ));
            };
            let Lit::Str(value) = &expression.lit else {
                return Err(configuration_error(
                    expression.lit.span(),
                    "cfg value must be a string literal",
                ));
            };
            snapshot
                .value(&name, &value.value())
                .map_err(|failure| cfg_lookup_error(failure, meta.span(), &name))
        }
        Meta::List(list) => {
            let nested = Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .map_err(|error| configuration_error(error.span(), error.to_string()))?;
            let values = nested
                .iter()
                .map(|predicate| evaluate_cfg(predicate, snapshot))
                .collect::<Result<Vec<_>, _>>()?;
            match name.as_str() {
                "all" => Ok(values.into_iter().all(|value| value)),
                "any" => Ok(values.into_iter().any(|value| value)),
                "not" if values.len() == 1 => Ok(!values[0]),
                "not" => Err(configuration_error(
                    meta.span(),
                    "not requires one predicate",
                )),
                _ => Err(configuration_error(
                    meta.span(),
                    "cfg supports only all, any, and not predicate functions",
                )),
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// Classifies the cfg lookup failure syntax forms accepted before semantic resolution.
enum CfgLookupFailure {
    /// Identifies the unknown authored syntax form.
    Unknown,
    /// Identifies the invocation only authored syntax form.
    InvocationOnly,
}

/// Derives the cfg lookup error from parsed Rust syntax without resolving semantic targets.
fn cfg_lookup_error(failure: CfgLookupFailure, span: Span, name: &str) -> ConfigurationError {
    match failure {
        CfgLookupFailure::Unknown => ConfigurationError {
            kind: ConfigurationErrorKind::Unknown,
            range: span_range(span),
            message: format!("cfg `{name}` is absent from Cargo's target snapshot"),
        },
        CfgLookupFailure::InvocationOnly => ConfigurationError {
            kind: ConfigurationErrorKind::InvocationOnly,
            range: span_range(span),
            message: format!("cfg `{name}` is unavailable to build-script discovery"),
        },
    }
}

/// Derives the span range from parsed Rust syntax without resolving semantic targets.
pub(crate) fn span_range(span: Span) -> Range<u32> {
    let range = span.byte_range();
    range.start.try_into().unwrap_or(u32::MAX)..range.end.try_into().unwrap_or(u32::MAX)
}
