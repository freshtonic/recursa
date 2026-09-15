//! Loss-minimizing migration from retired `Node` derives to `ast_node!` declarations.

use proc_macro2::{LineColumn, Span};
use quote::ToTokens;
use std::io::Write;
use std::process::{Command, Stdio};
use syn::{spanned::Spanned, visit::Visit};

/// Summary of one source conversion.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report {
    /// Number of declarations converted.
    pub declarations: usize,
    /// Number of concrete storage constructors replaced.
    pub containers: usize,
    /// Number of declaration-side lifetime occurrences removed.
    pub lifetimes: usize,
    /// Number of retired `node(omit(...))` attributes migrated.
    pub omissions: usize,
    /// Sites which require a human semantic decision.
    pub unsupported: Vec<String>,
}

#[derive(Clone, Debug)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Convert every retired Node declaration in a complete Rust source file.
pub fn convert_source(source: &str) -> Result<(String, Report), syn::Error> {
    let file = syn::parse_file(source)?;
    let offsets = LineOffsets::new(source);
    let mut report = Report::default();
    let mut edits = Vec::new();
    let unqualified_box_is_storage = !declares_box(&file.items);
    collect_items(
        &file.items,
        source,
        &offsets,
        unqualified_box_is_storage,
        &mut edits,
        &mut report,
    );
    edits.sort_by_key(|edit| (edit.start, edit.end));
    for pair in edits.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(syn::Error::new(
                Span::call_site(),
                "converter produced overlapping edits",
            ));
        }
    }
    let mut output = source.to_owned();
    for edit in edits.into_iter().rev() {
        output.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok((output, report))
}

/// Convert complete Rust source files embedded in string literals of a Rust host file.
pub fn convert_embedded_sources(source: &str) -> Result<(String, Report), syn::Error> {
    let file = syn::parse_file(source)?;
    let offsets = LineOffsets::new(source);
    let mut visitor = EmbeddedVisitor {
        offsets: &offsets,
        edits: Vec::new(),
        report: Report::default(),
    };
    visitor.visit_file(&file);
    visitor.edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut output = source.to_owned();
    for edit in visitor.edits.into_iter().rev() {
        output.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok((output, visitor.report))
}

/// Format complete Rust source while allowing rustfmt to see inside `ast_node!` bodies.
pub fn format_source(source: &str) -> Result<String, String> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    let offsets = LineOffsets::new(source);
    let mut headers = Vec::new();
    collect_ast_node_headers(&file.items, source, &offsets, &mut headers);
    let mut disguised = source.to_owned();
    for (index, (start, end, _)) in headers.iter().enumerate().rev() {
        disguised.replace_range(
            *start..*end,
            &format!("mod __recursa_ast_node_format_{index} {{"),
        );
    }
    let mut child = Command::new("rustfmt")
        .args(["--emit", "stdout", "--edition", "2024"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start rustfmt: {error}"))?;
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(disguised.as_bytes())
        .map_err(|error| format!("write rustfmt input: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for rustfmt: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let mut formatted = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    for (index, (_, _, header)) in headers.iter().enumerate() {
        formatted = formatted.replace(&format!("mod __recursa_ast_node_format_{index} {{"), header);
    }
    Ok(formatted)
}

/// Format Rust fenced code blocks containing `ast_node!` declarations in Markdown.
pub fn format_markdown(source: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("```rust") {
        let content_start = rest[start..]
            .find('\n')
            .map(|offset| start + offset + 1)
            .ok_or("unterminated Rust fence header")?;
        let Some(relative_end) = rest[content_start..].find("```") else {
            return Err("unterminated Rust fence".into());
        };
        let content_end = content_start + relative_end;
        output.push_str(&rest[..content_start]);
        let content = &rest[content_start..content_end];
        if content.contains("ast_node!") {
            output.push_str(&format_source(content)?);
        } else {
            output.push_str(content);
        }
        rest = &rest[content_end..];
    }
    output.push_str(rest);
    Ok(output)
}

fn collect_ast_node_headers(
    items: &[syn::Item],
    source: &str,
    offsets: &LineOffsets,
    headers: &mut Vec<(usize, usize, String)>,
) {
    for item in items {
        match item {
            syn::Item::Macro(item)
                if item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "ast_node") =>
            {
                let start = offsets.byte(item.mac.path.span().start());
                let content = offsets.byte(item.mac.tokens.span().start());
                if let Some(relative) = source[start..content].find('{') {
                    let end = start + relative + 1;
                    headers.push((start, end, source[start..end].to_owned()));
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_ast_node_headers(nested, source, offsets, headers);
                }
            }
            _ => {}
        }
    }
}

struct EmbeddedVisitor<'a> {
    offsets: &'a LineOffsets,
    edits: Vec<Edit>,
    report: Report,
}

impl<'ast> Visit<'ast> for EmbeddedVisitor<'_> {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.process_literal(literal);
        syn::visit::visit_lit_str(self, literal);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.process_token_stream(item.tokens.clone());
        syn::visit::visit_macro(self, item);
    }
}

