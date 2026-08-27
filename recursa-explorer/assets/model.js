"use strict";

function spanText(span) {
  return span ? `${span.start}..${span.end}` : "synthetic";
}

export function occurrenceEntries(root) {
  const entries = [];
  let order = 0;
  function visitNode(node, depth, parentKey) {
    const key = `occurrence-${order}`;
    const variant = node.activeVariant ? `::${node.activeVariant}` : "";
    const entry = {
      key, parentKey, depth, order: order++, type: "node", span: node.span,
      label: `${node.schema.name}${variant}`,
      meta: [node.text == null ? null : JSON.stringify(node.text), spanText(node.span)].filter(Boolean).join(" · "),
      children: [], raw: node,
    };
    entries.push(entry);
    for (const element of node.elements || []) {
      const elementKey = `occurrence-${order}`;
      const role = element.schema.role;
      const cardinality = element.schema.cardinality;
      const elementEntry = {
        key: elementKey, parentKey: key, depth: depth + 1, order: order++, type: "element",
        span: element.span, label: element.schema.name,
        meta: [element.text == null ? null : JSON.stringify(element.text), spanText(element.span), role, cardinality]
          .filter(Boolean).join(" · "),
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
  return { entries, rootKey: rootEntry.key };
}

export function schemaSummary(schema) {
  return {
    grammar: schema.name,
    root: schema.root.qualifiedName,
    nodes: schema.nodes.map((node) => ({
      name: node.qualifiedName,
      elements: node.elements,
      variants: node.variants,
    })),
    tokens: schema.tokens.map((token) => ({
      name: token.name,
      spelling: token.canonicalText,
      className: token.highlightClass,
      kind: token.kind,
    })),
  };
}
