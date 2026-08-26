//! M4 tracer-bullet generation for one content-token Node.

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use quote::{ToTokens as _, format_ident, quote};
use syn::ext::IdentExt as _;
use syn::{GenericParam, Generics, Ident, Visibility};

use crate::TypeShape;
use crate::build_diagnostic::{BuildReport, DiagnosticStage, GenerateError};
use crate::decision_plan::FollowRecipe;
use crate::lexical::LexicalMatcher;
use crate::m3::{CargoOutput, M3Input, analyze_source_tree, internal_generate_error};
use crate::materialization::{ClosedType, MaterializedElement, MaterializedNodeBody};
use crate::source_syntax::CargoCfgSnapshot;

const PRIVATE_PROTOCOL: u32 = 1;
const EMITTER_IDENTITY: &str = "recursa-m4-one-node-v1";
const SELECTED_GENERATION: &str = "recursa-selected.rs";

/// One complete published logical output.
pub struct Generation {
    report: BuildReport,
}

impl Generation {
    /// Warnings retained on this successful Generation.
    pub const fn report(&self) -> &BuildReport {
        &self.report
    }
}

struct StdoutCargo;

impl CargoOutput for StdoutCargo {
    fn emit(&mut self, instruction: String) {
        println!("{instruction}");
    }
}

/// Analyzes, renders, stages, and atomically selects one complete Generation.
pub fn generate(root: impl AsRef<Path>) -> Result<Generation, GenerateError> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "CARGO_MANIFEST_DIR is unavailable"))?;
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| failure(root.as_ref(), "OUT_DIR is unavailable"))?;
    let cfg = CargoCfgSnapshot::from_cargo_environment(env::vars_os());
    let mut cargo = StdoutCargo;
    let success = analyze_source_tree(
        M3Input::new(
            &manifest_dir,
            root.as_ref(),
            crate::lowering::RustEdition::Rust2024,
            cfg,
        ),
        &mut cargo,
    )?;
    let plan =
        EmissionPlan::join(&success.analyzed).map_err(|detail| failure(root.as_ref(), &detail))?;
    let rendered = render(&plan).map_err(|detail| failure(root.as_ref(), &detail))?;
    publish(&out_dir, &plan.identity, &rendered)
        .map_err(|detail| failure(root.as_ref(), &detail))?;
    Ok(Generation {
        report: success.build_report,
    })
}

fn failure(selected_root: &Path, detail: &str) -> GenerateError {
    internal_generate_error(DiagnosticStage::Emission, detail, selected_root)
}

#[derive(Clone)]
struct EmissionPlan {
    identity: String,
    facade: Ident,
    module_anchor: syn::Path,
    node: NodeParsePlan,
    token: TokenParsePlan,
    lexical_rules: Vec<LexRulePlan>,
}

#[derive(Clone)]
struct NodeParsePlan {
    semantic_key: String,
    ident: Ident,
    loader_anchor: String,
    generics: Generics,
    field: Ident,
    follow: FollowRecipe,
    capture: PlannedCapturePolicy,
    field_capture: PlannedCapturePolicy,
}

#[derive(Clone)]
struct TokenParsePlan {
    ident: Ident,
    visibility: Visibility,
    lifetime: syn::Lifetime,
    kind: u16,
    expected_atom: u32,
    capture: PlannedCapturePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannedCapturePolicy {
    Optional,
}

impl PlannedCapturePolicy {
    const fn canonical_name(self) -> &'static str {
        match self {
            Self::Optional => "optional",
        }
    }

    fn render(self, facade: &Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Optional => quote!(#facade::__private::CapturePolicy::Optional),
        }
    }
}

#[derive(Clone)]
struct LexRulePlan {
    pattern: String,
    kind: u16,
    skip: bool,
}