impl EmbeddedVisitor<'_> {
    fn process_token_stream(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Literal(token) => {
                    if let Ok(literal) = syn::parse_str::<syn::LitStr>(&token.to_string()) {
                        self.process_literal(&syn::LitStr::new(&literal.value(), token.span()));
                    }
                }
                proc_macro2::TokenTree::Group(group) => self.process_token_stream(group.stream()),
                _ => {}
            }
        }
    }

    fn process_literal(&mut self, literal: &syn::LitStr) {
        let value = literal.value();
        if value.contains("Node") || value.contains("ast_node!") {
            let conversion = if value.contains("{{") {
                convert_format_template(&value).or_else(|_| convert_source(&value))
            } else {
                convert_source(&value).or_else(|_| convert_format_template(&value))
            };
            if let Ok((mut converted, report)) = conversion {
                if !value.contains("{{") && converted.contains("ast_node!") {
                    converted = format_source(&converted).unwrap_or(converted);
                }
                if report.declarations > 0
                    || (converted.contains("ast_node!") && converted.contains('\n'))
                {
                    self.report.declarations += report.declarations;
                    self.report.containers += report.containers;
                    self.report.lifetimes += report.lifetimes;
                    self.report.omissions += report.omissions;
                    self.report.unsupported.extend(report.unsupported);
                    self.edits.push(Edit {
                        start: self.offsets.byte(literal.span().start()),
                        end: self.offsets.byte(literal.span().end()),
                        replacement: raw_string_literal(&converted),
                    });
                }
            }
        }
    }
}

#[doc(hidden)]
pub fn convert_format_template(value: &str) -> Result<(String, Report), syn::Error> {
    let substitutions = [
        ("{invalid_variant}", "__RECURSA_INVALID_VARIANT"),
        ("{field}", "__recursa_field: Leaf"),
        ("{imports}", "struct __RecursaImports;"),
        ("{order}", "struct __RecursaOrder;"),
        ("{nodes}", "struct __RecursaNodes;"),
        ("{exclusion}", "#[recursa_template_exclusion]"),
        ("{level}", "__RECURSA_LEVEL"),
        ("{REUSE_GRAMMAR}", "struct __RecursaReuseGrammar;"),
        ("\n{}", "\nstruct __RecursaPositional;"),
        ("{prelude}", "mod __RecursaPrelude {"),
        ("{block}", "struct __RecursaBlock;"),
        ("{child}", "__RecursaChild"),
        ("{depth}", "RECURSADEPTH"),
        ("{}", "RECURSAPOSITIONAL"),
    ];
    let mut dummy = value.to_owned();
    for (placeholder, replacement) in substitutions {
        dummy = dummy.replace(placeholder, replacement);
    }
    let (protected, format_slots) = protect_format_slots(&dummy)?;
    dummy = protected;
    dummy = dummy.replace("{{", "{").replace("}}", "}");
    let (converted, report) = convert_source(&dummy)?;
    let mut converted = format_source(&converted).unwrap_or(converted);
    for (index, (_, replacement)) in substitutions.iter().enumerate() {
        let marker = (b'A' + index as u8) as char;
        converted = converted.replace(replacement, &format!("__RECURSA_PLACEHOLDER_{marker}__"));
    }
    for (index, (_, replacement)) in format_slots.iter().enumerate() {
        converted = converted.replace(
            replacement,
            &format!("__RECURSA_FORMAT_PLACEHOLDER_{index}__"),
        );
    }
    converted = converted.replace('{', "{{").replace('}', "}}");
    for (index, (placeholder, _)) in substitutions.iter().enumerate() {
        let marker = (b'A' + index as u8) as char;
        converted = converted.replace(&format!("__RECURSA_PLACEHOLDER_{marker}__"), placeholder);
    }
    for (index, (placeholder, _)) in format_slots.iter().enumerate() {
        converted = converted.replace(
            &format!("__RECURSA_FORMAT_PLACEHOLDER_{index}__"),
            placeholder,
        );
    }
    Ok((converted, report))
}

