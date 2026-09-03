//! Structural loaders for matching-version Recursa generated output.

use proc_macro::TokenStream;
use quote::quote;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use syn::ext::IdentExt as _;
use syn::parse::Parse as _;
use syn::visit_mut::VisitMut as _;
use syn::{DeriveInput, Type, parse_macro_input};

/// Matching-version protocol expected by generated root and Node loaders.
const PRIVATE_PROTOCOL: u32 = 14;

/// Marks a source declaration as a grammar Node and loads its generated body.
///
/// Fixed syntax belongs in one `#[tok(...)]` sequence. Inside that sequence,
/// `optional(TOKENS...)` denotes an erased atomic group: the complete fixed-token sequence is
/// consumed when present, while an incomplete prefix consumes nothing. A plain `bool` field may
/// separately use `#[presence(TOKENS...)]` to store whether one non-empty fixed-token sequence
/// occurred. Its ordinary `#[tok(...)]` attachment remains independent and may place required or
/// erased-optional syntax around `this`.
///
/// The derive deliberately interprets no fields or grammar helper attributes.
#[proc_macro_derive(
    Node,
    attributes(lex, node, tok, presence, sep, parse, pratt, spanned, pretty)
)]
pub fn node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;
    let span = ident.span();
    let start = span.start();
    let file = span
        .local_file()
        .and_then(|path| path.canonicalize().ok())
        .unwrap_or_else(|| Path::new(&span.file()).to_path_buf());
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    let relative_file = manifest
        .as_deref()
        .and_then(|manifest| file.strip_prefix(manifest).ok())
        .unwrap_or(&file);
    let source_identity = normalized_source_identity(relative_file);
    let anchor = format!(
        "{}:{}:{}:{}",
        source_identity,
        start.line,
        start.column + 1,
        ident.unraw(),
    );
    let loader_anchor_text = format!("{:016x}", stable_hash(anchor.as_bytes()));
    let loader_anchor = syn::LitStr::new(&loader_anchor_text, proc_macro2::Span::call_site());
    match selected_node(&loader_anchor_text) {
        LoaderStatus::Ready => quote! {
            __recursa_load_node!(#ident, #loader_anchor);
        }
        .into(),
        LoaderStatus::RootUnavailable => TokenStream::new(),
        LoaderStatus::NodeUnavailable => quote! {
            compile_error!("error[RCA9002]: selected Generation has no readable body for this Node");
        }
        .into(),
    }
}

