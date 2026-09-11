"use strict";

import { lineStarts, lineAtByte } from "./offsets.js";

const LINE_HEIGHT = 26;

function ancestorKeys(key, byKey) {
  const result = new Set();
  let cursor = key ? byKey.get(key) : null;
  while (cursor) { result.add(cursor.key); cursor = cursor.parentKey ? byKey.get(cursor.parentKey) : null; }
  return result;
}

/// Renders the Grain mode's depth-column gutter beside the (separately rendered) token-highlighted
/// source: one bar per occurrence that has a span, positioned by byte-offset-derived line and depth.
export function createGrain(gutterEl, model, source, onSelect, onHover) {
  gutterEl.replaceChildren();
  const byKey = new Map(model.entries.map((entry) => [entry.key, entry]));
  const starts = lineStarts(source);
  const maxDepth = model.entries.reduce((max, entry) => Math.max(max, entry.depth), 0);
  gutterEl.style.width = `${(maxDepth + 1) * 8}px`;
  gutterEl.style.height = `${starts.length * LINE_HEIGHT}px`;

  const bars = new Map();
  for (const entry of model.entries) {
    if (!entry.span) continue;
    const firstLine = lineAtByte(starts, entry.span.start);
    const lastLine = lineAtByte(starts, Math.max(entry.span.start, entry.span.end - 1));
    const bar = document.createElement("div");
    bar.className = `grain-bar grain-bar-${entry.kind}`;
    bar.style.left = `${entry.depth * 8}px`;
    bar.style.top = `${firstLine * LINE_HEIGHT + 3}px`;
    bar.style.height = `${(lastLine - firstLine + 1) * LINE_HEIGHT - 6}px`;
    bar.dataset.key = entry.key;
    bar.addEventListener("click", () => onSelect(entry));
    bar.addEventListener("pointerenter", () => onHover(entry));
    bar.addEventListener("pointerleave", () => onHover(null));
    gutterEl.append(bar);
    bars.set(entry.key, bar);
  }

  function applyHighlight(key) {
    const onPath = ancestorKeys(key, byKey);
    for (const [barKey, bar] of bars) {
      const selected = barKey === key;
      bar.classList.toggle("is-selected", selected);
      bar.classList.toggle("is-on-path", !selected && onPath.has(barKey));
    }
  }

  return { select(key) { applyHighlight(key); } };
}