fn protect_format_slots(value: &str) -> Result<(String, Vec<(String, String)>), syn::Error> {
    let bytes = value.as_bytes();
    let mut output = String::new();
    let mut slots = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(relative_start) = value[cursor..].find('{') else {
            output.push_str(&value[cursor..]);
            break;
        };
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        if bytes.get(start + 1) == Some(&b'{') {
            output.push_str("{{");
            cursor = start + 2;
            continue;
        }
        cursor = start;
        cursor += 1;
        let mut depth = 1usize;
        while cursor < bytes.len() && depth > 0 {
            match bytes[cursor] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            return Err(syn::Error::new(
                Span::call_site(),
                "unterminated format placeholder",
            ));
        }
        let placeholder = value[start..cursor].to_owned();
        let replacement = format!("__RECURSA_FORMAT_SLOT_{}__", slots.len());
        output.push_str(&replacement);
        slots.push((placeholder, replacement));
    }
    Ok((output, slots))
}

fn raw_string_literal(value: &str) -> String {
    for hashes in 1..=255 {
        let fence = "#".repeat(hashes);
        if !value.contains(&format!("\"{fence}")) {
            return format!("r{fence}\"{value}\"{fence}");
        }
    }
    unreachable!("a Rust raw string delimiter is available")
}

fn collect_items(
    items: &[syn::Item],
    source: &str,
    offsets: &LineOffsets,
    unqualified_box_is_storage: bool,
    edits: &mut Vec<Edit>,
    report: &mut Report,
) {
    for item in items {
        match item {
            syn::Item::Struct(item) if node_attr(&item.attrs) => {
                let fields = item.fields.iter().collect::<Vec<_>>();
                convert_item(
                    item,
                    &item.attrs,
                    &item.generics,
                    &fields,
                    source,
                    offsets,
                    unqualified_box_is_storage,
                    edits,
                    report,
                )
            }
            syn::Item::Enum(item) if node_attr(&item.attrs) => {
                let fields = item
                    .variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter())
                    .collect::<Vec<_>>();
                convert_item(
                    item,
                    &item.attrs,
                    &item.generics,
                    &fields,
                    source,
                    offsets,
                    unqualified_box_is_storage,
                    edits,
                    report,
                )
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_items(
                        nested,
                        source,
                        offsets,
                        unqualified_box_is_storage,
                        edits,
                        report,
                    );
                }
            }
            _ => {}
        }
    }
}

