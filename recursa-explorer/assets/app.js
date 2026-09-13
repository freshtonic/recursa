"use strict";

import { getSchema, post } from "./api.js";
import { deepestContaining, utf8ByteAtUtf16 } from "./offsets.js";
import { occurrenceEntries, spanText } from "./model.js";
import { createTree } from "./tree.js";
import { createGraph } from "./graph.js";
import { createGrain } from "./grain.js";
import { createStrata } from "./strata.js";
import { highlightTakenPath } from "./railroad-path.js";
import {
  renderDiagnostics, renderSourceOverlay, renderGutter, selectTextareaSpan,
  renderBreadcrumb, renderOccurrenceFooter,
} from "./views.js";

const MODE_LABELS = { graph: "Constellation", outline: "Outline", grain: "Grain", strata: "Strata" };
const PAN_ZOOM_MODES = new Set(["graph"]);

const elements = Object.fromEntries([
  ["source", "source"], ["sourceOverlay", "source-overlay"], ["sourceGutter", "source-gutter"],
  ["diagnostics", "diagnostics"], ["tree", "tree"], ["treeEmpty", "tree-empty"],
  ["outlineSurface", "outline-surface"], ["shapeStripBars", "shape-strip-bars"],
  ["parseNow", "parse-now"], ["shuffleSample", "shuffle-sample"], ["prettyRun", "pretty-run"],
  ["shutdown", "shutdown"],
  ["grammarIdentity", "grammar-identity"], ["parseStatusText", "parse-status"],
  ["structureHeading", "structure-heading"],
  ["occurrenceCount", "occurrence-count"], ["breadcrumb", "breadcrumb"],
  ["foldControls", "fold-controls"], ["foldToggle", "fold-toggle"], ["expandAll", "expand-all"],
  ["zoomControls", "zoom-controls"], ["zoomLevel", "zoom-level"],
  ["zoomOut", "zoom-out"], ["zoomIn", "zoom-in"], ["zoomFit", "zoom-fit"],
  ["selectionSpan", "selection-span"], ["occurrenceFooter", "occurrence-footer"],
  ["modeSwitcher", "mode-switcher"],
  ["graphSurface", "graph-surface"], ["graphLayer", "graph-layer"], ["graphEdges", "graph-edges"],
  ["grainSurface", "grain-surface"], ["grainGutter", "grain-gutter"], ["grainSource", "grain-source"],
  ["grainCaption", "grain-caption"], ["grainCaptionTitle", "grain-caption-title"], ["grainCaptionMeta", "grain-caption-meta"],
  ["strataSurface", "strata-surface"], ["strataTrack", "strata-track"], ["strataHover", "strata-hover"], ["strataTotal", "strata-total"],
  ["workspace", "workspace"], ["columnResizer", "column-resizer"],
  ["rowResizer", "row-resizer"], ["bottomPanel", "bottom-panel"], ["railroadPanel", "railroad-panel"],
].map(([name, id]) => [name, document.getElementById(id)]));

const SURFACES = {
  outline: elements.outlineSurface, graph: elements.graphSurface,
  grain: elements.grainSurface, strata: elements.strataSurface,
};

const state = {
  revision: 0, controller: null, timer: null, schema: null, source: "", tokens: [], lexErrors: [],
  diagnostics: [], occurrence: null, byKey: new Map(), treeView: null, selected: null,
  tokenClasses: new Map(), fold: true, mode: "graph", bottomPanel: "occurrence",
  panState: { graph: { x: 0, y: 0, z: 1 } },
  modeViews: { graph: null, grain: null, strata: null },
  railroads: new Map(),
};

function grammarStatus(message) { elements.grammarIdentity.textContent = message; }
function parseStatus(message) { elements.parseStatusText.textContent = message; }

