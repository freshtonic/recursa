"use strict";

import { fin } from "./panzoom.js";

const DEPTH_GAP = 19;
const CELL_HEIGHT = 16;

function ancestorKeys(key, byKey) {
  const result = new Set();
  let cursor = key ? byKey.get(key) : null;
  while (cursor) { result.add(cursor.key); cursor = cursor.parentKey ? byKey.get(cursor.parentKey) : null; }
  return result;
}

/// Renders the Strata mode: every occurrence with a span at once, x by byte span and y by depth,
/// so the silhouette is the shape of the parse.
export function createStrata(trackEl, model, totalBytes, onSelect, onHover) {
  trackEl.replaceChildren();
  const byKey = new Map(model.entries.map((entry) => [entry.key, entry]));
  const maxDepth = model.entries.reduce((max, entry) => Math.max(max, entry.depth), 0);
  trackEl.style.height = `${(maxDepth + 1) * DEPTH_GAP + CELL_HEIGHT}px`;
  const trackWidth = Math.max(1, trackEl.getBoundingClientRect().width);
  const bytes = Math.max(1, totalBytes);

  const cells = new Map();
  for (const entry of model.entries) {
    if (!entry.span) continue;
    const left = fin((entry.span.start / bytes) * trackWidth, 0);
    const width = fin(Math.max(3, ((entry.span.end - entry.span.start) / bytes) * trackWidth - 1.5), 3);
    const cell = document.createElement("div");
    cell.className = `strata-cell strata-cell-${entry.kind}`;
    cell.style.left = `${left}px`;
    cell.style.top = `${entry.depth * DEPTH_GAP}px`;
    cell.style.width = `${width}px`;
    const tint = entry.kind === "node" ? Math.min(30, 5 + entry.depth * 1.4) : Math.min(16, 2 + entry.depth * 0.8);
    cell.style.background = `color-mix(in srgb, var(--color-accent) ${tint}%, transparent)`;
    cell.dataset.key = entry.key;

    const text = entry.kind === "node" && entry.variant ? `${entry.name}::${entry.variant}` : entry.name;
    if (width >= text.length * 6.2 + 12) {
      const label = document.createElement("span");
      label.className = "strata-label";
      label.textContent = text;
      cell.append(label);
    }

    cell.addEventListener("click", () => onSelect(entry));
    cell.addEventListener("pointerenter", () => onHover(entry));
    cell.addEventListener("pointerleave", () => onHover(null));
    trackEl.append(cell);
    cells.set(entry.key, cell);
  }

  function applyHighlight(key) {
    const onPath = ancestorKeys(key, byKey);
    for (const [cellKey, cell] of cells) {
      const selected = cellKey === key;
      cell.classList.toggle("is-selected", selected);
      cell.classList.toggle("is-on-path", !selected && onPath.has(cellKey));
    }
  }

  return { select(key) { applyHighlight(key); } };
}