impl EmissionPlan {
    fn join(analyzed: &crate::m3::AnalyzedGrammar) -> Result<Self, String> {
        let lowered = &analyzed.lowered;
        let resolved = lowered.source_modeled.resolved();
        if resolved.nodes.len() != 1 || lowered.applications.len() != 1 {
            return Err("the one-Node emitter requires exactly one materialized Node".to_owned());
        }
        if resolved.content_tokens.len() != 1 {
            return Err("the one-Node emitter requires exactly one content-token type".to_owned());
        }
        let node = &resolved.nodes[0];
        let application = &lowered.applications[0];
        let MaterializedNodeBody::Struct(elements) = &application.body else {
            return Err("the one-Node emitter requires a source struct".to_owned());
        };
        let [MaterializedElement::SemanticField { field, ty, .. }] = elements.as_slice() else {
            return Err("the one-Node emitter requires one semantic field".to_owned());
        };
        if !matches!(ty, ClosedType::ContentToken(_)) {
            return Err("the one-Node field must be a direct content token".to_owned());
        }
        let resolved_field = node
            .fields
            .iter()
            .find(|candidate| candidate.id == *field)
            .ok_or_else(|| "the materialized field lacks a source correlation".to_owned())?;
        let field_ident = resolved_field
            .syntax
            .ident
            .as_ref()
            .ok_or_else(|| "the one-Node emitter requires a named field".to_owned())?;
        let lifetime = node
            .generics
            .params
            .iter()
            .find_map(|parameter| match parameter {
                GenericParam::Lifetime(parameter) => Some(parameter.lifetime.clone()),
                GenericParam::Type(_) | GenericParam::Const(_) => None,
            })
            .ok_or_else(|| "the content-bearing Node requires an input lifetime".to_owned())?;

        let content = &resolved.content_tokens[0];
        let site = content
            .sites
            .first()
            .ok_or_else(|| "the content-token type has no source binding".to_owned())?;
        let target = lowered
            .lexical_plan
            .parser_targets
            .iter()
            .find(|target| target.semantic == content.id)
            .ok_or_else(|| "the content-token type lacks a lexical target plan".to_owned())?;
        let kind = u16::try_from(target.base_kind.index())
            .map_err(|_| "generated token kind exceeds the runtime width".to_owned())?;

        let content_type = lowered
            .lowering_map
            .content_targets
            .iter()
            .find(|correlation| correlation.semantic == content.id)
            .ok_or_else(|| "the content token lacks an M2 type correlation".to_owned())?
            .lowered;
        let TypeShape::Token { atom, .. } = &lowered.grammar.types[content_type.index()].shape
        else {
            return Err("the content target did not lower to an M2 token type".to_owned());
        };
        let expected_atom = atom
            .index()
            .try_into()
            .map_err(|_| "generated expectation exceeds the runtime width".to_owned())?;

        let application_type = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == application.id)
            .ok_or_else(|| "the Node lacks an M2 type correlation".to_owned())?
            .lowered;
        let TypeShape::Sequence { sites } = &lowered.grammar.types[application_type.index()].shape
        else {
            return Err("the one-Node source struct lacks a frozen sequence plan".to_owned());
        };
        let [parse_site] = sites.as_slice() else {
            return Err("the one-Node sequence plan must contain one parse site".to_owned());
        };
        let output = analyzed
            .analysis
            .output
            .as_ref()
            .ok_or_else(|| "successful M3 output lacks M2 decisions".to_owned())?;
        let follow = output
            .decisions()
            .follow_recipes_by_site()
            .get(parse_site.index())
            .cloned()
            .ok_or_else(|| "the content field lacks a frozen FOLLOW recipe".to_owned())?;
        if !matches!(follow, FollowRecipe::Caller) {
            return Err("the one-Node tracer bullet requires a caller FOLLOW recipe".to_owned());
        }

