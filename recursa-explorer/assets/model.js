"use strict";

const CARDINALITY_DISPLAY = {
  One: "One",
  Optional: "Optional",
  ZeroOrMore: "ZeroOrMore",
  OneOrMore: "OneOrMore",
  OptionalOneOrMore: "Optional<OneOrMore>",
};

export function cardinalityDisplay(raw) {
  return CARDINALITY_DISPLAY[raw] || raw;
}

export function spanText(span) {
  return span ? `${span.start}..${span.end}` : "synthetic";
}

/// True for an element occurrence that is itself a field of an active enum variant. Its name is
/// a codegen artifact (the authored field name, or an auto-generated `field_N` slot for an
/// unnamed field) prefixed with the variant — information the variant label (`Enum::Variant`)
/// already carries, so the UI never renders it.
export function isEnumField(entry) {
  return entry.kind === "element" && Boolean(entry.enumField);
}

/// The name to render for `entry`, empty for a suppressed enum-field occurrence.
export function displayName(entry) {
  return isEnumField(entry) ? "" : entry.name;
}

/// Flattens one parsed root into UI occurrences: one entry per node or element occurrence, in
/// document order, each addressable by its own key and reachable from every other by that key.
export function occurrenceEntries(root) {
  const entries = [];
  let order = 0;
  function visitNode(node, depth, parentKey) {
    const key = `occurrence-${order}`;
    const entry = {
      key, parentKey, depth, order: order++, kind: "node",
      name: node.schema.name, variant: node.activeVariant || null,
      role: null, cardinality: null, cardinalityRaw: null,
      span: node.span, text: node.text ?? null,
      children: [], raw: node,
    };
    entries.push(entry);
    for (const element of node.elements || []) {
      const elementKey = `occurrence-${order}`;
      const elementEntry = {
        key: elementKey, parentKey: key, depth: depth + 1, order: order++, kind: "element",
        name: element.schema.name, variant: null, enumField: Boolean(entry.variant),
        role: element.schema.role, cardinalityRaw: element.schema.cardinality,
        cardinality: cardinalityDisplay(element.schema.cardinality),
        span: element.span, text: element.text ?? null,
        children: [], raw: element,
      };
      entries.push(elementEntry);
      entry.children.push(elementKey);
      for (const child of element.children || []) {
        const childEntry = visitNode(child, depth + 2, elementKey);
        elementEntry.children.push(childEntry.key);
      }
    }
    return entry;
  }
  const rootEntry = visitNode(root, 0, null);
  computeSpans(entries);
  return { entries, rootKey: rootEntry.key };
}

/// Fills in `span: null` occurrences (a Node with no directly captured token, or a container
/// element with no fixed token of its own) with the min/max of their children's spans, bottom-up.
/// A span stays `null` only when the occurrence and every descendant are all synthetic.
function computeSpans(entries) {
  const byKey = new Map(entries.map((entry) => [entry.key, entry]));
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry.span) continue;
    let start = null;
    let end = null;
    for (const key of entry.children) {
      const child = byKey.get(key);
      if (!child.span) continue;
      start = start === null ? child.span.start : Math.min(start, child.span.start);
      end = end === null ? child.span.end : Math.max(end, child.span.end);
    }
    if (start !== null) entry.span = { start, end };
  }
}
