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
    use super::{CaptureMode, GrammarConfigSyntax, capture_override, path_text};

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
    fn removed_predictive_parser_settings_have_targeted_migration_errors() {
        for (source, message) in [
            (
                "module = crate, parser_style = recursive_descent",
                "parser selection has been removed; Recursa generates one LR parser",
            ),
            (
                "module = crate, parsers(rd = recursive_descent)",
                "named parsers have been removed; Recursa generates one LR parser",
            ),
            (
                "module = crate, max_lookahead = 3",
                "max_lookahead has been removed; LR parsing does not use predictive lookahead depth",
            ),
        ] {
            assert_eq!(
                syn::parse_str::<GrammarConfigSyntax>(source)
                    .unwrap_err()
                    .to_string(),
                message,
            );
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
        let mut spans = None;
        let mut derives = None;
        let mut diagrams = false;
        let mut pretty = None;
        let mut framing = None;
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
                return Err(syn::Error::new(
                    key.span(),
                    "named parsers have been removed; Recursa generates one LR parser",
                ));
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
                    let _value: LitInt = input.parse()?;
                    return Err(syn::Error::new(
                        key.span(),
                        "max_lookahead has been removed; LR parsing does not use predictive lookahead depth",
                    ));
                } else if key == "spans" {
                    reject_duplicate(&spans, key.span(), "spans")?;
                    spans = Some(CaptureMode::parse(input.parse()?)?);
                } else if key == "parser_style" {
                    let _value: syn::Ident = input.parse()?;
                    return Err(syn::Error::new(
                        key.span(),
                        "parser selection has been removed; Recursa generates one LR parser",
                    ));
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
        Ok(Self {
            module: module.ok_or_else(|| input.error("grammar configuration requires module"))?,
            facade: facade.unwrap_or_else(|| syn::Ident::new("recursa", Span::call_site())),
            facade_explicit,
            text_equivalence: text_equivalence.unwrap_or(TextEquivalence::Sensitive),
            invalid_keyword_matching,
            // Optional preserves the pre-configuration generated API behavior.
            spans: spans.unwrap_or(CaptureMode::Optional),
            derives: derives.unwrap_or_default(),
            diagrams,
            pretty: pretty.unwrap_or_default(),
            framing,
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