function formatBytes(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function totalBytes() {
  return state.occurrence?.entries[0]?.span?.end ?? new TextEncoder().encode(state.source).length;
}

function maxDepth() {
  return state.occurrence ? state.occurrence.entries.reduce((max, entry) => Math.max(max, entry.depth), 0) : 0;
}

function destroyModeViews() {
  state.modeViews.graph?.destroy?.();
  state.modeViews = { graph: null, grain: null, strata: null };
  elements.grainCaption.hidden = true; elements.strataHover.textContent = "";
}

function clearDocument(message = "Parsing…") {
  state.occurrence = null; state.byKey = new Map(); state.treeView = null; state.selected = null; state.diagnostics = [];
  elements.tree.replaceChildren(); elements.shapeStripBars.replaceChildren();
  destroyModeViews();
  for (const surface of Object.values(SURFACES)) surface.hidden = true;
  elements.treeEmpty.hidden = false;
  elements.treeEmpty.textContent = message;
  elements.occurrenceCount.textContent = ""; elements.breadcrumb.replaceChildren();
  elements.selectionSpan.textContent = ""; elements.occurrenceFooter.replaceChildren();
  renderDiagnostics(elements.diagnostics, [], "", () => {});
}

function renderOverlay() {
  renderSourceOverlay(
    elements.sourceOverlay, state.source, state.tokens, state.lexErrors, state.diagnostics,
    state.selected?.span || null, state.tokenClasses,
  );
  elements.sourceOverlay.scrollTop = elements.source.scrollTop;
  elements.sourceOverlay.scrollLeft = elements.source.scrollLeft;
  elements.sourceGutter.scrollTop = elements.source.scrollTop;
  renderSourceOverlay(
    elements.grainSource, state.source, state.tokens, state.lexErrors, state.diagnostics,
    state.selected?.span || null, state.tokenClasses,
  );
}

function updateOccurrenceCount() {
  if (!state.occurrence) { elements.occurrenceCount.textContent = ""; return; }
  const parts = [`${state.occurrence.entries.length} occurrences`];
  if (state.mode === "outline" && state.treeView) parts.push(`${state.treeView.rowCount} rows`);
  parts.push(`depth ${maxDepth()}`);
  elements.occurrenceCount.textContent = parts.join(" · ");
}

function updateZoomLevel(panState) {
  elements.zoomLevel.textContent = `${Math.round(panState.z * 100)}%`;
}

function handleGrainHover(entry) {
  if (!entry) { elements.grainCaption.hidden = true; return; }
  elements.grainCaption.hidden = false;
  elements.grainCaptionTitle.textContent = entry.kind === "node" && entry.variant ? `${entry.name}::${entry.variant}` : entry.name;
  const parts = entry.kind === "element" ? [entry.role, entry.cardinality] : [];
  parts.push(`bytes ${spanText(entry.span)}`);
  elements.grainCaptionMeta.textContent = parts.join(" · ");
}

function handleStrataHover(entry) {
  elements.strataHover.textContent = entry ? (entry.kind === "node" && entry.variant ? `${entry.name}::${entry.variant}` : entry.name) : "";
}

/// Lazily builds (and caches, per current document) the view for `mode`, so switching modes never
/// loses pan/zoom state and never rebuilds a mode that hasn't changed.
function buildModeView(mode) {
  if (mode === "outline") {
    if (!state.treeView) renderTree(false);
    return state.treeView;
  }
  if (state.modeViews[mode]) return state.modeViews[mode];
  let view = null;
  if (mode === "graph") {
    view = createGraph(
      elements.graphSurface, elements.graphLayer, elements.graphEdges, state.occurrence,
      state.panState.graph, (entry) => selectOccurrence(entry), updateZoomLevel,
    );
    view.fit();
  } else if (mode === "grain") {
    view = createGrain(elements.grainGutter, state.occurrence, state.source, (entry) => selectOccurrence(entry), handleGrainHover);
  } else if (mode === "strata") {
    view = createStrata(elements.strataTrack, state.occurrence, totalBytes(), (entry) => selectOccurrence(entry), handleStrataHover);
    elements.strataTotal.textContent = `byte ${totalBytes()}`;
  }
  state.modeViews[mode] = view;
  return view;
}

function renderActiveMode() {
  for (const [mode, button] of modeButtons()) button.classList.toggle("is-active", mode === state.mode);
  elements.structureHeading.textContent = MODE_LABELS[state.mode];
  elements.foldControls.hidden = state.mode !== "outline";
  elements.zoomControls.hidden = !PAN_ZOOM_MODES.has(state.mode);
  updateOccurrenceCount();
  if (!state.occurrence) return;
  for (const [mode, surface] of Object.entries(SURFACES)) surface.hidden = mode !== state.mode;
  const view = buildModeView(state.mode);
  if (PAN_ZOOM_MODES.has(state.mode)) updateZoomLevel(state.panState[state.mode]);
  if (state.selected) view?.select(state.selected.key);
}

function modeButtons() {
  return [...elements.modeSwitcher.querySelectorAll(".mode-option")].map((button) => [button.dataset.mode, button]);
}

function renderTree(expandAllOnce) {
  if (!state.occurrence) return;
  state.treeView = createTree(
    elements.tree, elements.shapeStripBars, state.occurrence,
    { fold: state.fold, expandAll: expandAllOnce },
    (entry) => selectOccurrence(entry),
  );
  updateOccurrenceCount();
}

function selectOccurrence(entry, focusSource = false) {
  if (!entry) return;
  state.selected = entry;
  state.treeView?.select(entry.key);
  for (const view of Object.values(state.modeViews)) view?.select(entry.key);
  updateOccurrenceCount();
  renderOverlay();
  renderBreadcrumb(elements.breadcrumb, state.occurrence?.entries || [], state.byKey, entry.key, (crumbEntry) => selectOccurrence(crumbEntry));
  renderOccurrenceFooter(elements.occurrenceFooter, entry);
  elements.selectionSpan.textContent = spanText(entry.span);
  if (state.bottomPanel === "railroad") renderRailroadPanel(entry);
  if (focusSource && entry.span) selectTextareaSpan(elements.source, state.source, entry.span);
}

function selectSourceOffset(utf16Offset) {
  if (!state.occurrence) return;
  const entry = deepestContaining(state.occurrence.entries, utf8ByteAtUtf16(state.source, utf16Offset));
  if (entry) selectOccurrence(entry);
}

function showDocument(documentValue) {
  state.occurrence = occurrenceEntries(documentValue.root);
  state.byKey = new Map(state.occurrence.entries.map((entry) => [entry.key, entry]));
  elements.treeEmpty.hidden = true;
  renderActiveMode();
  selectOccurrence(state.occurrence.entries[0]);
}

async function parseRevision(revision, signal) {
  try {
    const [lexed, parsed] = await Promise.all([
      post("lex", { source: state.source }, signal),
      post("parse", { source: state.source }, signal),
    ]);
    if (revision !== state.revision) return;
    state.tokens = lexed.tokens; state.lexErrors = lexed.errors;
    if (parsed.status === "ok") {
      state.diagnostics = [];
      showDocument(parsed.document);
      const ms = (parsed.statistics.elapsedMicros / 1000).toFixed(2);
      const tokenCount = state.tokens.length;
      const memory = parsed.statistics.memoryBytes != null ? ` · ${formatBytes(parsed.statistics.memoryBytes)} AST` : "";
      parseStatus(`parsed · ${ms} ms · ${tokenCount} token${tokenCount === 1 ? "" : "s"}${memory}`);
    } else {
      state.diagnostics = parsed.diagnostics || [];
      clearDocument("Invalid input has no partial AST.");
      state.diagnostics = parsed.diagnostics || [];
      parseStatus(`${state.diagnostics.length} diagnostic${state.diagnostics.length === 1 ? "" : "s"}`);
      renderDiagnostics(elements.diagnostics, state.diagnostics, state.source, (span) => {
        state.selected = { span }; renderOverlay(); selectTextareaSpan(elements.source, state.source, span);
      });
    }
    renderOverlay();
  } catch (error) {
    if (error.name !== "AbortError" && revision === state.revision) parseStatus(error.message);
  }
}

function scheduleParse(immediate = false) {
  state.source = elements.source.value;
  state.revision += 1; const revision = state.revision;
  state.controller?.abort(); clearTimeout(state.timer); state.controller = new AbortController();
  state.tokens = []; state.lexErrors = [];
  clearDocument(); renderOverlay(); renderGutter(elements.sourceGutter, state.source);
  parseStatus("Parsing…");
  state.timer = setTimeout(() => parseRevision(revision, state.controller.signal), immediate ? 0 : 140);
}

async function start() {
  try {
    state.schema = await getSchema();
    state.tokenClasses = new Map(state.schema.tokens.map((token) => [token.kind, token.highlightClass]));
    elements.shuffleSample.hidden = !state.schema.capabilities.sample;
    elements.prettyRun.hidden = !state.schema.capabilities.pretty;
    grammarStatus(`${state.schema.name} · root ${state.schema.root.qualifiedName}`);
    parseStatus("Ready");
    renderOverlay(); renderGutter(elements.sourceGutter, state.source);
    renderActiveMode();
  } catch (error) { grammarStatus(error.message); }
}

/// Generates a fresh random statement via the grammar's Arbitrary capability, renders it to
/// source text in an isolated sample-worker subprocess, and loads it exactly like Pretty output
/// — reparsed through the ordinary strict parser, so the result carries genuine spans rather
/// than a synthetic document's provenance. Isolating each attempt in its own subprocess means a
/// worker that overflows its stack only takes down that attempt; a failed attempt (a crash or a
/// graceful generation error) is retried with fresh entropy up to MAX_SHUFFLE_ATTEMPTS times.
const MAX_SHUFFLE_ATTEMPTS = 10;

async function runShuffle() {
  const revision = state.revision;
  state.controller?.abort();
  const controller = new AbortController(); state.controller = controller;
  for (let attempt = 1; attempt <= MAX_SHUFFLE_ATTEMPTS; attempt += 1) {
    parseStatus(
      attempt === 1
        ? "Generating a random statement…"
        : `Generating a random statement… (retry ${attempt}/${MAX_SHUFFLE_ATTEMPTS})`,
    );
    const bytes = new Uint8Array(4096);
    crypto.getRandomValues(bytes);
    try {
      const result = await post("sample", { bytes: Array.from(bytes) }, controller.signal);
      if (revision !== state.revision) return;
      if (result.status === "ok") {
        elements.source.value = result.rendered;
        scheduleParse(true);
        return;
      }
      if (result.status === "unsupported") {
        parseStatus(result.message || "Sample generation is unavailable.");
        return;
      }
      // result.status === "error": the sample worker failed (possibly a stack overflow);
      // fall through and retry with fresh entropy.
    } catch (error) {
      if (error.name === "AbortError" || revision !== state.revision) return;
      // Network/transport failure: fall through and retry too.
    }
  }
  parseStatus(`Random generation failed after ${MAX_SHUFFLE_ATTEMPTS} attempts.`);
}

/// Replaces the editor's text with its Pretty-rendered form, then reparses it as ordinary source.
async function runPretty() {
  const revision = state.revision;
  state.controller?.abort();
  const controller = new AbortController(); state.controller = controller;
  parseStatus("Rendering…");
  try {
    const result = await post("pretty", { source: elements.source.value, max_width: 80, indent: 2 }, controller.signal);
    if (revision !== state.revision) return;
    if (result.status !== "ok") { parseStatus(result.message || "Pretty is unavailable for this input."); return; }
    elements.source.value = result.rendered;
    scheduleParse(true);
  } catch (error) {
    if (error.name !== "AbortError" && revision === state.revision) parseStatus(error.message);
  }
}

/// Drag-to-resize the Source/Parsed-structure split, persisted across sessions.
const SOURCE_WIDTH_KEY = "recursa-explorer:source-width";
// The floor is what the column used to always start at, before it grew to a third of the
// workspace by default; the ceiling tracks the workspace's own width so it always allows resizing
// up to at least half the available area, rather than a fixed pixel cap that stops mattering on a
// wide window.
const MIN_SOURCE_WIDTH = 352;

function maxSourceWidth() {
  const workspaceWidth = elements.workspace.getBoundingClientRect().width || window.innerWidth;
  return Math.max(MIN_SOURCE_WIDTH, workspaceWidth / 2);
}

function setSourceWidth(width) {
  const clamped = Math.min(maxSourceWidth(), Math.max(MIN_SOURCE_WIDTH, width));
  elements.workspace.style.setProperty("--source-width", `${clamped}px`);
  return clamped;
}

function setupResizer() {
  const stored = Number(localStorage.getItem(SOURCE_WIDTH_KEY));
  const defaultWidth = Math.max(MIN_SOURCE_WIDTH, window.innerWidth / 3);
  setSourceWidth(Number.isFinite(stored) && stored > 0 ? stored : defaultWidth);

  let dragging = false;
  let startX = 0;
  let startWidth = 0;
  elements.columnResizer.addEventListener("pointerdown", (event) => {
    dragging = true;
    startX = event.clientX;
    startWidth = elements.source.closest(".source-column").getBoundingClientRect().width;
    elements.columnResizer.setPointerCapture(event.pointerId);
  });
  elements.columnResizer.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    setSourceWidth(startWidth + (event.clientX - startX));
  });
  elements.columnResizer.addEventListener("pointerup", () => {
    if (!dragging) return;
    dragging = false;
    const width = elements.source.closest(".source-column").getBoundingClientRect().width;
    localStorage.setItem(SOURCE_WIDTH_KEY, String(Math.round(width)));
  });
  elements.columnResizer.addEventListener("keydown", (event) => {
    const step = event.shiftKey ? 40 : 12;
    if (event.key === "ArrowLeft") {
      const width = setSourceWidth(elements.source.closest(".source-column").getBoundingClientRect().width - step);
      localStorage.setItem(SOURCE_WIDTH_KEY, String(Math.round(width)));
      event.preventDefault();
    } else if (event.key === "ArrowRight") {
      const width = setSourceWidth(elements.source.closest(".source-column").getBoundingClientRect().width + step);
      localStorage.setItem(SOURCE_WIDTH_KEY, String(Math.round(width)));
      event.preventDefault();
    }
  });
}