        let mut lexical_rules = Vec::new();
        for rule in &lowered.lexical_plan.rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => regex_syntax::escape(text),
            };
            lexical_rules.push(LexRulePlan {
                pattern,
                kind: rule
                    .kind
                    .index()
                    .try_into()
                    .map_err(|_| "generated token kind exceeds the runtime width".to_owned())?,
                skip: false,
            });
        }
        for rule in &lowered.lexical_plan.non_token_rules {
            let pattern = match &rule.matcher {
                LexicalMatcher::Regex(pattern) => pattern.clone(),
                LexicalMatcher::Fixed(text) => regex_syntax::escape(text),
            };
            lexical_rules.push(LexRulePlan {
                pattern,
                kind,
                skip: true,
            });
        }
        let pattern = lowered
            .lexical_plan
            .content_bases
            .iter()
            .find(|base| base.kind == target.base_kind)
            .map(|base| base.pattern.clone())
            .ok_or_else(|| "the content token lacks a frozen lexical pattern".to_owned())?;
        let module_anchor = emitted_crate_path(&resolved.build_inputs.module_anchor.segments)?;
        let facade = emitted_ident(&resolved.build_inputs.facade.ident)?;
        let source = &resolved.tree.files[node.source.index()];
        let physical_source = normalized_source_identity(&source.path)?;
        let (line, column) = source_line_column(&source.text, node.range.start)?;
        let loader_anchor = declaration_loader_anchor(&physical_source, line, column, &node.ident);
        let token = TokenParsePlan {
            ident: emitted_ident(&content.ident)?,
            visibility: site.visibility.clone(),
            lifetime,
            kind,
            expected_atom,
            capture: PlannedCapturePolicy::Optional,
        };
        let node = NodeParsePlan {
            semantic_key: node.key.segments.join("::"),
            ident: emitted_ident(&node.ident)?,
            loader_anchor,
            generics: node.generics.clone(),
            field: field_ident.clone(),
            follow,
            capture: PlannedCapturePolicy::Optional,
            field_capture: PlannedCapturePolicy::Optional,
        };
        let mut canonical = format!(
            "{EMITTER_IDENTITY};release={};protocol={PRIVATE_PROTOCOL};facade={facade};anchor={};node={};loader_anchor={};field={};node_capture={};field_capture={};token={};token_capture={};visibility={};lifetime={};kind={kind};atom={expected_atom};pattern={pattern:?};",
            env!("CARGO_PKG_VERSION"),
            resolved.build_inputs.module_anchor.segments.join("::"),
            node.semantic_key,
            node.loader_anchor,
            node.field,
            node.capture.canonical_name(),
            node.field_capture.canonical_name(),
            token.ident,
            token.capture.canonical_name(),
            token.visibility.to_token_stream(),
            token.lifetime,
        );
        canonical.push_str(&node.generics.to_token_stream().to_string());
        for rule in &lexical_rules {
            canonical.push_str(&format!(
                ";rule={:?}:{}:{}",
                rule.pattern, rule.kind, rule.skip
            ));
        }
        let identity = format!("g_{:016x}", stable_hash(canonical.as_bytes()));
        let plan = Self {
            identity,
            facade,
            module_anchor,
            node,
            token,
            lexical_rules,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn validate(&self) -> Result<(), String> {
        if self.identity.is_empty() || self.node.semantic_key.is_empty() {
            return Err("the Emission plan lacks a Generation or semantic Node key".to_owned());
        }
        if !matches!(self.node.follow, FollowRecipe::Caller) {
            return Err("the one-Node Parse plan lacks its caller FOLLOW recipe".to_owned());
        }
        if self.lexical_rules.is_empty()
            || !self
                .lexical_rules
                .iter()
                .any(|rule| !rule.skip && rule.kind == self.token.kind)
        {
            return Err("the content token lacks its required lexical rule".to_owned());
        }
        Ok(())
    }
}

struct RenderedGeneration {
    root: String,
    node: String,
    selector: String,
}

