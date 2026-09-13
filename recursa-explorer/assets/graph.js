"use strict";

import { createPanZoom, fin } from "./panzoom.js";
import { displayName } from "./model.js";

const ROW_GAP = 172;
const DEPTH_GAP = 46;
const ORIGIN = 20;
const CHAR_W = 7.35;
const META_CHAR_W = 5.85;

function pillMeta(entry) {
  return entry.kind === "element" ? `${entry.role} · ${entry.cardinality}` : "";
}

function pillSize(entry) {
  const nameLength = (displayName(entry)?.length || 0) + (entry.variant?.length || 0) + 2;
  const meta = pillMeta(entry);
  const width = fin(Math.max(nameLength * CHAR_W, meta.length * META_CHAR_W) + 22, 60);
  const height = entry.kind === "element" ? 37 : 26;
  return { width, height };
}

/// Assigns each leaf a sequential horizontal slot (in document order), then gives every internal
/// occurrence the mean slot of its first and last child — the design's graph-placement algorithm.
function assignSlots(entries, byKey) {
  const slot = new Map();
  let nextLeaf = 0;
  for (const entry of entries) {
    if (!entry.children.length) { slot.set(entry.key, nextLeaf); nextLeaf += 1; }
  }
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry.children.length) {
      const childSlots = entry.children.map((key) => slot.get(key));
      slot.set(entry.key, fin((childSlots[0] + childSlots.at(-1)) / 2, 0));
    }
  }
  return slot;
}

function layout(model) {
  const byKey = new Map(model.entries.map((entry) => [entry.key, entry]));
  const slot = assignSlots(model.entries, byKey);
  const boxes = new Map();
  let maxX = 0;
  let maxY = 0;
  for (const entry of model.entries) {
    const { width, height } = pillSize(entry);
    const x = fin(ORIGIN + slot.get(entry.key) * ROW_GAP, ORIGIN);
    const y = ORIGIN + entry.depth * DEPTH_GAP;
    boxes.set(entry.key, { x, y, width, height });
    maxX = Math.max(maxX, x + width);
    maxY = Math.max(maxY, y + height);
  }
  return { byKey, boxes, contentWidth: maxX + 20, contentHeight: maxY + 20 };
}

function edgePath(parentBox, childBox) {
  const x1 = parentBox.x + parentBox.width / 2;
  const y1 = parentBox.y + parentBox.height;
  const x2 = childBox.x + childBox.width / 2;
  const y2 = childBox.y;
  const midY = fin((y1 + y2) / 2, y1);
  return `M ${x1},${y1} C ${x1},${midY} ${x2},${midY} ${x2},${y2}`;
}

function ancestorKeys(key, byKey) {
  const result = new Set();
  let cursor = key ? byKey.get(key) : null;
  while (cursor) { result.add(cursor.key); cursor = cursor.parentKey ? byKey.get(cursor.parentKey) : null; }
  return result;
}

/// Renders the Constellation mode: a pannable, zoomable node-and-edge graph over the flat
/// occurrence list, positioned by the design's leaf-slot / depth-band layout algorithm.
export function createGraph(field, layer, edgesSvg, model, panState, onSelect, onZoomChange) {
  layer.querySelectorAll(".graph-pill").forEach((pill) => pill.remove());
  edgesSvg.querySelectorAll("path").forEach((path) => path.remove());

  const { byKey, boxes, contentWidth, contentHeight } = layout(model);
  edgesSvg.setAttribute("width", String(contentWidth));
  edgesSvg.setAttribute("height", String(contentHeight));
  layer.style.width = `${contentWidth}px`;
  layer.style.height = `${contentHeight}px`;

  const pills = new Map();
  for (const entry of model.entries) {
    const box = boxes.get(entry.key);
    if (entry.parentKey) {
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", edgePath(boxes.get(entry.parentKey), box));
      path.setAttribute("class", "graph-edge");
      path.dataset.childKey = entry.key;
      path.dataset.parentKey = entry.parentKey;
      edgesSvg.append(path);
    }
    const pill = document.createElement("div");
    pill.className = `graph-pill graph-pill-${entry.kind}`;
    pill.dataset.key = entry.key;
    pill.style.left = `${box.x}px`;
    pill.style.top = `${box.y}px`;
    pill.style.width = `${box.width}px`;
    pill.style.height = `${box.height}px`;
    const name = document.createElement("span");
    name.className = "graph-pill-name";
    name.textContent = displayName(entry) || "";
    if (entry.kind === "node" && entry.variant) {
      const variant = document.createElement("span");
      variant.className = "graph-pill-variant";
      variant.textContent = `::${entry.variant}`;
      name.append(variant);
    }
    pill.append(name);
    const meta = pillMeta(entry);
    if (meta) {
      const metaEl = document.createElement("span");
      metaEl.className = "graph-pill-meta";
      metaEl.textContent = meta;
      pill.append(metaEl);
    }
    pill.addEventListener("click", () => onSelect(entry));
    layer.append(pill);
    pills.set(entry.key, pill);
  }

  const panZoom = createPanZoom(field, layer, panState, onZoomChange);

  function applyHighlight(key) {
    const onPath = ancestorKeys(key, byKey);
    for (const [pillKey, pill] of pills) {
      const selected = pillKey === key;
      pill.classList.toggle("is-selected", selected);
      pill.style.opacity = selected || onPath.has(pillKey) ? "1" : ".5";
    }
    for (const path of edgesSvg.querySelectorAll(".graph-edge")) {
      const onSelectedPath = onPath.has(path.dataset.childKey) && onPath.has(path.dataset.parentKey);
      path.classList.toggle("is-on-path", onSelectedPath);
    }
  }

  return {
    select(key) { applyHighlight(key); },
    fit() { panZoom.fit(contentWidth, contentHeight); },
    zoomIn: panZoom.zoomIn,
    zoomOut: panZoom.zoomOut,
    destroy: panZoom.destroy,
  };
}