/// Drag-to-resize the bottom (Occurrence / Railroad diagram) panel's height.
const BOTTOM_HEIGHT_KEY = "recursa-explorer:bottom-height";
const MIN_BOTTOM_HEIGHT = 80;

function maxBottomHeight() {
  const columnHeight = elements.bottomPanel.closest(".structure-column")?.getBoundingClientRect().height
    || window.innerHeight;
  return Math.max(MIN_BOTTOM_HEIGHT, columnHeight / 2);
}

function setBottomHeight(height) {
  const clamped = Math.min(maxBottomHeight(), Math.max(MIN_BOTTOM_HEIGHT, height));
  elements.bottomPanel.style.setProperty("--bottom-height", `${clamped}px`);
  return clamped;
}

function setupRowResizer() {
  const stored = Number(localStorage.getItem(BOTTOM_HEIGHT_KEY));
  setBottomHeight(Number.isFinite(stored) && stored > 0 ? stored : 150);

  let dragging = false;
  let startY = 0;
  let startHeight = 0;
  elements.rowResizer.addEventListener("pointerdown", (event) => {
    dragging = true;
    startY = event.clientY;
    startHeight = elements.bottomPanel.getBoundingClientRect().height;
    elements.rowResizer.setPointerCapture(event.pointerId);
  });
  elements.rowResizer.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    // Dragging the handle up (negative deltaY) grows the panel below it.
    setBottomHeight(startHeight - (event.clientY - startY));
  });
  elements.rowResizer.addEventListener("pointerup", () => {
    if (!dragging) return;
    dragging = false;
    const height = elements.bottomPanel.getBoundingClientRect().height;
    localStorage.setItem(BOTTOM_HEIGHT_KEY, String(Math.round(height)));
  });
  elements.rowResizer.addEventListener("keydown", (event) => {
    const step = event.shiftKey ? 40 : 12;
    if (event.key === "ArrowDown") {
      const height = setBottomHeight(elements.bottomPanel.getBoundingClientRect().height - step);
      localStorage.setItem(BOTTOM_HEIGHT_KEY, String(Math.round(height)));
      event.preventDefault();
    } else if (event.key === "ArrowUp") {
      const height = setBottomHeight(elements.bottomPanel.getBoundingClientRect().height + step);
      localStorage.setItem(BOTTOM_HEIGHT_KEY, String(Math.round(height)));
      event.preventDefault();
    }
  });
}