/// Loads the complete Generation selected by the build script.
///
/// Only `recursa-codegen` interprets this macro's grammar configuration.
#[proc_macro]
pub fn grammar(_input: TokenStream) -> TokenStream {
    if selected_output().is_none() {
        return quote! {
            compile_error!("error[RCA9001]: selected Generation has no readable root output");
        }
        .into();
    }
    let release = format!("release_{}", env!("CARGO_PKG_VERSION")).replace(['.', '-'], "_");
    let release = syn::Ident::new(&release, proc_macro2::Span::call_site());
    let protocol = syn::Ident::new(
        &format!("protocol_{PRIVATE_PROTOCOL}"),
        proc_macro2::Span::call_site(),
    );
    let capabilities = syn::Ident::new(
        &format!(
            "cap_core_d{}_e{}_a{}_s{}",
            u8::from(cfg!(feature = "diagrams")),
            u8::from(cfg!(feature = "explorer")),
            u8::from(cfg!(feature = "arbitrary")),
            u8::from(cfg!(feature = "serde")),
        ),
        proc_macro2::Span::call_site(),
    );
    let arbitrary_gate = if cfg!(feature = "arbitrary") {
        quote! {
            macro_rules! __recursa_if_arbitrary {
                ($($tokens:tt)*) => { $($tokens)* };
            }
        }
    } else {
        quote! {
            macro_rules! __recursa_if_arbitrary {
                ($($tokens:tt)*) => {};
            }
        }
    };
    let serde_gate = if cfg!(feature = "serde") {
        quote! {
            macro_rules! __recursa_if_serde {
                ($($tokens:tt)*) => { $($tokens)* };
            }
        }
    } else {
        quote! {
            macro_rules! __recursa_if_serde {
                ($($tokens:tt)*) => {};
            }
        }
    };
    let explorer_gate = if cfg!(feature = "explorer") {
        quote! {
            macro_rules! __recursa_if_explorer {
                ($($tokens:tt)*) => { $($tokens)* };
            }
        }
    } else {
        quote! {
            macro_rules! __recursa_if_explorer {
                ($($tokens:tt)*) => {};
            }
        }
    };
    let explorer_sample = if cfg!(feature = "explorer") && cfg!(feature = "arbitrary") {
        quote! {
            macro_rules! __recursa_explorer_sample {
                ($sample:path) => { (true, Some($sample)) };
            }
        }
    } else {
        quote! {
            macro_rules! __recursa_explorer_sample {
                ($sample:path) => { (false, None) };
            }
        }
    };
    quote! {
        #arbitrary_gate
        #serde_gate
        #explorer_gate
        #explorer_sample
        include!(concat!(env!("OUT_DIR"), "/recursa-selected.rs"));
        __recursa_root_handshake!(#release, #protocol, #capabilities);
    }
    .into()
}

/// Files selected for the active successful Generation.
struct SelectedOutput {
    /// Directory containing the immutable Generation payload.
    directory: PathBuf,
    /// Loader anchors mapped to declaration-local generated source files.
    loader_files: Option<BTreeMap<String, String>>,
}

/// Availability of selected generated source at a Node derive site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoaderStatus {
    /// Both the root output and requested Node body are readable.
    Ready,
    /// No valid selected Generation root could be loaded.
    RootUnavailable,
    /// The Generation exists but has no readable body for this Node anchor.
    NodeUnavailable,
}

/// Loads and caches the selected Generation for this compiler output directory.
fn selected_output() -> Option<Arc<SelectedOutput>> {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR")?);
    static SELECTED: OnceLock<Mutex<BTreeMap<PathBuf, Option<Arc<SelectedOutput>>>>> =
        OnceLock::new();
    let selected = SELECTED.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut selected = selected.lock().ok()?;
    if let Some(cached) = selected.get(&out_dir) {
        return cached.clone();
    }
    let loaded = load_selected_output(&out_dir).map(Arc::new);
    selected.insert(out_dir, loaded.clone());
    loaded
}

/// Validates the selector and loads its immutable root and Node loader map.
fn load_selected_output(out_dir: &Path) -> Option<SelectedOutput> {
    let selector = fs::read_to_string(out_dir.join("recursa-selected.rs")).ok()?;
    let identity = selector
        .lines()
        .next()?
        .strip_prefix("// recursa-generation:")?;
    if !valid_generation_identity(identity) {
        return None;
    }
    let directory = out_dir.join("recursa-generations").join(identity);
    fs::read_to_string(directory.join("root.rs")).ok()?;
    let loader_files = fs::read_to_string(directory.join("loader-map"))
        .ok()
        .and_then(|loader_map| {
            loader_map
                .lines()
                .map(|line| {
                    let (anchor, name) = line.split_once('\t')?;
                    valid_node_file(name).then(|| (anchor.to_owned(), name.to_owned()))
                })
                .collect()
        });
    Some(SelectedOutput {
        directory,
        loader_files,
    })
}

/// Reports whether a generated Node body is available for one loader anchor.
fn selected_node(anchor: &str) -> LoaderStatus {
    let Some(selected) = selected_output() else {
        return LoaderStatus::RootUnavailable;
    };
    let Some(loader_files) = &selected.loader_files else {
        return LoaderStatus::NodeUnavailable;
    };
    let Some(name) = loader_files.get(anchor) else {
        return LoaderStatus::NodeUnavailable;
    };
    if !valid_node_file(name) || fs::read_to_string(selected.directory.join(name)).is_err() {
        return LoaderStatus::NodeUnavailable;
    }
    LoaderStatus::Ready
}

