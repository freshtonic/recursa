"use strict";

import { partitionDecorations, spanToUtf16, byteToUtf16Map, utf8ByteAtUtf16 } from "./offsets.js";
import { spanText, isEnumField, displayName } from "./model.js";

function clear(element) { element.replaceChildren(); }

export function renderGutter(element, source) {
  clear(element);
  const lineCount = source ? source.split("\n").length : 1;
  const fragment = document.createDocumentFragment();
  for (let line = 1; line <= lineCount; line += 1) {
    const row = document.createElement("span");
    row.textContent = String(line);
    row.style.display = "block";
    fragment.append(row);
  }
  element.append(fragment);
}

export function renderSourceOverlay(element, source, tokens, lexErrors, diagnostics, selectedSpan, tokenClasses) {
  clear(element);
  const decorations = [
    ...tokens.map((token) => {
      const classes = [`tok-${tokenClasses.get(token.kind) || "Content"}`];
      if (selectedSpan) {
        const exact = selectedSpan.start === token.span.start && selectedSpan.end === token.span.end;
        const contains = selectedSpan.start <= token.span.start && selectedSpan.end >= token.span.end;
        if (exact) classes.push("source-exact");
        else if (contains) classes.push("source-contains");
      }
      return { span: token.span, className: classes.join(" ") };
    }),
    ...lexErrors.map((error) => ({ span: error.span, className: "lex-error" })),
    ...diagnostics.map((diagnostic) => ({ span: diagnostic.span, className: "diagnostic-range" })),
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

export function renderBreadcrumb(element, entries, byKey, selectedKey, onSelect) {
  clear(element);
  const chain = [];
  let cursor = selectedKey ? byKey.get(selectedKey) : null;
  while (cursor) {
    chain.unshift(cursor);
    cursor = cursor.parentKey ? byKey.get(cursor.parentKey) : null;
  }
  chain.filter((entry) => !isEnumField(entry)).forEach((entry, index) => {
    if (index > 0) {
      const sep = document.createElement("span");
      sep.className = "crumb-sep";
      sep.textContent = "›";
      element.append(sep);
    }
    const crumb = document.createElement("button");
    crumb.type = "button";
    crumb.className = `crumb${entry.key === selectedKey ? " is-current" : ""}`;
    crumb.textContent = entry.kind === "node" && entry.variant ? `${entry.name}::${entry.variant}` : displayName(entry);
    crumb.addEventListener("click", () => onSelect(entry));
    element.append(crumb);
  });
}

export function renderOccurrenceFooter(element, entry) {
  clear(element);
  if (!entry) return;
  const title = document.createElement("div");
  title.className = "occurrence-title";
  const eyebrow = document.createElement("span");
  eyebrow.className = "eyebrow";
  eyebrow.textContent = "Occurrence";
  const name = document.createElement("span");
  name.className = "occurrence-name";
  name.textContent = displayName(entry);
  title.append(eyebrow, name);
  if (entry.variant) {
    const variant = document.createElement("span");
    variant.className = "occurrence-variant";
    variant.textContent = `::${entry.variant}`;
    title.append(variant);
  }
  element.append(title);

  const fields = document.createElement("div");
  fields.className = "occurrence-fields";
  const entries = [
    ["Kind", entry.kind],
    ["Span", spanText(entry.span)],
    ["Depth", String(entry.depth)],
  ];
  if (entry.kind === "element") {
    entries.push(["Role", entry.role], ["Cardinality", entry.cardinality]);
  }
  entries.push(["Text", entry.text != null ? JSON.stringify(entry.text) : "—"]);
  entries.push(["Children", String(entry.children.length)]);
  for (const [key, value] of entries) {
    const field = document.createElement("div");
    field.className = "occurrence-field";
    const fieldKey = document.createElement("span");
    fieldKey.className = "field-key";
    fieldKey.textContent = key;
    const fieldValue = document.createElement("span");
    fieldValue.className = "field-value";
    fieldValue.textContent = value;
    field.append(fieldKey, fieldValue);
    fields.append(field);
  }
  element.append(fields);
}

export function selectTextareaSpan(textarea, source, span) {
  const converted = spanToUtf16(byteToUtf16Map(source), span);
  textarea.focus();
  textarea.setSelectionRange(converted.start, converted.end);
}