/// Toggles the bottom panel between the Occurrence fields and the Railroad diagram.
function bottomTabButtons() {
  return [...elements.bottomPanel.querySelectorAll(".bottom-tabs .mode-option")].map(
    (button) => [button.dataset.panel, button],
  );
}

function setupBottomTabs() {
  for (const [panelName, button] of bottomTabButtons()) {
    button.addEventListener("click", () => {
      if (state.bottomPanel === panelName) return;
      state.bottomPanel = panelName;
      for (const [name, tab] of bottomTabButtons()) tab.classList.toggle("is-active", name === state.bottomPanel);
      elements.occurrenceFooter.hidden = state.bottomPanel !== "occurrence";
      elements.railroadPanel.hidden = state.bottomPanel !== "railroad";
      if (state.bottomPanel === "railroad" && state.selected) renderRailroadPanel(state.selected);
    });
  }
}

/// Parses one SVG document string into a real `<svg>` element via the DOM parser, not raw markup
/// assignment. Strips the railroad crate's embedded `<style>` elements and `style="..."`
/// attributes from the source text before parsing — even a detached, unattached document builds
/// their stylesheets at parse time, which the explorer's CSP (style-src 'self', no inline styles)
/// blocks and reports regardless of whether the parsed nodes are later inserted. `style.css`
/// carries the equivalent Nocturne-colored rules for the same selectors.
function parseSvg(source) {
  const cleaned = source
    .replace(/<style[^>]*>[\s\S]*?<\/style>/g, "")
    .replace(/\s+style="[^"]*"/g, "");
  const doc = new DOMParser().parseFromString(cleaned, "image/svg+xml");
  return doc.documentElement;
}

