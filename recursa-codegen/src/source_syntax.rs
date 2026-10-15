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
    /// Resolved configuration scale declared by the root grammar, when one exists.
    configurations: Option<std::sync::Arc<ConfigurationScale>>,
}

impl CargoCfgSnapshot {
    /// Attaches the resolved configuration scale used by `config` gates.
    pub(crate) fn with_configurations(mut self, scale: ConfigurationScale) -> Self {
        self.configurations = Some(std::sync::Arc::new(scale));
        self
    }

    /// Returns the resolved configuration scale, when the grammar declares one.
    pub(crate) fn configurations(&self) -> Option<&ConfigurationScale> {
        self.configurations.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The grammar's ordered named configurations together with the one this build states.
///
/// The names come from the root `grammar!` `configurations(...)` list, in ascending order.
/// `active` is the index of the highest name whose Cargo feature is enabled, so it is the
/// configuration that this build reproduces.
pub(crate) struct ConfigurationScale {
    /// Declared names in ascending order.
    pub(crate) names: Vec<String>,
    /// Index of the configuration that this build states.
    pub(crate) active: usize,
}

impl ConfigurationScale {
    /// Returns the ordinal of one declared name.
    pub(crate) fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|declared| declared == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One entry of the root `grammar!` `configurations(...)` list.
pub(crate) struct ConfigurationSyntax {
    /// Authored name, which a `config` gate refers to.
    pub(crate) name: syn::Ident,
    /// Cargo feature that enables this configuration; the first entry has none.
    pub(crate) feature: Option<syn::LitStr>,
    /// Authored range of the name.
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// One authored `#[config(...)]` declaration.
///
/// The gate both removes its element from a build that the range excludes and, for a `since`
/// bound, records the requirement that generation publishes.
pub(crate) struct ConfigGate {
    /// Lowest configuration that admits the element; `None` means the baseline admits it.
    pub(crate) since: Option<String>,
    /// First configuration that no longer admits the element.
    pub(crate) before: Option<String>,
    /// Whether the requirement belongs to the absence of the value rather than its presence.
    pub(crate) on_absent: bool,
    /// Opaque diagnostic code carried to the consumer.
    pub(crate) code: Option<String>,
    /// Opaque message carried to the consumer.
    pub(crate) message: Option<String>,
    /// Opaque hint carried to the consumer.
    pub(crate) hint: Option<String>,
    /// Citation of the source that justifies the gate.
    pub(crate) citation: Option<String>,
}

impl ConfigGate {
    /// Reads one `config(...)` meta without resolving it against a build configuration.
    pub(crate) fn from_meta(meta: &Meta) -> Result<Self, ConfigurationError> {
        let Meta::List(list) = meta else {
            return Err(configuration_error(
                meta.span(),
                "config must use parentheses",
            ));
        };
        let entries = Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| configuration_error(error.span(), error.to_string()))?;
        let mut gate = Self::default();
        for entry in &entries {
            let Meta::NameValue(pair) = entry else {
                return Err(configuration_error(
                    entry.span(),
                    "config accepts only `key = value` entries",
                ));
            };
            let key = pair
                .path
                .get_ident()
                .ok_or_else(|| configuration_error(pair.span(), "config key must be a name"))?
                .unraw()
                .to_string();
            match key.as_str() {
                "since" | "before" => {
                    let name = configuration_name(&pair.value)?;
                    let slot = if key == "since" {
                        &mut gate.since
                    } else {
                        &mut gate.before
                    };
                    if slot.is_some() {
                        return Err(configuration_error(
                            pair.span(),
                            format!("duplicate config {key} bound"),
                        ));
                    }
                    *slot = Some(name);
                }
                "on" => {
                    let name = configuration_name(&pair.value)?;
                    match name.as_str() {
                        "absent" => gate.on_absent = true,
                        "present" => gate.on_absent = false,
                        _ => {
                            return Err(configuration_error(
                                pair.span(),
                                "config `on` accepts only `present` or `absent`",
                            ));
                        }
                    }
                }
                "code" | "message" | "hint" | "cite" => {
                    let text = literal_text(&pair.value)?;
                    let slot = match key.as_str() {
                        "code" => &mut gate.code,
                        "message" => &mut gate.message,
                        "hint" => &mut gate.hint,
                        _ => &mut gate.citation,
                    };
                    if slot.is_some() {
                        return Err(configuration_error(
                            pair.span(),
                            format!("duplicate config {key} entry"),
                        ));
                    }
                    *slot = Some(text);
                }
                other => {
                    return Err(configuration_error(
                        pair.span(),
                        format!("unsupported config entry `{other}`"),
                    ));
                }
            }
        }
        if gate.since.is_none() && gate.before.is_none() {
            return Err(configuration_error(
                meta.span(),
                "config requires `since` or `before`",
            ));
        }
        Ok(gate)
    }

    /// Reports whether the build configuration admits the gated element.
    pub(crate) fn admits(
        &self,
        meta: &Meta,
        cfg: &CargoCfgSnapshot,
    ) -> Result<bool, ConfigurationError> {
        let Some(scale) = cfg.configurations() else {
            return Err(configuration_error(
                meta.span(),
                "config requires a `configurations(...)` list in the root grammar declaration",
            ));
        };
        let bound = |name: &Option<String>| match name {
            None => Ok(None),
            Some(name) => scale.index(name).map(Some).ok_or_else(|| {
                configuration_error(meta.span(), format!("unknown configuration `{name}`"))
            }),
        };
        let since = bound(&self.since)?;
        let before = bound(&self.before)?;
        if let (Some(since), Some(before)) = (since, before)
            && since >= before
        {
            return Err(configuration_error(
                meta.span(),
                "config `since` must name a lower configuration than `before`",
            ));
        }
        Ok(since.is_none_or(|since| since <= scale.active)
            && before.is_none_or(|before| scale.active < before))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One forbidden pair of labels written by a `#[reject(...)]` declaration.
pub(crate) struct RejectPairSyntax {
    /// Name written first inside `pair(...)`.
    pub(crate) left: String,
    /// Name written second inside `pair(...)`.
    pub(crate) right: String,
    /// Authored range of the whole `pair(...)` entry.
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The one test a `#[reject(...)]` declaration makes.
///
/// One declaration carries one message, so it makes one kind of test: any number of forbidden
/// pairs, one `once` set, or one text rule (`docs/declared-rejections.md`).
pub(crate) enum RejectCheckSyntax {
    /// `pair(A, B), pair(C, D), ...`: forbidden unordered pairs of labels, in written order.
    Pairs(Vec<RejectPairSyntax>),
    /// `once(A, B, ...)`: at most one item whose label is in the set.
    Once {
        /// Labels of the set, in written order.
        labels: Vec<String>,
        /// Authored range of the `once(...)` entry.
        range: Range<u32>,
    },
    /// `one_of(V, "a", ...)` or `not_in(V, "a", ...)`: an item of variant `V` is refused when its
    /// folded text is (`one_of`) or is not (`not_in`) one of the texts.
    Text {
        /// `true` for `not_in`, `false` for `one_of`.
        negated: bool,
        /// Name of the classified variant whose items the rule reads, or, on a struct, of the
        /// classified field.
        variant: String,
        /// Texts in written order.
        texts: Vec<String>,
        /// Authored range of the entry.
        range: Range<u32>,
    },
}

impl RejectCheckSyntax {
    /// Every label this check names, with the authored range of the entry that names it.
    ///
    /// A text rule names a variant rather than a label, and is reported apart.
    pub(crate) fn labels(&self) -> Vec<(&str, Range<u32>)> {
        match self {
            Self::Pairs(pairs) => pairs
                .iter()
                .flat_map(|pair| {
                    [
                        (pair.left.as_str(), pair.range.clone()),
                        (pair.right.as_str(), pair.range.clone()),
                    ]
                })
                .collect(),
            Self::Once { labels, range } => labels
                .iter()
                .map(|label| (label.as_str(), range.clone()))
                .collect(),
            Self::Text { .. } => Vec::new(),
        }
    }

    /// Renders the check for the one generated documentation line of its enum.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Pairs(pairs) => pairs
                .iter()
                .map(|pair| format!("`{}` with `{}`", pair.left, pair.right))
                .collect::<Vec<_>>()
                .join(", "),
            Self::Once { labels, .. } => format!(
                "more than one of {}",
                labels
                    .iter()
                    .map(|label| format!("`{label}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Text {
                negated,
                variant,
                texts,
                ..
            } => format!(
                "`{variant}` named {}{}",
                if *negated { "other than " } else { "" },
                texts
                    .iter()
                    .map(|text| format!("`{text}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One authored `#[reject(...)]` declaration: one test and its message data.
///
/// The declaration carries the message data a configuration gate carries, and `message` and
/// `cite` are required rather than optional: a rule that rejects a parse must say why and must
/// name the source that rejects it (`docs/declared-rejections.md`).
pub(crate) struct RejectRuleSyntax {
    /// The test the rule makes.
    pub(crate) check: RejectCheckSyntax,
    /// Opaque diagnostic code carried to the consumer.
    pub(crate) code: Option<String>,
    /// Opaque message carried to the consumer.
    pub(crate) message: String,
    /// Opaque hint carried to the consumer.
    pub(crate) hint: Option<String>,
    /// Citation of the source that rejects the item.
    pub(crate) citation: String,
    /// Authored range of the whole declaration.
    pub(crate) range: Range<u32>,
}

impl RejectRuleSyntax {
    /// Reads one `reject(...)` meta into its test and its message data.
    ///
    /// Only a declaration that cannot be read at all fails here. Every failure that needs the
    /// enum's resolved variants — an unknown name, a pair of one name, a repeated pair, a
    /// declaration on something other than an enum — is reported by source-model lowering at its
    /// own registered code.
    pub(crate) fn from_meta(meta: &Meta) -> Result<Self, ConfigurationError> {
        let Meta::List(list) = meta else {
            return Err(configuration_error(
                meta.span(),
                "reject must use parentheses",
            ));
        };
        let entries = Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| configuration_error(error.span(), error.to_string()))?;
        let mut pairs = Vec::new();
        let mut single: Option<RejectCheckSyntax> = None;
        let (mut code, mut message, mut hint, mut citation) = (None, None, None, None);
        for entry in &entries {
            match entry {
                Meta::List(pair) if pair.path.is_ident("pair") => {
                    pairs.push(reject_pair(pair)?);
                }
                Meta::List(entry)
                    if entry.path.is_ident("once")
                        || entry.path.is_ident("one_of")
                        || entry.path.is_ident("not_in") =>
                {
                    if single.is_some() {
                        return Err(configuration_error(
                            entry.span(),
                            "reject makes one test: write one `once(...)`, `one_of(...)` or \
                             `not_in(...)` per declaration",
                        ));
                    }
                    single = Some(if entry.path.is_ident("once") {
                        reject_once(entry)?
                    } else {
                        reject_text(entry, entry.path.is_ident("not_in"))?
                    });
                }
                Meta::NameValue(assignment) => {
                    let key = assignment
                        .path
                        .get_ident()
                        .ok_or_else(|| {
                            configuration_error(assignment.span(), "reject key must be a name")
                        })?
                        .unraw()
                        .to_string();
                    let slot = match key.as_str() {
                        "code" => &mut code,
                        "message" => &mut message,
                        "hint" => &mut hint,
                        "cite" => &mut citation,
                        other => {
                            return Err(configuration_error(
                                assignment.span(),
                                format!("unsupported reject entry `{other}`"),
                            ));
                        }
                    };
                    if slot.is_some() {
                        return Err(configuration_error(
                            assignment.span(),
                            format!("duplicate reject {key} entry"),
                        ));
                    }
                    *slot = Some(literal_text(&assignment.value)?);
                }
                other => {
                    return Err(configuration_error(
                        other.span(),
                        "reject accepts `pair(A, B)`, `once(A, ...)`, `one_of(V, \"text\", ...)`, \
                         `not_in(V, \"text\", ...)` and `key = \"value\"` entries only",
                    ));
                }
            }
        }
        let check = match (single, pairs.is_empty()) {
            (Some(_), false) => {
                return Err(configuration_error(
                    meta.span(),
                    "reject makes one test: `pair(...)` entries do not combine with `once(...)`, \
                     `one_of(...)` or `not_in(...)` in one declaration",
                ));
            }
            (Some(check), true) => check,
            (None, false) => RejectCheckSyntax::Pairs(pairs),
            (None, true) => {
                return Err(configuration_error(
                    meta.span(),
                    "reject requires a `pair(A, B)`, `once(A, ...)`, `one_of(V, ...)` or \
                     `not_in(V, ...)` entry",
                ));
            }
        };
        let message = message
            .ok_or_else(|| configuration_error(meta.span(), "reject requires a `message` entry"))?;
        let citation = citation
            .ok_or_else(|| configuration_error(meta.span(), "reject requires a `cite` entry"))?;
        Ok(Self {
            check,
            code,
            message,
            hint,
            citation,
            range: span_range(meta.span()),
        })
    }
}

/// Reads one `pair(A, B)` entry of a `reject(...)` declaration.
fn reject_pair(pair: &syn::MetaList) -> Result<RejectPairSyntax, ConfigurationError> {
    let names = Punctuated::<syn::Ident, Token![,]>::parse_terminated
        .parse2(pair.tokens.clone())
        .map_err(|error| configuration_error(error.span(), error.to_string()))?;
    let [left, right] = names.iter().collect::<Vec<_>>()[..] else {
        return Err(configuration_error(
            pair.span(),
            "reject `pair(...)` names exactly two labels",
        ));
    };
    Ok(RejectPairSyntax {
        left: left.unraw().to_string(),
        right: right.unraw().to_string(),
        range: span_range(pair.span()),
    })
}

/// Reads one `once(A, ...)` entry of a `reject(...)` declaration.
fn reject_once(entry: &syn::MetaList) -> Result<RejectCheckSyntax, ConfigurationError> {
    let names = Punctuated::<syn::Ident, Token![,]>::parse_terminated
        .parse2(entry.tokens.clone())
        .map_err(|error| configuration_error(error.span(), error.to_string()))?;
    if names.is_empty() {
        return Err(configuration_error(
            entry.span(),
            "reject `once(...)` names at least one label",
        ));
    }
    Ok(RejectCheckSyntax::Once {
        labels: names.iter().map(|name| name.unraw().to_string()).collect(),
        range: span_range(entry.span()),
    })
}

/// Reads one `one_of(V, "a", ...)` or `not_in(V, "a", ...)` entry of a `reject(...)` declaration.
fn reject_text(
    entry: &syn::MetaList,
    negated: bool,
) -> Result<RejectCheckSyntax, ConfigurationError> {
    let operands = Punctuated::<Expr, Token![,]>::parse_terminated
        .parse2(entry.tokens.clone())
        .map_err(|error| configuration_error(error.span(), error.to_string()))?;
    let mut operands = operands.iter();
    let variant = operands
        .next()
        .and_then(|operand| match operand {
            Expr::Path(path) if path.qself.is_none() => path.path.get_ident().cloned(),
            _ => None,
        })
        .ok_or_else(|| {
            configuration_error(
                entry.span(),
                "a text rule names its classified variant first, then its texts",
            )
        })?;
    let texts = operands.map(literal_text).collect::<Result<Vec<_>, _>>()?;
    if texts.is_empty() {
        return Err(configuration_error(
            entry.span(),
            "a text rule names at least one text",
        ));
    }
    Ok(RejectCheckSyntax::Text {
        negated,
        variant: variant.unraw().to_string(),
        texts,
        range: span_range(entry.span()),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One text mapping of a `#[classify(...)]` declaration: `text("path", Path)`.
pub(crate) struct ClassifyTextSyntax {
    /// Folded text the mapping reads.
    pub(crate) text: String,
    /// Label an item with that folded text counts as.
    pub(crate) label: String,
    /// Authored range of the `text(...)` entry.
    pub(crate) range: Range<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One authored `#[classify(Variant.field, fold = path, text("...", Label), ...)]` declaration on
/// an enum, or `#[classify(field, fold = path)]` on a struct.
///
/// On an enum, the declaration reads the identifier `field` of one variant's struct payload, folds
/// its source text with the consumer's `fold` function, and lets an item whose folded text a
/// mapping names count as that mapping's label for the enum's rejection rules. On a struct, it
/// reads the struct's own `field`, and the struct's text rules compare its folded text
/// (`docs/declared-rejections.md`).
pub(crate) struct ClassifySyntax {
    /// Name of the classified variant, or `None` for the struct form, which names a field only.
    pub(crate) variant: Option<String>,
    /// Name of the identifier field of the variant's struct payload.
    pub(crate) field: String,
    /// Path of the consumer's fold function, `fn(&str) -> Option<impl AsRef<str>>`.
    pub(crate) fold: syn::Path,
    /// Text mappings in written order.
    pub(crate) texts: Vec<ClassifyTextSyntax>,
    /// Authored range of the whole declaration.
    pub(crate) range: Range<u32>,
}

impl ClassifySyntax {
    /// Reads one `classify(...)` meta.
    ///
    /// Only a declaration that cannot be read at all fails here; a name that needs the enum's
    /// resolved variants is reported by source-model lowering.
    pub(crate) fn from_meta(meta: &Meta) -> Result<Self, ConfigurationError> {
        let Meta::List(list) = meta else {
            return Err(configuration_error(
                meta.span(),
                "classify must use parentheses",
            ));
        };
        let entries = Punctuated::<Expr, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| configuration_error(error.span(), error.to_string()))?;
        let mut entries = entries.iter();
        let (variant, field) = entries
            .next()
            .and_then(|first| match first {
                Expr::Field(access) => {
                    let Expr::Path(base) = access.base.as_ref() else {
                        return None;
                    };
                    let syn::Member::Named(field) = &access.member else {
                        return None;
                    };
                    let variant = base.path.get_ident()?;
                    Some((Some(variant.unraw().to_string()), field.unraw().to_string()))
                }
                Expr::Path(path) if path.qself.is_none() => {
                    let field = path.path.get_ident()?;
                    Some((None, field.unraw().to_string()))
                }
                _ => None,
            })
            .ok_or_else(|| {
                configuration_error(
                    meta.span(),
                    "classify names the classified field first, as `Variant.field` on an enum \
                     or `field` on a struct",
                )
            })?;
        let mut fold = None;
        let mut texts = Vec::new();
        for entry in entries {
            match entry {
                Expr::Assign(assign) if matches!(assign.left.as_ref(), Expr::Path(path) if path.path.is_ident("fold")) =>
                {
                    let Expr::Path(path) = assign.right.as_ref() else {
                        return Err(configuration_error(
                            assign.right.span(),
                            "classify `fold` names a function path",
                        ));
                    };
                    if fold.replace(path.path.clone()).is_some() {
                        return Err(configuration_error(
                            assign.span(),
                            "duplicate classify fold entry",
                        ));
                    }
                }
                Expr::Call(call) if matches!(call.func.as_ref(), Expr::Path(path) if path.path.is_ident("text")) =>
                {
                    let [text, label] = call.args.iter().collect::<Vec<_>>()[..] else {
                        return Err(configuration_error(
                            call.span(),
                            "classify `text(...)` names one text and one label",
                        ));
                    };
                    let label = match label {
                        Expr::Path(path) if path.qself.is_none() => path.path.get_ident().cloned(),
                        _ => None,
                    }
                    .ok_or_else(|| {
                        configuration_error(
                            label.span(),
                            "classify `text(...)` maps to a label name",
                        )
                    })?;
                    texts.push(ClassifyTextSyntax {
                        text: literal_text(text)?,
                        label: label.unraw().to_string(),
                        range: span_range(call.span()),
                    });
                }
                other => {
                    return Err(configuration_error(
                        other.span(),
                        "classify accepts `fold = path` and `text(\"...\", Label)` entries only",
                    ));
                }
            }
        }
        let fold = fold
            .ok_or_else(|| configuration_error(meta.span(), "classify requires a `fold` entry"))?;
        // The struct form maps no text: the struct's text rules name the texts. Source-model
        // lowering reports a struct form on an enum, and an enum form on a struct.
        if variant.is_some() && texts.is_empty() {
            return Err(configuration_error(
                meta.span(),
                "classify requires at least one `text(\"...\", Label)` entry",
            ));
        }
        if variant.is_none() && !texts.is_empty() {
            return Err(configuration_error(
                meta.span(),
                "a struct's `classify(field, ...)` maps no text: its text rules name the texts",
            ));
        }
        Ok(Self {
            variant,
            field,
            fold,
            texts,
            range: span_range(meta.span()),
        })
    }
}

/// Reads a bare name written as the value of a `config` entry.
fn configuration_name(value: &Expr) -> Result<String, ConfigurationError> {
    match value {
        Expr::Path(path)
            if path.qself.is_none() && path.attrs.is_empty() && path.path.get_ident().is_some() =>
        {
            Ok(path
                .path
                .get_ident()
                .expect("the guard checked for one identifier")
                .unraw()
                .to_string())
        }
        other => Err(configuration_error(
            other.span(),
            "config expects a configuration name",
        )),
    }
}

/// Reads a string literal written as the value of a `config` entry.
fn literal_text(value: &Expr) -> Result<String, ConfigurationError> {
    match value {
        Expr::Lit(literal) => match &literal.lit {
            Lit::Str(text) => Ok(text.value()),
            other => Err(configuration_error(
                other.span(),
                "config expects a string literal",
            )),
        },
        other => Err(configuration_error(
            other.span(),
            "config expects a string literal",
        )),
    }
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
    /// Ordered named configurations that a `config` gate refers to.
    pub(crate) configurations: Vec<ConfigurationSyntax>,
    /// Whether generation should publish railroad diagrams.
    pub(crate) diagrams: bool,
    /// Whether generated semantic boxes and vectors borrow one parse-owned bump arena.
    pub(crate) arena_ast: bool,
    /// Whether generation emits the Flat AST representation beside the nested one.
    pub(crate) flat: bool,
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
    fn the_flat_bare_flag_parses_beside_arena_ast_and_rejects_a_duplicate() {
        let plain: GrammarConfigSyntax =
            syn::parse_str("module = crate").expect("default configuration parses");
        assert!(!plain.flat);

        let selected: GrammarConfigSyntax =
            syn::parse_str("module = crate, flat").expect("the flat flag parses");
        assert!(selected.flat);
        assert!(!selected.arena_ast);

        let combined: GrammarConfigSyntax =
            syn::parse_str("module = crate, arena_ast, flat").expect("flat and arena_ast combine");
        assert!(combined.flat);
        assert!(combined.arena_ast);

        assert_eq!(
            syn::parse_str::<GrammarConfigSyntax>("module = crate, flat, flat")
                .unwrap_err()
                .to_string(),
            "duplicate flat setting",
        );
    }

    #[test]
    fn unknown_grammar_settings_are_reported_at_the_key_before_parsing_a_value() {
        for setting in ["packed", "packed = true", "packed(option)"] {
            let source = format!("module = crate, {setting},");
            let error = syn::parse_str::<GrammarConfigSyntax>(&source).unwrap_err();
            assert_eq!(error.to_string(), "unsupported grammar setting `packed`");
            let range = super::span_range(error.span());
            assert_eq!(&source[range.start as usize..range.end as usize], "packed");
        }
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
        let mut configurations = None;
        let mut diagrams = false;
        let mut arena_ast = false;
        let mut flat = false;
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
            } else if key == "configurations" {
                reject_duplicate(&configurations, key.span(), "configurations")?;
                let content;
                syn::parenthesized!(content in input);
                configurations = Some(parse_configurations(&content)?);
            } else if key == "diagrams" {
                if diagrams {
                    return Err(syn::Error::new(key.span(), "duplicate diagrams setting"));
                }
                diagrams = true;
            } else if key == "arena_ast" {
                if arena_ast {
                    return Err(syn::Error::new(key.span(), "duplicate arena_ast setting"));
                }
                arena_ast = true;
            } else if key == "flat" {
                if flat {
                    return Err(syn::Error::new(key.span(), "duplicate flat setting"));
                }
                flat = true;
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
            } else if key == "module" {
                input.parse::<Token![=]>()?;
                reject_duplicate(&module, key.span(), "module")?;
                module = Some(Path::parse_mod_style(input)?);
            } else if key == "facade" {
                input.parse::<Token![=]>()?;
                reject_duplicate(&facade, key.span(), "facade")?;
                facade = Some(input.parse()?);
            } else if key == "keyword_matching" {
                input.parse::<Token![=]>()?;
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
                input.parse::<Token![=]>()?;
                let _value: LitInt = input.parse()?;
                return Err(syn::Error::new(
                    key.span(),
                    "max_lookahead has been removed; LR parsing does not use predictive lookahead depth",
                ));
            } else if key == "spans" {
                input.parse::<Token![=]>()?;
                reject_duplicate(&spans, key.span(), "spans")?;
                spans = Some(CaptureMode::parse(input.parse()?)?);
            } else if key == "parser_style" {
                input.parse::<Token![=]>()?;
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
            configurations: configurations.unwrap_or_default(),
            diagrams,
            arena_ast,
            flat,
            pretty: pretty.unwrap_or_default(),
            framing,
        })
    }
}

/// Resolves the declared configuration list against the build's Cargo features.
///
/// The enabled features must form a prefix of the list, because a configuration includes every
/// earlier one. The highest enabled entry is the configuration that the build states.
pub(crate) fn resolve_configuration_scale(
    declared: &[ConfigurationSyntax],
    cfg: &CargoCfgSnapshot,
) -> Result<ConfigurationScale, ConfigurationError> {
    let mut active = 0usize;
    let mut enabled = Vec::with_capacity(declared.len());
    for entry in declared {
        let on = match &entry.feature {
            None => true,
            Some(feature) => cfg
                .value("feature", &feature.value())
                .map_err(|failure| cfg_lookup_error(failure, feature.span(), "feature"))?,
        };
        enabled.push(on);
    }
    for (index, on) in enabled.iter().copied().enumerate() {
        if on {
            if index != active {
                return Err(ConfigurationError {
                    kind: ConfigurationErrorKind::Malformed,
                    range: declared[index].range.clone(),
                    message: format!(
                        "configuration `{}` is enabled while `{}` is not; each configuration \
                         feature must enable every earlier one",
                        declared[index].name, declared[active].name,
                    ),
                });
            }
            active = index + 1;
        }
    }
    Ok(ConfigurationScale {
        names: declared
            .iter()
            .map(|entry| entry.name.unraw().to_string())
            .collect(),
        active: active.saturating_sub(1),
    })
}

/// Reads the ordered `configurations(...)` list of the root grammar declaration.
///
/// The first entry is the baseline and takes no feature: every build admits it. Each later
/// entry names the Cargo feature that enables it, so the existing feature layout keeps working
/// without change.
fn parse_configurations(input: ParseStream<'_>) -> syn::Result<Vec<ConfigurationSyntax>> {
    let mut configurations = Vec::new();
    while !input.is_empty() {
        let name: syn::Ident = input.call(syn::Ident::parse_any)?;
        let range = span_range(name.span());
        let feature = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let key: syn::Ident = input.parse()?;
            if key != "feature" {
                return Err(syn::Error::new(
                    key.span(),
                    "a configuration selects a Cargo feature: write `name = feature(\"...\")`",
                ));
            }
            let content;
            syn::parenthesized!(content in input);
            Some(content.parse::<syn::LitStr>()?)
        } else {
            None
        };
        if configurations.is_empty() && feature.is_some() {
            return Err(syn::Error::new(
                name.span(),
                "the first configuration is the baseline and takes no feature",
            ));
        }
        if !configurations.is_empty() && feature.is_none() {
            return Err(syn::Error::new(
                name.span(),
                "every configuration after the first names its Cargo feature",
            ));
        }
        if configurations
            .iter()
            .any(|entry: &ConfigurationSyntax| entry.name == name)
        {
            return Err(syn::Error::new(
                name.span(),
                format!("duplicate configuration `{name}`"),
            ));
        }
        configurations.push(ConfigurationSyntax {
            name,
            feature,
            range,
        });
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
    }
    if configurations.len() < 2 {
        return Err(input.error("a configurations list needs a baseline and at least one more"));
    }
    Ok(configurations)
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
    } else if meta.path().is_ident("config") {
        // A `config` gate removes its element exactly as a `cfg` does. An active gate stays in
        // the meta list, because the source model reads it again to record the requirement.
        *active &= ConfigGate::from_meta(meta)?.admits(meta, cfg)?;
        if *active {
            output.push(meta.clone());
        }
    } else if meta.path().is_ident("config_attr") {
        let Meta::List(list) = meta else {
            return Err(configuration_error(
                meta.span(),
                "config_attr must use parentheses",
            ));
        };
        let nested = Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| configuration_error(error.span(), error.to_string()))?;
        let bounds = nested
            .iter()
            .take_while(|entry| matches!(entry, Meta::NameValue(_)))
            .count();
        if bounds == 0 {
            return Err(configuration_error(
                meta.span(),
                "config_attr requires a `since` or `before` bound",
            ));
        }
        let predicate = Meta::List(syn::MetaList {
            path: syn::parse_quote!(config),
            delimiter: syn::MacroDelimiter::Paren(syn::token::Paren(meta.span())),
            tokens: {
                let entries = nested.iter().take(bounds);
                quote::quote!(#(#entries),*)
            },
        });
        if ConfigGate::from_meta(&predicate)?.admits(meta, cfg)? {
            for nested in nested.iter().skip(bounds) {
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