fn render(plan: &EmissionPlan) -> Result<RenderedGeneration, String> {
    plan.validate()?;
    let root = render_root(plan).to_string();
    let node = render_node(plan).to_string();
    let selected_root = format!("/recursa-generations/{}/root.rs", plan.identity);
    let selector = quote! {
        include!(concat!(env!("OUT_DIR"), #selected_root));
    }
    .to_string();
    syn::parse_file(&root).map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    syn::parse_file(&node).map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
    syn::parse_file(&selector)
        .map_err(|error| format!("Generation selector rendering is invalid Rust: {error}"))?;
    Ok(RenderedGeneration {
        root,
        node,
        selector,
    })
}

fn render_root(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let release_text = env!("CARGO_PKG_VERSION");
    let release = format!("release_{release_text}").replace(['.', '-'], "_");
    let release = format_ident!("{release}");
    let protocol = format_ident!("protocol_{PRIVATE_PROTOCOL}");
    let node_ident = &plan.node.ident;
    let token_ident = &plan.token.ident;
    let token_view = sibling_ident(token_ident, "View");
    let token_visibility = &plan.token.visibility;
    let input_lifetime = &plan.token.lifetime;
    let kind = plan.token.kind;
    let expected_atom = plan.token.expected_atom;
    let kind_count = plan
        .lexical_rules
        .iter()
        .map(|rule| usize::from(rule.kind))
        .max()
        .unwrap_or(0)
        + 2;
    let lexical_error_kind = kind_count - 1;
    let kind_names = (0..kind_count)
        .map(|index| {
            if index == usize::from(kind) {
                token_ident.to_string()
            } else if index == lexical_error_kind {
                "lexical error".to_owned()
            } else {
                format!("token {index}")
            }
        })
        .collect::<Vec<_>>();
    let patterns = plan.lexical_rules.iter().map(|rule| &rule.pattern);
    let kinds = plan.lexical_rules.iter().map(|rule| rule.kind);
    let skips = plan.lexical_rules.iter().map(|rule| rule.skip);
    let key = &plan.node.semantic_key;
    let loader_anchor = syn::LitStr::new(&plan.node.loader_anchor, proc_macro2::Span::call_site());
    let token_capture = plan.token.capture.render(facade);
    let node_path = format!("/recursa-generations/{}/node.rs", plan.identity);

    quote! {
        const _: #facade::__private::FacadeCompatibility<
            { #facade::__private::release_stamp(#release_text) },
            #PRIVATE_PROTOCOL,
        > = #facade::__private::FACADE_COMPATIBILITY;

        #[doc(hidden)]
        pub(crate) struct __RecursaGenerationAnchor;
        const _: fn() = || {
            let _: #module_anchor::__RecursaGenerationAnchor = __RecursaGenerationAnchor;
        };

        static __RECURSA_GRAMMAR: #facade::__private::GrammarInfo =
            #facade::__private::grammar_info(
                #key,
                &[#(#kind_names),*],
                &[stringify!(#token_ident)],
                #lexical_error_kind as u16,
            );
        static __RECURSA_EMPTY_FOLLOW: #facade::__private::FollowSet =
            #facade::__private::follow_set(#kind_count as u32, &[0], &[]);
        static __RECURSA_TOKEN_START: #facade::__private::FollowSet =
            #facade::__private::follow_set(
                #kind_count as u32,
                &[1_u64 << #kind],
                &[#facade::__private::ExpectedAtomId(#expected_atom)],
            );
        static __RECURSA_LEX_RULES: &[#facade::__private::LexRule] = &[
            #(
                #facade::__private::LexRule {
                    pattern: #patterns,
                    kind: #kinds,
                    skip: #skips,
                }
            ),*
        ];

        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum TokenKind {
            #token_ident,
            LexicalError,
        }

        /// Read-only metadata for this generated grammar.
        pub struct GrammarInfo {
            _private: (),
        }

        impl GrammarInfo {
            pub fn name(&self) -> &'static str {
                __RECURSA_GRAMMAR.name()
            }
        }

        static __RECURSA_PUBLIC_GRAMMAR_INFO: GrammarInfo = GrammarInfo { _private: () };

        pub fn grammar_info() -> &'static GrammarInfo {
            &__RECURSA_PUBLIC_GRAMMAR_INFO
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct Token<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> Token<'input> {
            pub fn kind(self) -> TokenKind {
                match self.record.kind {
                    #kind => TokenKind::#token_ident,
                    _ => TokenKind::LexicalError,
                }
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("generated lexer produced a valid UTF-8 span")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }
        }

        pub struct Tokens<'lexed, 'input> {
            source: &'input str,
            records: ::core::slice::Iter<'lexed, #facade::__private::TokenRecord>,
        }

        impl<'input> Iterator for Tokens<'_, 'input> {
            type Item = Token<'input>;

            fn next(&mut self) -> Option<Self::Item> {
                self.records.next().copied().map(|record| Token {
                    source: self.source,
                    record,
                })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                self.records.size_hint()
            }
        }

        impl ExactSizeIterator for Tokens<'_, '_> {}

        /// A cursor bound to this generated grammar's token identities.
        pub struct Input<'input, A = ()> {
            inner: #facade::__private::Input<'input, A>,
        }

        impl<'input, A> Input<'input, A> {
            pub fn source(&self) -> &'input str {
                self.inner.source()
            }

            pub fn builder(&mut self) -> &mut A {
                self.inner.builder()
            }

            pub fn cursor(&self) -> usize {
                self.inner.cursor()
            }

            pub fn is_eof(&self) -> bool {
                self.inner.is_eof()
            }
        }

        pub struct LexResult<'input> {
            inner: #facade::__private::LexResult<'input>,
        }

        impl<'input> LexResult<'input> {
            pub fn tokens(&self) -> Tokens<'_, 'input> {
                Tokens {
                    source: #facade::__private::lex_source(&self.inner),
                    records: #facade::__private::lex_records(&self.inner).iter(),
                }
            }

            pub fn source(&self) -> &'input str {
                #facade::__private::lex_source(&self.inner)
            }

            pub fn input(&self) -> Input<'_> {
                Input {
                    inner: self.inner.input(&__RECURSA_GRAMMAR),
                }
            }

            pub fn input_with_builder<A>(&self, builder: A) -> Input<'_, A> {
                Input {
                    inner: self
                        .inner
                        .input_with_builder(&__RECURSA_GRAMMAR, builder),
                }
            }
        }

        pub fn lex(source: &str) -> LexResult<'_> {
            LexResult {
                inner: #facade::__private::lex(
                    source,
                    __RECURSA_LEX_RULES,
                    #lexical_error_kind as u16,
                ),
            }
        }

        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #token_visibility struct #token_ident<#input_lifetime> {
            text: #facade::__private::ContentText<#input_lifetime>,
        }

        impl<#input_lifetime> #token_ident<#input_lifetime> {
            pub fn new(text: impl Into<Box<str>>) -> Self {
                Self {
                    text: #facade::__private::ContentText::owned(text.into()),
                }
            }

            pub fn text(&self) -> &str {
                self.text.as_str()
            }

            pub const fn kind(&self) -> TokenKind {
                TokenKind::#token_ident
            }
        }

        #[derive(Clone, Copy)]
        #token_visibility struct #token_view<'view, #input_lifetime>
        where
            #input_lifetime: 'view,
        {
            value: &'view #token_ident<#input_lifetime>,
            cursor: #facade::__private::ViewCursor<'view>,
        }

        impl<#input_lifetime> #facade::__private::HasSpanSealed
            for #token_view<'_, #input_lifetime>
        {}

        impl<#input_lifetime> #facade::HasSpan for #token_view<'_, #input_lifetime> {
            fn span(&self) -> Option<#facade::Span> {
                self.cursor.span()
            }
        }

        impl<#input_lifetime> #facade::NodeView for #token_view<'_, #input_lifetime> {
            type Value = #token_ident<#input_lifetime>;

            fn value(&self) -> &Self::Value {
                self.value
            }
        }

        impl<#input_lifetime> #facade::__private::NodeViewFactory for #token_ident<#input_lifetime> {
            type View<'view> = #token_view<'view, #input_lifetime>
            where
                Self: 'view;

            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> Self::View<'view> {
                #token_view { value: self, cursor }
            }
        }

        impl<#input_lifetime, A> #facade::__private::Parse<#input_lifetime, A>
            for #token_ident<#input_lifetime>
        {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy =
                #token_capture;

            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &__RECURSA_GRAMMAR
            }

            fn root_follow() -> &'static #facade::__private::FollowSet {
                &__RECURSA_EMPTY_FOLLOW
            }

            fn parse(
                context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                let Some(record) = context.peek(0) else {
                    return Err(context.unexpected(&__RECURSA_TOKEN_START));
                };
                if record.kind != #kind {
                    return Err(context.unexpected(&__RECURSA_TOKEN_START));
                }
                context.advance();
                let text = context.matched_text(record)?;
                context.finish(Self { text }, record.span)
            }
        }

        impl<#input_lifetime, A>
            #facade::__private::ParseSealed<Input<#input_lifetime, A>>
            for #token_ident<#input_lifetime>
        {}

        impl<#input_lifetime, A> #facade::Parse<Input<#input_lifetime, A>>
            for #token_ident<#input_lifetime>
        {
            type Output = #facade::Parsed<#input_lifetime, Self>;

            fn parse(
                input: &mut Input<#input_lifetime, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }

        macro_rules! __recursa_root_handshake {
            (#release, #protocol, core) => {};
            ($other_release:ident, $other_protocol:ident, $other_capability:ident) => {
                compile_error!("error[RCA9003]: Recursa release/private-protocol mismatch");
            };
        }

        macro_rules! __recursa_generation_handshake {
            (#generation) => {};
            ($other:ident) => {
                compile_error!("error[RCA9004]: root and Node selected different Generations");
            };
        }

        macro_rules! __recursa_load_node {
            (#node_ident, #loader_anchor) => {
                include!(concat!(env!("OUT_DIR"), #node_path));
            };
            ($other:ident, $other_anchor:literal) => {
                compile_error!("error[RCA9002]: selected Generation has no body for this Node");
            };
        }
    }
}

fn render_node(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let generation = format_ident!("{}", plan.identity);
    let node_ident = &plan.node.ident;
    let field = &plan.node.field;
    let token_ident = &plan.token.ident;
    let token_view = sibling_ident(token_ident, "View");
    let view_ident = sibling_ident(node_ident, "View");
    let generics = &plan.node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let mut parse_generics = generics.clone();
    parse_generics.params.push(syn::parse_quote!(A));
    let (parse_impl_generics, _, parse_where_clause) = parse_generics.split_for_impl();
    let input_lifetime = &plan.token.lifetime;
    let node_capture = plan.node.capture.render(facade);
    let field_capture = plan.node.field_capture.render(facade);
    debug_assert!(matches!(plan.node.follow, FollowRecipe::Caller));

    quote! {
        __recursa_generation_handshake!(#generation);

        #[derive(Clone, Copy)]
        pub struct #view_ident<'view, #input_lifetime>
        where
            #input_lifetime: 'view,
        {
            value: &'view #node_ident<#input_lifetime>,
            cursor: #facade::__private::ViewCursor<'view>,
        }

        impl<'view, #input_lifetime> #view_ident<'view, #input_lifetime> {
            pub fn #field(&self) -> #token_view<'view, #input_lifetime> {
                #facade::__private::NodeViewFactory::view(
                    &self.value.#field,
                    self.cursor.child(0),
                )
            }
        }

        impl<#input_lifetime> #facade::__private::HasSpanSealed
            for #view_ident<'_, #input_lifetime>
        {}

        impl<#input_lifetime> #facade::HasSpan for #view_ident<'_, #input_lifetime> {
            fn span(&self) -> Option<#facade::Span> {
                self.cursor.span()
            }
        }

        impl<#input_lifetime> #facade::NodeView for #view_ident<'_, #input_lifetime> {
            type Value = #node_ident<#input_lifetime>;

            fn value(&self) -> &Self::Value {
                self.value
            }
        }

        impl #impl_generics #facade::__private::NodeViewFactory for #node_ident #type_generics
            #where_clause
        {
            type View<'view> = #view_ident<'view, #input_lifetime>
            where
                Self: 'view;

            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> Self::View<'view> {
                #view_ident { value: self, cursor }
            }
        }

        impl #parse_impl_generics #facade::__private::Parse<#input_lifetime, A>
            for #node_ident #type_generics #parse_where_clause
        {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy =
                #node_capture;

            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &__RECURSA_GRAMMAR
            }

            fn root_follow() -> &'static #facade::__private::FollowSet {
                &__RECURSA_EMPTY_FOLLOW
            }

            fn parse(
                context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                let record = context.peek(0);
                let caller_follow = context.follow().clone();
                let (#field, occurrence) = context
                    .parse_node::<#token_ident<#input_lifetime>>(
                        &caller_follow,
                        #field_capture,
                    )?
                    .into_parts();
                let span = record
                    .expect("a successful generated token parse consumed its first record")
                    .span;
                context.finish_with_children(Self { #field }, span, [occurrence])
            }
        }

        impl #parse_impl_generics
            #facade::__private::ParseSealed<Input<#input_lifetime, A>>
            for #node_ident #type_generics #parse_where_clause
        {}

        impl #parse_impl_generics #facade::Parse<Input<#input_lifetime, A>>
            for #node_ident #type_generics #parse_where_clause
        {
            type Output = #facade::Parsed<#input_lifetime, Self>;

            fn parse(
                input: &mut Input<#input_lifetime, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }

        impl #impl_generics #node_ident #type_generics #where_clause {
            pub fn parse<A>(
                input: &mut Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError> {
                <Self as #facade::Parse<Input<#input_lifetime, A>>>::parse(input)
            }

            pub fn parse_spanned<A>(
                input: &mut Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError> {
                #facade::__private::parse_spanned(&mut input.inner)
            }
        }
    }
}

fn publish(out_dir: &Path, identity: &str, rendered: &RenderedGeneration) -> Result<(), String> {
    let generations = out_dir.join("recursa-generations");
    fs::create_dir_all(&generations)
        .map_err(|error| format!("cannot create Generation directory: {error}"))?;
    let published = generations.join(identity);
    if published.exists() {
        if !published.join("root.rs").is_file() || !published.join("node.rs").is_file() {
            return Err("an existing published Generation is incomplete".to_owned());
        }
    } else {
        let nonce = staging_nonce()?;
        let staging = generations.join(format!(
            ".staging-{}-{nonce}-{identity}",
            std::process::id()
        ));
        fs::create_dir(&staging)
            .map_err(|error| format!("cannot create Generation staging directory: {error}"))?;
        stage_and_publish(&staging, &published, rendered, || Ok(()))?;
    }
    select_generation(out_dir, rendered)
}

fn stage_and_publish(
    staging: &Path,
    published: &Path,
    rendered: &RenderedGeneration,
    after_root: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    fs::write(staging.join("root.rs"), &rendered.root)
        .map_err(|error| format!("cannot stage generated root: {error}"))?;
    after_root()?;
    fs::write(staging.join("node.rs"), &rendered.node)
        .map_err(|error| format!("cannot stage generated Node: {error}"))?;
    fs::rename(staging, published)
        .map_err(|error| format!("cannot cross the Generation publication barrier: {error}"))?;
    Ok(())
}

fn select_generation(out_dir: &Path, rendered: &RenderedGeneration) -> Result<(), String> {
    let nonce = staging_nonce()?;
    let staging = out_dir.join(format!(
        ".recursa-selector-{}-{nonce}.rs",
        std::process::id()
    ));
    fs::write(&staging, &rendered.selector)
        .map_err(|error| format!("cannot stage the Generation selector: {error}"))?;
    fs::rename(&staging, out_dir.join(SELECTED_GENERATION))
        .map_err(|error| format!("cannot atomically select the complete Generation: {error}"))
}

fn staging_nonce() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock cannot name staging output: {error}"))
        .map(|duration| duration.as_nanos())
}

fn emitted_ident(name: &str) -> Result<Ident, String> {
    syn::parse_str::<Ident>(name)
        .or_else(|_| syn::parse_str::<Ident>(&format!("r#{name}")))
        .map_err(|error| format!("cannot emit Rust identifier `{name}`: {error}"))
}

fn emitted_crate_path(segments: &[String]) -> Result<syn::Path, String> {
    let mut path: syn::Path = syn::parse_quote!(crate);
    for segment in segments {
        path.segments
            .push(syn::PathSegment::from(emitted_ident(segment)?));
    }
    Ok(path)
}

fn sibling_ident(ident: &Ident, suffix: &str) -> Ident {
    Ident::new(
        &format!("{}{suffix}", ident.unraw()),
        proc_macro2::Span::call_site(),
    )
}

fn source_line_column(source: &str, byte_offset: u32) -> Result<(usize, usize), String> {
    let prefix = source
        .get(..byte_offset as usize)
        .ok_or_else(|| "the Node item position is not a UTF-8 source boundary".to_owned())?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    Ok((line, column))
}

fn normalized_source_identity(path: &Path) -> Result<String, String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => segments.push(
                segment
                    .to_str()
                    .ok_or_else(|| "the Node source path is not valid UTF-8".to_owned())?,
            ),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("the Node source identity is not manifest-relative".to_owned());
            }
        }
    }
    if segments.is_empty() {
        return Err("the Node source identity is empty".to_owned());
    }
    Ok(segments.join("/"))
}