fn convert_item<T: Spanned>(
    item: &T,
    attrs: &[syn::Attribute],
    generics: &syn::Generics,
    fields: &[&syn::Field],
    source: &str,
    offsets: &LineOffsets,
    unqualified_box_is_storage: bool,
    edits: &mut Vec<Edit>,
    report: &mut Report,
) {
    report.declarations += 1;
    let start = offsets.byte(item.span().start());
    let end = offsets.byte(item.span().end());
    let facade = node_facade(attrs);
    edits.push(Edit {
        start,
        end: start,
        replacement: format!("{facade}ast_node! {{\n"),
    });
    edits.push(Edit {
        start: end,
        end,
        replacement: "\n}".into(),
    });
    for attr in attrs {
        if attr.path().is_ident("derive") {
            let paths = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            );
            if let Ok(paths) = paths {
                let kept = paths
                    .iter()
                    .filter(|path| path.segments.last().is_none_or(|s| s.ident != "Node"))
                    .collect::<Vec<_>>();
                if kept.len() != paths.len() {
                    let a = offsets.byte(attr.span().start());
                    let b = offsets.byte(attr.span().end());
                    let replacement = if kept.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "#[derive({})]",
                            kept.iter()
                                .map(|p| p.to_token_stream().to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    edits.push(Edit {
                        start: a,
                        end: b,
                        replacement,
                    });
                }
            }
        }
        if attr.path().is_ident("node") {
            let text = source
                .get(offsets.byte(attr.span().start())..offsets.byte(attr.span().end()))
                .unwrap_or("");
            if text.contains("omit") {
                if let Some(replacement) = migrate_node_omit(text) {
                    edits.push(Edit {
                        start: offsets.byte(attr.span().start()),
                        end: offsets.byte(attr.span().end()),
                        replacement,
                    });
                    report.omissions += 1;
                } else {
                    report.unsupported.push(format!(
                        "combined node attribute at {}:{}",
                        attr.span().start().line,
                        attr.span().start().column + 1
                    ));
                }
            }
        }
    }
    let lifetime_count = generics
        .params
        .iter()
        .filter(|param| matches!(param, syn::GenericParam::Lifetime(_)))
        .count();
    if lifetime_count > 0 {
        report.lifetimes += lifetime_count;
        let retained = generics
            .params
            .iter()
            .filter(|param| !matches!(param, syn::GenericParam::Lifetime(_)))
            .map(ToTokens::to_token_stream)
            .collect::<Vec<_>>();
        let lt = generics
            .lt_token
            .as_ref()
            .expect("parameters have an opening delimiter");
        let gt = generics
            .gt_token
            .as_ref()
            .expect("parameters have a closing delimiter");
        let replacement = if retained.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                retained
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let mut generic_start = offsets.byte(lt.span.start());
        let mut generic_end = offsets.byte(gt.span.end());
        if generic_start > 0 && source.as_bytes().get(generic_start - 1) == Some(&b'<') {
            generic_start -= 1;
        }
        if source.as_bytes().get(generic_end) == Some(&b'>') {
            generic_end += 1;
        }
        edits.push(Edit {
            start: generic_start,
            end: generic_end,
            replacement,
        });
    }
    for field in fields {
        let mut transformer = TypeTransformer {
            needs_rewrite: false,
            unqualified_box_is_storage,
        };
        transformer.visit_type(&field.ty);
        if transformer.needs_rewrite {
            let original_type = source
                .get(offsets.byte(field.ty.span().start())..offsets.byte(field.ty.span().end()))
                .unwrap_or("");
            if original_type.contains("/*") || original_type.contains("//") {
                report.unsupported.push(format!(
                    "comment inside rewritten field type at {}:{}",
                    field.ty.span().start().line,
                    field.ty.span().start().column + 1
                ));
                continue;
            }
            let replacement = transform_type(
                &field.ty,
                unqualified_box_is_storage,
                &mut report.containers,
                &mut report.lifetimes,
            );
            edits.push(Edit {
                start: offsets.byte(field.ty.span().start()),
                end: offsets.byte(field.ty.span().end()),
                replacement,
            });
        }
    }
}

fn node_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .meta
                .to_token_stream()
                .to_string()
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == "Node")
    })
}

fn node_facade(attrs: &[syn::Attribute]) -> String {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        if let Ok(paths) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) {
            for path in paths {
                if path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "Node")
                {
                    let mut segments = path.segments.clone();
                    segments.pop();
                    return if segments.is_empty() {
                        "recursa::".into()
                    } else {
                        let rendered = segments.to_token_stream().to_string().replace(' ', "");
                        format!("{}::", rendered.trim_end_matches(':'))
                    };
                }
            }
        }
    }
    "recursa::".into()
}

struct TypeTransformer {
    needs_rewrite: bool,
    unqualified_box_is_storage: bool,
}
impl<'ast> Visit<'ast> for TypeTransformer {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if container_kind(&node.path, self.unqualified_box_is_storage).is_some() || node.path.segments.iter().any(|s| matches!(&s.arguments, syn::PathArguments::AngleBracketed(a) if a.args.iter().any(|x| matches!(x, syn::GenericArgument::Lifetime(_))))) { self.needs_rewrite = true; }
        syn::visit::visit_type_path(self, node)
    }
}