/// Renders (and caches, per document) the railroad diagram for `entry`'s Node type.
async function renderRailroadPanel(entry) {
  if (entry.kind !== "node") {
    elements.railroadPanel.replaceChildren();
    const empty = document.createElement("p");
    empty.className = "railroad-empty";
    empty.textContent = "Select a node occurrence to see its railroad diagram.";
    elements.railroadPanel.append(empty);
    return;
  }
  const qualifiedName = entry.raw?.schema?.qualifiedName;
  if (!qualifiedName) return;
  const requestKey = entry.key;
  let result = state.railroads.get(qualifiedName);
  if (!result) {
    try {
      result = await post("railroad", { qualified_name: qualifiedName });
    } catch (error) {
      result = { status: "error", message: error.message };
    }
    state.railroads.set(qualifiedName, result);
  }
  // Selection may have moved on while the request was in flight.
  if (state.selected?.key !== requestKey || state.bottomPanel !== "railroad") return;
  elements.railroadPanel.replaceChildren();
  const title = document.createElement("p");
  title.className = "railroad-title";
  title.textContent = qualifiedName;
  elements.railroadPanel.append(title);
  if (result.status !== "ok") {
    const empty = document.createElement("p");
    empty.className = "railroad-empty";
    empty.textContent = "No railroad diagram is available for this grammar.";
    elements.railroadPanel.append(empty);
    return;
  }
  const svg = parseSvg(result.svg);
  svg.classList.add("railroad-svg");
  if (entry.raw) highlightTakenPath(svg, entry.raw);
  elements.railroadPanel.append(svg);
}