fn declaration_loader_anchor(file: &str, line: usize, column: usize, name: &str) -> String {
    let input = format!("{file}:{line}:{column}:{name}");
    format!("{:016x}", loader_anchor_hash(input.as_bytes()))
}

fn loader_anchor_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf_29ce_4842_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_plan() -> EmissionPlan {
        EmissionPlan {
            identity: "g_fixture".to_owned(),
            facade: syn::parse_quote!(recursa),
            module_anchor: syn::parse_quote!(crate::grammar),
            node: NodeParsePlan {
                semantic_key: "grammar::Word".to_owned(),
                ident: syn::parse_quote!(Word),
                loader_anchor: "0123456789abcdef".to_owned(),
                generics: syn::parse_quote!(<'input>),
                field: syn::parse_quote!(text),
                follow: FollowRecipe::Caller,
                capture: PlannedCapturePolicy::Optional,
                field_capture: PlannedCapturePolicy::Optional,
            },
            token: TokenParsePlan {
                ident: syn::parse_quote!(WordText),
                visibility: syn::parse_quote!(pub),
                lifetime: syn::parse_quote!('input),
                kind: 0,
                expected_atom: 0,
                capture: PlannedCapturePolicy::Optional,
            },
            lexical_rules: vec![LexRulePlan {
                pattern: "[A-Za-z]+".to_owned(),
                kind: 0,
                skip: false,
            }],
        }
    }

    #[test]
    fn corrupt_required_parse_or_lexical_plan_facts_fail_before_rendering() {
        let mut missing_lexical_rule = fixture_plan();
        missing_lexical_rule.lexical_rules.clear();
        let Err(error) = render(&missing_lexical_rule) else {
            panic!("a missing lexical rule must fail before rendering");
        };
        assert_eq!(error, "the content token lacks its required lexical rule");

        let mut corrupt_follow = fixture_plan();
        corrupt_follow.node.follow = FollowRecipe::Static { first: Vec::new() };
        let Err(error) = render(&corrupt_follow) else {
            panic!("a corrupt FOLLOW recipe must fail before rendering");
        };
        assert_eq!(
            error,
            "the one-Node Parse plan lacks its caller FOLLOW recipe"
        );
    }

    #[test]
    fn emission_failures_use_the_registered_m4_code_and_preserve_detail() {
        let error = failure(
            Path::new("src/grammar.rs"),
            "the frozen emission plan is incomplete",
        );
        let [diagnostic] = error.report().diagnostics() else {
            panic!("one emission invariant failure must produce one diagnostic");
        };

        assert_eq!(diagnostic.code().as_str(), "RCA9105");
        assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);
        assert!(matches!(
            diagnostic.evidence(),
            crate::DiagnosticEvidence::InternalInvariant { producer, detail }
                if producer == "Emission" && detail == "the frozen emission plan is incomplete"
        ));
    }

    #[test]
    fn incomplete_staging_is_unselectable_and_a_retry_publishes_one_complete_generation() {
        let output = tempfile::tempdir().unwrap();
        let generations = output.path().join("recursa-generations");
        fs::create_dir(&generations).unwrap();
        let staging = generations.join(".staging-fixture");
        let published = generations.join("g_fixture");
        fs::create_dir(&staging).unwrap();
        let rendered = render(&fixture_plan()).unwrap();

        assert_eq!(
            stage_and_publish(&staging, &published, &rendered, || {
                Err("injected staging interruption".to_owned())
            })
            .unwrap_err(),
            "injected staging interruption",
        );
        assert!(!published.exists());
        assert!(staging.join("root.rs").is_file());
        assert!(!staging.join("node.rs").exists());
        assert!(!output.path().join(SELECTED_GENERATION).exists());

        publish(output.path(), "g_fixture", &rendered).unwrap();
        assert!(published.join("root.rs").is_file());
        assert!(published.join("node.rs").is_file());
        assert!(output.path().join(SELECTED_GENERATION).is_file());
    }
}