/// Checks the closed, path-safe syntax of a Generation directory identity.
fn valid_generation_identity(identity: &str) -> bool {
    identity
        .strip_prefix("g_")
        .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Checks the closed, path-safe syntax of a declaration-local Node filename.
fn valid_node_file(name: &str) -> bool {
    name.strip_prefix("node-").is_some_and(|suffix| {
        suffix.strip_suffix(".rs").is_some_and(|index| {
            !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}

/// Attaches doc-time railroad documentation to one generated item.
///
/// Generated output carries this attribute only under `cfg(doc)`, so ordinary builds strip it
/// before expansion and never render or embed diagram markup. While `cargo doc` expands it, the
/// attribute decodes the diagram plan published by the selected Generation, renders the requested
/// Node, content-token, or aggregate document, writes the standalone SVG next to the plan, and
/// prepends the Rustdoc document to the item. Arguments are `aggregate`, `node = INDEX`, or
/// `token = INDEX` in canonical Generation order.
#[cfg(feature = "diagrams")]
#[proc_macro_attribute]
pub fn __recursa_diagram_doc(arguments: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let documentation = match diagram_documentation(arguments.into()) {
        Ok(documentation) => documentation,
        Err(error) => {
            let error = error.into_compile_error();
            return quote!(#error #item).into();
        }
    };
    quote!(#[doc = #documentation] #item).into()
}

/// Which published diagram one generated attribute requests.
#[cfg(feature = "diagrams")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagramRequest {
    /// The aggregate grammar document listing every diagram.
    Aggregate,
    /// One Node diagram by canonical parse-plan index.
    Node(usize),
    /// One content-token diagram by canonical token index.
    Token(usize),
}

/// Parses the closed argument grammar of [`__recursa_diagram_doc`].
#[cfg(feature = "diagrams")]
fn parse_diagram_request(arguments: proc_macro2::TokenStream) -> syn::Result<DiagramRequest> {
    let span = proc_macro2::Span::call_site();
    let error = || {
        syn::Error::new(
            span,
            "expected `aggregate`, `node = INDEX`, or `token = INDEX`",
        )
    };
    let mut tokens = arguments.into_iter();
    let keyword = match tokens.next() {
        Some(proc_macro2::TokenTree::Ident(keyword)) => keyword.to_string(),
        _ => return Err(error()),
    };
    if keyword == "aggregate" {
        return tokens
            .next()
            .map_or(Ok(DiagramRequest::Aggregate), |_| Err(error()));
    }
    match tokens.next() {
        Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '=' => {}
        _ => return Err(error()),
    }
    let index = match tokens.next() {
        Some(proc_macro2::TokenTree::Literal(literal)) => literal
            .to_string()
            .trim_end_matches("usize")
            .parse::<usize>()
            .map_err(|_| error())?,
        _ => return Err(error()),
    };
    if tokens.next().is_some() {
        return Err(error());
    }
    match keyword.as_str() {
        "node" => Ok(DiagramRequest::Node(index)),
        "token" => Ok(DiagramRequest::Token(index)),
        _ => Err(error()),
    }
}

/// Renders the requested Rustdoc document from the selected Generation's published plan.
///
/// The standalone SVG is written beside the plan on a best-effort basis: documentation must still
/// render when the Generation directory is read-only.
#[cfg(feature = "diagrams")]
fn diagram_documentation(arguments: proc_macro2::TokenStream) -> syn::Result<String> {
    let request = parse_diagram_request(arguments)?;
    let span = proc_macro2::Span::call_site();
    let selected = selected_output().ok_or_else(|| {
        syn::Error::new(
            span,
            "error[RCA9001]: selected Generation has no readable root output",
        )
    })?;
    let directory = selected.directory.join(recursa_diagrams::DIAGRAM_DIRECTORY);
    let plan = selected_diagram_plan(&directory).map_err(|detail| {
        syn::Error::new(
            span,
            format!("error[RCA9005]: selected Generation has no readable diagram plan: {detail}"),
        )
    })?;
    let missing = |what: &str| {
        syn::Error::new(
            span,
            format!("error[RCA9005]: selected Generation diagram plan has no {what}"),
        )
    };
    match request {
        DiagramRequest::Aggregate => Ok(recursa_diagrams::render_aggregate_document(&plan)),
        DiagramRequest::Node(index) => {
            let node = plan
                .nodes
                .get(index)
                .ok_or_else(|| missing(&format!("Node {index}")))?;
            let (document, svg) = recursa_diagrams::render_node_document(node, plan.anchor_depth);
            write_standalone_svg(
                &directory,
                &recursa_diagrams::node_svg_file_name(index),
                &svg,
            );
            Ok(document)
        }
        DiagramRequest::Token(index) => {
            let token = plan
                .tokens
                .get(index)
                .ok_or_else(|| missing(&format!("content token {index}")))?;
            let (document, svg) = recursa_diagrams::render_token_document(token, plan.anchor_depth);
            write_standalone_svg(
                &directory,
                &recursa_diagrams::token_svg_file_name(index),
                &svg,
            );
            Ok(document)
        }
    }
}

/// Decodes and caches the published diagram plan for one Generation directory.
#[cfg(feature = "diagrams")]
fn selected_diagram_plan(directory: &Path) -> Result<Arc<recursa_diagrams::DiagramPlan>, String> {
    static PLANS: OnceLock<Mutex<BTreeMap<PathBuf, Arc<recursa_diagrams::DiagramPlan>>>> =
        OnceLock::new();
    let plans = PLANS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut plans = plans
        .lock()
        .map_err(|_| "diagram plan cache is poisoned".to_owned())?;
    if let Some(plan) = plans.get(directory) {
        return Ok(Arc::clone(plan));
    }
    let encoded = fs::read_to_string(directory.join(recursa_diagrams::PLAN_FILE_NAME))
        .map_err(|error| error.to_string())?;
    let plan = Arc::new(
        recursa_diagrams::DiagramPlan::decode(&encoded).map_err(|error| error.to_string())?,
    );
    plans.insert(directory.to_path_buf(), Arc::clone(&plan));
    Ok(plan)
}

/// Writes one rendered SVG atomically beside the plan, ignoring filesystem failures.
#[cfg(feature = "diagrams")]
fn write_standalone_svg(directory: &Path, name: &str, svg: &str) {
    let target = directory.join(name);
    if fs::read_to_string(&target).is_ok_and(|existing| existing == svg) {
        return;
    }
    let staging = directory.join(format!(".{name}.{}", std::process::id()));
    if fs::write(&staging, svg).is_ok() && fs::rename(&staging, &target).is_err() {
        let _ = fs::remove_file(&staging);
    }
}

/// Declares the grammar's tokens for build-time analysis and generation.
///
/// The declaration may define keyword categories and orthogonal flags, attach one category and
/// zero or more flags to each metadata-bearing keyword, and compose named admission sets from
/// those names with `+` and `-`. A lexical target selects a named set with
/// `#[lex(..., admits(Name))]`; its generated type then accepts exactly that canonical fixed-token
/// membership in addition to its content kind. Legacy fixed-token names remain valid inside
/// `admits(...)` while consumers migrate.
///
/// Closed source-backed matchers are declared centrally in `matchers { ... }` and bound by a
/// same-named content-token field using `#[lex(matcher)]`. The supported constructors are
/// `same_delimiter(opener = r"...")`, `nested(opener = "...", closer = "...")`,
/// `next_exclusion(pattern = r"...", excluded = r"...")`,
/// `operator_run(characters = "...", fences = ["..."], trailing = "...", qualifying = "...")`,
/// and `physical_line(prefix = r"...")`. `physical_line` includes the first LF, CRLF, or CR
/// terminator after its prefix, or extends to EOF when no terminator remains. The same vocabulary
/// is available as named non-emitting rules in `ignore { ... }`.
///
/// Each constructor accepts an optional `priority = N`. Selection compares final matched byte
/// extent, then exact fixed-token status, then explicit priority; declaration order is never a
/// tie-breaker. Equal unresolved priorities are rejected during source analysis. Unterminated
/// delimited and nested matches become generated lexical errors spanning their opener through EOF
/// and expose stable mechanism-specific codes and opener anchors.
/// The analyzer validates the declaration and owns all generated bindings. This procedural macro
/// intentionally emits no runtime items itself.
#[proc_macro]
pub fn tokens(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

/// Parsed configuration shared by the total visitor derives.
struct TotalVisitorInput {
    /// Concrete Node types whose generated event variants are dispatched.
    dispatch: Vec<Type>,
    /// Error type shared by all visitor implementations.
    error: Type,
    /// Path through which generated code reaches the Recursa facade.
    facade: syn::Path,
    /// Optional explicit generated visit-event enum path.
    event: Option<syn::Path>,
}

/// Reads and validates one total visitor derive attribute.
fn total_visitor_input(input: &DeriveInput, attribute: &str) -> syn::Result<TotalVisitorInput> {
    let mut dispatch = None;
    let mut error = None;
    let mut facade = None;
    let mut event = None;
    for attr in &input.attrs {
        if !attr.path().is_ident(attribute) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("dispatch") {
                let value = meta.value()?;
                let content;
                syn::bracketed!(content in value);
                dispatch = Some(
                    content
                        .parse_terminated(Type::parse, syn::Token![,])?
                        .into_iter()
                        .collect(),
                );
                Ok(())
            } else if meta.path.is_ident("error") {
                error = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("facade") {
                facade = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("event") {
                event = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("expected `dispatch`, `error`, `facade`, or `event`"))
            }
        })?;
    }
    Ok(TotalVisitorInput {
        dispatch: dispatch.ok_or_else(|| {
            syn::Error::new_spanned(
                &input.ident,
                format!("missing `dispatch` in #[{attribute}(...)]"),
            )
        })?,
        error: error.ok_or_else(|| {
            syn::Error::new_spanned(
                &input.ident,
                format!("missing `error` in #[{attribute}(...)]"),
            )
        })?,
        facade: facade.unwrap_or_else(mapped_facade_path),
        event,
    })
}

/// Resolves the facade path used by emitted visitor implementations.
fn mapped_facade_path() -> syn::Path {
    match proc_macro_crate::crate_name("recursa") {
        Ok(proc_macro_crate::FoundCrate::Itself) => syn::parse_quote!(crate),
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            syn::Path::from(syn::Ident::new(&name, proc_macro2::Span::call_site()))
        }
        Err(_) => syn::parse_quote!(recursa),
    }
}

/// Derives immutable total-visitor dispatch across a generated grammar family.
///
/// The input must provide `dispatch` and `error` entries in
/// `#[total_visitor(...)]`; `facade` and `event` may override generated paths.
#[proc_macro_derive(TotalVisitor, attributes(total_visitor))]
pub fn total_visitor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    render_total_visitor(&input, false)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives mutable total-visitor dispatch across a generated grammar family.
///
/// The input must provide `dispatch` and `error` entries in
/// `#[total_visitor_mut(...)]`; `facade` and `event` may override generated
/// paths.
#[proc_macro_derive(TotalVisitorMut, attributes(total_visitor_mut))]
pub fn total_visitor_mut(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    render_total_visitor(&input, true)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Renders either immutable or mutable total-visitor dispatch.
///
/// Both forms are produced here so lifetime substitution, event naming, and
/// dispatch bounds cannot drift between the two generated protocols.
fn render_total_visitor(
    input: &DeriveInput,
    mutable: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    // Resolve the event-family paths before changing generics: authored type
    // lifetimes and the generated visit lifetime have different ownership.
    let attribute = if mutable {
        "total_visitor_mut"
    } else {
        "total_visitor"
    };
    let configuration = total_visitor_input(input, attribute)?;
    let name = &input.ident;
    let error = &configuration.error;
    let facade = &configuration.facade;
    let event_path = configuration
        .event
        .clone()
        .map(|mut event| {
            if mutable {
                event
                    .segments
                    .last_mut()
                    .expect("an event path is nonempty")
                    .ident =
                    syn::Ident::new("__RecursaVisitEventMut", proc_macro2::Span::call_site());
            }
            event
        })
        .map_or_else(
            || total_visitor_event_path(&configuration.dispatch, mutable),
            Ok,
        )?;
    let mut family_path = event_path.clone();
    family_path
        .segments
        .last_mut()
        .expect("an event path is nonempty")
        .ident = syn::Ident::new(
        if mutable {
            "__RecursaVisitEventsMut"
        } else {
            "__RecursaVisitEvents"
        },
        proc_macro2::Span::call_site(),
    );
    // Add the source lifetime and one Visitor bound per dispatched Node while
    // preserving the user's type generics on the implementation target.
    let mut implementation_generics = input.generics.clone();
    implementation_generics
        .params
        .insert(0, syn::parse_quote!('__recursa_input));
    let dispatch = configuration
        .dispatch
        .iter()
        .map(|ty| visitor_dispatch_type(ty, syn::parse_quote!('__recursa_input)))
        .collect::<Vec<_>>();
    for ty in &dispatch {
        let predicate: syn::WherePredicate = if mutable {
            syn::parse_quote!(Self: #facade::VisitorMut<#ty, Error = #error>)
        } else {
            syn::parse_quote!(Self: #facade::Visitor<#ty, Error = #error>)
        };
        implementation_generics
            .make_where_clause()
            .predicates
            .push(predicate);
    }
    let (impl_generics, _, where_clause) = implementation_generics.split_for_impl();
    let (_, type_generics, _) = input.generics.split_for_impl();
    // Render the mutable and immutable callback sets from the same validated
    // dispatch list, keeping enter and exit behavior structurally symmetric.
    if mutable {
        let enter = dispatch
            .iter()
            .map(|ty| {
                let variant = visitor_variant(ty, &event_path)?;
                Ok(quote! {
                    #event_path::#variant(node) =>
                        <Self as #facade::VisitorMut<#ty>>::enter_mut(self, node),
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let exit = dispatch
            .iter()
            .map(|ty| {
                let variant = visitor_variant(ty, &event_path)?;
                Ok(quote! {
                    #event_path::#variant(node) =>
                        <Self as #facade::VisitorMut<#ty>>::exit_mut(self, node),
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(quote! {
            impl #impl_generics
                #facade::TotalVisitorMut<#family_path<'__recursa_input>>
                for #name #type_generics #where_clause
            {
                type Error = #error;
                fn total_enter_mut<'__recursa_visit>(
                    &mut self,
                    event: #event_path<'__recursa_visit, '__recursa_input>,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>>
                where
                    #family_path<'__recursa_input>: '__recursa_visit,
                {
                    match event {
                        #(#enter)*
                        _ => ::core::ops::ControlFlow::Continue(()),
                    }
                }
                fn total_exit_mut<'__recursa_visit>(
                    &mut self,
                    event: #event_path<'__recursa_visit, '__recursa_input>,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>>
                where
                    #family_path<'__recursa_input>: '__recursa_visit,
                {
                    match event {
                        #(#exit)*
                        _ => ::core::ops::ControlFlow::Continue(()),
                    }
                }
            }
        })
    } else {
        let enter = dispatch
            .iter()
            .map(|ty| {
                let variant = visitor_variant(ty, &event_path)?;
                Ok(quote! {
                    #event_path::#variant(node) =>
                        <Self as #facade::Visitor<#ty>>::enter(self, node),
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let exit = dispatch
            .iter()
            .map(|ty| {
                let variant = visitor_variant(ty, &event_path)?;
                Ok(quote! {
                    #event_path::#variant(node) =>
                        <Self as #facade::Visitor<#ty>>::exit(self, node),
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(quote! {
            impl #impl_generics
                #facade::TotalVisitor<#family_path<'__recursa_input>>
                for #name #type_generics #where_clause
            {
                type Error = #error;
                fn total_enter<'__recursa_visit>(
                    &mut self,
                    event: #event_path<'__recursa_visit, '__recursa_input>,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>>
                where
                    #family_path<'__recursa_input>: '__recursa_visit,
                {
                    match event {
                        #(#enter)*
                        _ => ::core::ops::ControlFlow::Continue(()),
                    }
                }
                fn total_exit<'__recursa_visit>(
                    &mut self,
                    event: #event_path<'__recursa_visit, '__recursa_input>,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<Self::Error>>
                where
                    #family_path<'__recursa_input>: '__recursa_visit,
                {
                    match event {
                        #(#exit)*
                        _ => ::core::ops::ControlFlow::Continue(()),
                    }
                }
            }
        })
    }
}

/// Derives the generated event enum path from the first dispatched Node type.
fn total_visitor_event_path(dispatch: &[Type], mutable: bool) -> syn::Result<syn::Path> {
    let first = dispatch.first().ok_or_else(|| {
        syn::Error::new(proc_macro2::Span::call_site(), "dispatch cannot be empty")
    })?;
    let Type::Path(first) = first else {
        return Err(syn::Error::new_spanned(
            first,
            "dispatch types must be paths",
        ));
    };
    let mut event = first.path.clone();
    event.segments.pop();
    event.segments.push(syn::PathSegment::from(syn::Ident::new(
        if mutable {
            "__RecursaVisitEventMut"
        } else {
            "__RecursaVisitEvent"
        },
        proc_macro2::Span::call_site(),
    )));
    Ok(event)
}

/// Rebinds every lifetime in a dispatch type to the generated input lifetime.
fn visitor_dispatch_type(ty: &Type, lifetime: syn::Lifetime) -> Type {
    /// Syntax visitor that replaces lifetimes without changing type structure.
    struct ReplaceLifetime(syn::Lifetime);
    impl syn::visit_mut::VisitMut for ReplaceLifetime {
        fn visit_lifetime_mut(&mut self, target: &mut syn::Lifetime) {
            *target = self.0.clone();
        }
    }
    let mut ty = ty.clone();
    ReplaceLifetime(lifetime).visit_type_mut(&mut ty);
    ty
}

/// Encodes a grammar-relative Node path as its collision-free event variant.
fn visitor_variant(ty: &Type, event: &syn::Path) -> syn::Result<syn::Ident> {
    let Type::Path(ty) = ty else {
        unreachable!("dispatch types were validated as paths")
    };
    let mut event_module = event.segments.iter().collect::<Vec<_>>();
    event_module.pop();
    let target = ty.path.segments.iter().collect::<Vec<_>>();
    if target.len() <= event_module.len()
        || !target
            .iter()
            .zip(&event_module)
            .all(|(target, module)| target.ident == module.ident)
    {
        return Err(syn::Error::new_spanned(
            ty,
            "dispatch type is not beneath the configured grammar event module",
        ));
    }
    let relative = target[event_module.len()..]
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>()
        .join("::");
    let suffix = relative
        .split("::")
        .map(|segment| format!("{}_{}", segment.len(), segment))
        .collect::<Vec<_>>()
        .join("_");
    Ok(syn::Ident::new(
        &format!("__RecursaTarget_{suffix}"),
        proc_macro2::Span::call_site(),
    ))
}

/// Computes the deterministic hash used for declaration loader anchors.
fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Normalizes a source path into the platform-independent loader identity.
fn normalized_source_identity(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
            Component::ParentDir => Some("..".to_owned()),
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_visit_variant_uses_the_path_relative_to_the_event_module() {
        let ty = syn::parse_quote!(grammar::public_left::Duplicate);
        let event = syn::parse_quote!(grammar::__RecursaVisitEvent);
        assert_eq!(
            visitor_variant(&ty, &event).unwrap(),
            "__RecursaTarget_11_public_left_9_Duplicate"
        );
    }

    #[test]
    fn root_visit_variant_is_length_encoded() {
        let ty = syn::parse_quote!(grammar::Opaque);
        let event = syn::parse_quote!(grammar::__RecursaVisitEvent);
        assert_eq!(
            visitor_variant(&ty, &event).unwrap(),
            "__RecursaTarget_6_Opaque"
        );
    }
}
