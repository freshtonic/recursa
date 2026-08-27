"use strict";

import { partitionDecorations, spanToUtf16, byteToUtf16Map, utf8ByteAtUtf16 } from "./offsets.js";
import { schemaSummary } from "./model.js";

function clear(element) { element.replaceChildren(); }

export function renderSourceOverlay(element, source, tokens, lexErrors, diagnostics, selectedSpan, tokenClasses) {
  clear(element);
  const decorations = [
    ...tokens.map((token) => ({ span: token.span, className: `tok-${tokenClasses.get(token.kind) || "Content"}` })),
    ...lexErrors.map((error) => ({ span: error.span, className: "lex-error" })),
    ...diagnostics.map((diagnostic) => ({ span: diagnostic.span, className: "diagnostic-range" })),
    ...(selectedSpan ? [{ span: selectedSpan, className: "source-selected" }] : []),
  ];
  for (const segment of partitionDecorations(source, decorations)) {
    const span = document.createElement("span");
    span.textContent = segment.text;
    span.className = [...new Set(segment.decorations.map((item) => item.className))].join(" ");
    span.dataset.byteStart = String(utf8ByteAtUtf16(source, segment.start));
    span.dataset.byteEnd = String(utf8ByteAtUtf16(source, segment.end));
    element.append(span);
  }
  if (source.endsWith("\n")) element.append(document.createTextNode("\n"));
}

export function renderDiagnostics(element, diagnostics, source, onSelect) {
  clear(element);
  for (const diagnostic of diagnostics) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    const expected = diagnostic.expected?.length ? ` Expected ${diagnostic.expected.join(", ")}.` : "";
    button.textContent = `${diagnostic.kind}: ${diagnostic.message}${expected}`;
    button.addEventListener("click", () => onSelect(diagnostic.span));
    item.append(button);
    element.append(item);
  }
  if (!diagnostics.length && source) {
    const item = document.createElement("li");
    item.textContent = "No diagnostics.";
    element.append(item);
  }
}

export function renderSchema(element, schema) {
  clear(element);
  const summary = schemaSummary(schema);
  const identity = document.createElement("p");
  identity.textContent = `${summary.grammar} · root ${summary.root}`;
  element.append(identity);
  const nodesHeading = document.createElement("strong");
  nodesHeading.textContent = `Nodes (${summary.nodes.length})`;
  element.append(nodesHeading);
  const nodes = document.createElement("ul");
  nodes.className = "schema-list";
  for (const node of summary.nodes) {
    const item = document.createElement("li"); const code = document.createElement("code");
    code.textContent = node.name; item.append(code);
    const shapes = [...node.elements.map((element) => element.name),
      ...node.variants.map((variant) => `${variant.name}(${variant.elements.map((element) => element.name).join(", ")})`)];
    if (shapes.length) item.append(document.createTextNode(` — ${shapes.join("; ")}`));
    nodes.append(item);
  }
  element.append(nodes);
  const tokensHeading = document.createElement("strong");
  tokensHeading.textContent = `Tokens (${summary.tokens.length})`;
  element.append(tokensHeading);
  const tokens = document.createElement("ul"); tokens.className = "token-list";
  for (const token of summary.tokens) {
    const item = document.createElement("li");
    item.textContent = `${token.name} #${token.kind}${token.spelling ? ` “${token.spelling}”` : ""} · ${token.className}`;
    tokens.append(item);
  }
  element.append(tokens);
}

export function selectTextareaSpan(textarea, source, span) {
  const converted = spanToUtf16(byteToUtf16Map(source), span);
  textarea.focus();
  textarea.setSelectionRange(converted.start, converted.end);
}

export function renderPretty(element, rendered, sourceMap, selectedSourceSpan) {
  clear(element);
  const decorations = sourceMap.map((entry) => ({
    span: entry.output,
    source: entry.source,
    className: selectedSourceSpan && entry.source.start <= selectedSourceSpan.start &&
      entry.source.end >= selectedSourceSpan.end ? "pretty-map" : "pretty-source",
  }));
  for (const segment of partitionDecorations(rendered, decorations)) {
    const span = document.createElement("span");
    span.textContent = segment.text;
    span.className = segment.decorations.map((item) => item.className).join(" ");
    const mapping = segment.decorations[0];
    if (mapping) {
      span.tabIndex = 0;
      span.dataset.sourceStart = String(mapping.source.start);
      span.dataset.sourceEnd = String(mapping.source.end);
    }
    element.append(span);
  }
}