fn transform_type(
    ty: &syn::Type,
    unqualified_box_is_storage: bool,
    containers: &mut usize,
    lifetimes: &mut usize,
) -> String {
    match ty {
        syn::Type::Path(path) => {
            if let Some(marker) = container_kind(&path.path, unqualified_box_is_storage) {
                if let Some(inner) = first_type_arg(&path.path) {
                    *containers += 1;
                    return format!(
                        "{marker}!({})",
                        transform_type(inner, unqualified_box_is_storage, containers, lifetimes)
                    );
                }
            }
            let mut path = path.clone();
            for segment in &mut path.path.segments {
                if let syn::PathArguments::AngleBracketed(args) = &mut segment.arguments {
                    let before = args.args.len();
                    args.args = args
                        .args
                        .iter()
                        .filter(|arg| !matches!(arg, syn::GenericArgument::Lifetime(_)))
                        .cloned()
                        .collect();
                    *lifetimes += before - args.args.len();
                    for arg in &mut args.args {
                        if let syn::GenericArgument::Type(inner) = arg {
                            let rendered = transform_type(
                                inner,
                                unqualified_box_is_storage,
                                containers,
                                lifetimes,
                            );
                            if let Ok(parsed) = syn::parse_str(&rendered) {
                                *inner = parsed;
                            }
                        }
                    }
                    if args.args.is_empty() {
                        segment.arguments = syn::PathArguments::None;
                    }
                }
            }
            path.to_token_stream().to_string()
        }
        _ => ty.to_token_stream().to_string(),
    }
}

fn first_type_arg(path: &syn::Path) -> Option<&syn::Type> {
    path.segments.last().and_then(|s| {
        if let syn::PathArguments::AngleBracketed(a) = &s.arguments {
            a.args.iter().find_map(|x| {
                if let syn::GenericArgument::Type(t) = x {
                    Some(t)
                } else {
                    None
                }
            })
        } else {
            None
        }
    })
}

fn container_kind(path: &syn::Path, unqualified_box_is_storage: bool) -> Option<&'static str> {
    let names = path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>();
    match names.as_slice() {
        [name] if name == "Vec" => Some("zero_or_many"),
        [name] if name == "Vec1" => Some("one_or_many"),
        [name] if name == "Box" && unqualified_box_is_storage => Some("boxed"),
        [a, b] if (a == "std" || a == "alloc") && b == "boxed" => None,
        [a, b, c] if (a == "std" || a == "alloc") && b == "boxed" && c == "Box" => Some("boxed"),
        [a, b] if (a == "vec1" || a == "recursa") && b == "Vec1" => Some("one_or_many"),
        [a, b] if a == "recursa" && (b == "ArenaBox" || b == "ArenaVec" || b == "ArenaVec1") => {
            Some(match b.as_str() {
                "ArenaBox" => "boxed",
                "ArenaVec" => "zero_or_many",
                _ => "one_or_many",
            })
        }
        _ => None,
    }
}

fn declares_box(items: &[syn::Item]) -> bool {
    items.iter().any(|item| match item {
        syn::Item::Struct(x) => x.ident == "Box",
        syn::Item::Enum(x) => x.ident == "Box",
        syn::Item::Type(x) => x.ident == "Box",
        syn::Item::Mod(x) => x
            .content
            .as_ref()
            .is_some_and(|(_, nested)| declares_box(nested)),
        _ => false,
    })
}

fn migrate_node_omit(text: &str) -> Option<String> {
    let body = text.strip_prefix("#[node(")?.strip_suffix(")]")?;
    let body = body.trim();
    if body.starts_with("omit(") && body.ends_with(')') {
        Some(format!("#[{}]", body))
    } else {
        None
    }
}

struct LineOffsets {
    starts: Vec<usize>,
}
impl LineOffsets {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }
    fn byte(&self, lc: LineColumn) -> usize {
        self.starts[lc.line.saturating_sub(1)] + lc.column
    }
}