elements.source.addEventListener("input", () => scheduleParse());
elements.source.addEventListener("select", () => selectSourceOffset(elements.source.selectionStart));
elements.source.addEventListener("scroll", renderOverlay);
elements.source.addEventListener("pointermove", (event) => {
  elements.source.style.pointerEvents = "none";
  elements.sourceOverlay.style.pointerEvents = "auto";
  const position = document.caretPositionFromPoint?.(event.clientX, event.clientY);
  const legacy = position ? null : document.caretRangeFromPoint?.(event.clientX, event.clientY);
  elements.source.style.pointerEvents = "";
  elements.sourceOverlay.style.pointerEvents = "";
  const node = position?.offsetNode || legacy?.startContainer;
  const offset = position?.offset ?? legacy?.startOffset;
  if (!node || offset == null || !elements.sourceOverlay.contains(node)) return;
  const range = document.createRange(); range.setStart(elements.sourceOverlay, 0); range.setEnd(node, offset);
  selectSourceOffset(range.toString().length);
});
elements.parseNow.addEventListener("click", () => scheduleParse(true));
elements.shuffleSample.addEventListener("click", runShuffle);
elements.prettyRun.addEventListener("click", runPretty);
elements.foldToggle.addEventListener("click", () => {
  state.fold = !state.fold;
  elements.foldToggle.classList.toggle("is-on", state.fold);
  elements.foldToggle.setAttribute("aria-pressed", String(state.fold));
  renderTree(false);
  if (state.selected) selectOccurrence(state.selected);
});
elements.expandAll.addEventListener("click", () => {
  renderTree(true);
  if (state.selected) selectOccurrence(state.selected);
});
for (const [mode, button] of modeButtons()) {
  button.addEventListener("click", () => {
    if (state.mode === mode) return;
    state.mode = mode;
    renderActiveMode();
  });
}
elements.zoomOut.addEventListener("click", () => state.modeViews[state.mode]?.zoomOut());
elements.zoomIn.addEventListener("click", () => state.modeViews[state.mode]?.zoomIn());
elements.zoomFit.addEventListener("click", () => state.modeViews[state.mode]?.fit());
elements.shutdown.addEventListener("click", async () => {
  elements.shutdown.disabled = true; parseStatus("Stopping…");
  try { await post("shutdown", {}); parseStatus("Explorer stopped. This tab can be closed."); }
  catch (error) { parseStatus(error.message); elements.shutdown.disabled = false; }
});

setupResizer();
setupRowResizer();
setupBottomTabs();
start();
