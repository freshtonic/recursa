//! Private stateless JSON projection of one grammar-erased adapter.

use recursa::explorer::{
    Adapter, Element, ElementSchema, ElementTarget, ExplorerDocument, OccurrenceSchema,
    ParseFailure,
};
use recursa::{PrettyConfig, Span};
use serde::Deserialize;
use serde_json::{Value, json};

/// Source-bearing request shared by lex and parse operations.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceRequest {
    /// Complete source to process transactionally.
    pub(super) source: String,
}

/// Source and layout settings for one Pretty request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrettyRequest {
    /// Complete source parsed before rendering.
    pub(super) source: String,
    /// Preferred rendered width, defaulting to the runtime contract.
    #[serde(default = "default_width")]
    pub(super) max_width: usize,
    /// Spaces per indentation level, defaulting to the runtime contract.
    #[serde(default = "default_indent")]
    pub(super) indent: usize,
}

/// Caller-supplied deterministic entropy for a sample request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SampleRequest {
    /// Bytes passed unchanged to the generated sample callback.
    pub(super) bytes: Vec<u8>,
}

/// Returns the default Pretty width without duplicating it in deserialization metadata.
fn default_width() -> usize {
    PrettyConfig::default().max_width()
}

/// Returns the default Pretty indentation without duplicating it in deserialization metadata.
fn default_indent() -> usize {
    PrettyConfig::default().indent()
}

/// Projects the adapter's complete static schema and optional capabilities.
pub(super) fn schema(adapter: Adapter) -> Value {
    let schema = adapter.schema();
    json!({
        "name": schema.name(),
        "root": node_schema_json(schema.root()),
        "nodes": schema.nodes().map(node_schema_json).collect::<Vec<_>>(),
        "tokens": schema.tokens().map(|token| json!({
            "name": token.name(),
            "qualifiedName": token.qualified_name(),
            "kind": token.kind(),
            "canonicalText": token.canonical_text(),
            "highlightClass": format!("{:?}", token.highlight_class()),
        })).collect::<Vec<_>>(),
        "capabilities": {
            "pretty": adapter.capabilities().pretty(),
            "sample": adapter.capabilities().arbitrary(),
        },
    })
}

/// Projects exact lexical records and rejected ranges for one source.
pub(super) fn lex(adapter: Adapter, source: &str) -> Value {
    let result = adapter.lex(source);
    json!({
        "source": result.source(),
        "tokens": result.tokens().map(|token| json!({
            "span": span_json(token.span()),
            "text": token.text(),
            "kind": token.schema().kind(),
            "name": token.schema().name(),
        })).collect::<Vec<_>>(),
        "errors": result.errors().map(|error| json!({
            "span": span_json(error.span()),
            "text": error.text(),
        })).collect::<Vec<_>>(),
    })
}

/// Parses a complete source and returns either one document or structured diagnostics.
pub(super) fn parse(adapter: Adapter, source: &str) -> Value {
    match adapter.parse(source) {
        Ok(document) => success_document(&document),
        Err(failure) => failure_json(&failure),
    }
}

/// Parses and renders a complete source without retaining state between requests.
pub(super) fn pretty(adapter: Adapter, request: &PrettyRequest) -> Value {
    let document = match adapter.parse(&request.source) {
        Ok(document) => document,
        Err(failure) => return failure_json(&failure),
    };
    let Some(output) = document.pretty(&PrettyConfig::new(request.max_width, request.indent))
    else {
        return json!({ "status": "unsupported" });
    };
    json!({
        "status": "ok",
        "rendered": output.rendered,
        "sourceMap": output.source_map.entries().iter().map(|entry| json!({
            "output": { "start": entry.output().start, "end": entry.output().end },
            "source": span_json(entry.source()),
        })).collect::<Vec<_>>(),
    })
}

/// Produces a deterministic synthetic document when the root supports sampling.
pub(super) fn sample(adapter: Adapter, bytes: &[u8]) -> Value {
    match adapter.sample(bytes) {
        None => json!({ "status": "unsupported" }),
        Some(Err(error)) => json!({ "status": "error", "message": error.message() }),
        Some(Ok(document)) => success_document(&document),
    }
}

/// Projects a successful explorer document and its provenance-rich root.
fn success_document(document: &ExplorerDocument<'_>) -> Value {
    json!({
        "status": "ok",
        "document": {
            "source": document.source(),
            "provenance": format!("{:?}", document.provenance()),
            "root": node_json(document.root()),
        },
    })
}

/// Projects strict parse failure without inventing a partial document.
fn failure_json(failure: &ParseFailure<'_>) -> Value {
    json!({
        "status": "error",
        "source": failure.source(),
        "diagnostics": failure.diagnostics().iter().map(|diagnostic| json!({
            "kind": format!("{:?}", diagnostic.kind()),
            "span": span_json(diagnostic.span()),
            "text": diagnostic.text(),
            "found": diagnostic.found(),
            "expected": diagnostic.expected().collect::<Vec<_>>(),
            "message": diagnostic.message(),
        })).collect::<Vec<_>>(),
    })
}

/// Projects one static Node schema, including inactive enum variants.
fn node_schema_json(schema: recursa::explorer::NodeSchema) -> Value {
    json!({
        "name": schema.name(),
        "qualifiedName": schema.qualified_name(),
        "elements": schema.elements().map(element_schema_json).collect::<Vec<_>>(),
        "variants": schema.variants().map(|variant| json!({
            "name": variant.name(),
            "qualifiedName": variant.qualified_name(),
            "elements": variant.elements().map(element_schema_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Projects one source-order element schema and its optional semantic target.
fn element_schema_json(schema: ElementSchema) -> Value {
    let target = schema.target().map(|target| match target {
        ElementTarget::Node(node) => json!({
            "type": "node",
            "name": node.name(),
            "qualifiedName": node.qualified_name(),
        }),
        ElementTarget::ContentToken(token) => json!({
            "type": "contentToken",
            "name": token.name(),
            "qualifiedName": token.qualified_name(),
        }),
    });
    json!({
        "name": schema.name(),
        "role": format!("{:?}", schema.role()),
        "cardinality": format!("{:?}", schema.cardinality()),
        "target": target,
        "tokenKind": schema.token().map(|token| token.kind()),
    })
}

/// Recursively projects the lazy explorer occurrence tree for transport.
fn node_json(node: recursa::explorer::Node<'_>) -> Value {
    let schema = match node.schema() {
        OccurrenceSchema::Node(schema) => json!({
            "type": "node",
            "name": schema.name(),
            "qualifiedName": schema.qualified_name(),
        }),
        OccurrenceSchema::ContentToken(schema) => json!({
            "type": "contentToken",
            "name": schema.name(),
            "qualifiedName": schema.qualified_name(),
        }),
    };
    json!({
        "schema": schema,
        "activeVariant": node.active_variant().map(|variant| variant.name()),
        "span": node.span().map(span_json),
        "text": node.token_text(),
        "elements": node.elements().map(element_json).collect::<Vec<_>>(),
    })
}

/// Projects one element occurrence and its recursive child occurrences.
fn element_json(element: Element<'_>) -> Value {
    json!({
        "schema": element_schema_json(element.schema()),
        "span": element.span().map(span_json),
        "text": element.text(),
        "children": element.children().map(node_json).collect::<Vec<_>>(),
    })
}

/// Converts the runtime's checked byte range into the private wire shape.
fn span_json(span: Span) -> Value {
    json!({ "start": span.start(), "end": span.end() })
}
