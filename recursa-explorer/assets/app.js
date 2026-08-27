"use strict";

import { getSchema, post } from "./api.js";
import { deepestContaining, parseHexSeed, utf8ByteAtUtf16 } from "./offsets.js";
import { occurrenceEntries } from "./model.js";
import { createTree } from "./tree.js";
import { renderDiagnostics, renderPretty, renderSchema, renderSourceOverlay, selectTextareaSpan } from "./views.js";

const elements = Object.fromEntries([
  ["status", "status"], ["source", "source"], ["sourceOverlay", "source-overlay"],
  ["diagnostics", "diagnostics"], ["tree", "tree"], ["treeEmpty", "tree-empty"],
  ["sourceContext", "source-context"],
  ["provenance", "provenance"], ["schema", "schema"], ["parseNow", "parse-now"],
  ["prettyPanel", "pretty-panel"], ["prettyRun", "pretty-run"], ["prettyWidth", "pretty-width"],
  ["prettyIndent", "pretty-indent"], ["prettyOutput", "pretty-output"], ["prettyApply", "pretty-apply"],
  ["samplePanel", "sample-panel"], ["sampleRun", "sample-run"], ["sampleSeed", "sample-seed"],
  ["sampleError", "sample-error"], ["shutdown", "shutdown"],
].map(([name, id]) => [name, document.getElementById(id)]));

const state = {
  revision: 0, controller: null, timer: null, schema: null, source: "", tokens: [], lexErrors: [],
  diagnostics: [], occurrence: null, treeView: null, selected: null, pretty: null, tokenClasses: new Map(),
  samples: new Map(),
};

function status(message) { elements.status.textContent = message; }

function clearDocument(message = "Parsing…") {
  state.occurrence = null; state.treeView = null; state.selected = null; state.diagnostics = [];
  elements.tree.replaceChildren(); elements.tree.hidden = true; elements.treeEmpty.hidden = false;
  elements.treeEmpty.textContent = message; elements.provenance.hidden = true;
  renderDiagnostics(elements.diagnostics, [], "", () => {});
}

function renderOverlay() {
  renderSourceOverlay(
    elements.sourceOverlay, state.source, state.tokens, state.lexErrors, state.diagnostics,
    state.selected?.span || null, state.tokenClasses,
  );
  elements.sourceOverlay.scrollTop = elements.source.scrollTop;
  elements.sourceOverlay.scrollLeft = elements.source.scrollLeft;
}

function selectOccurrence(entry, focusSource = false) {
  if (!entry) return;
  state.selected = entry; state.treeView?.select(entry.key); renderOverlay();
  if (state.pretty) renderPretty(elements.prettyOutput, state.pretty.rendered, state.pretty.sourceMap, entry.span);
  if (focusSource && entry.span) selectTextareaSpan(elements.source, state.source, entry.span);
}

function selectSourceOffset(utf16Offset) {
  if (!state.occurrence) return;
  const entry = deepestContaining(state.occurrence.entries, utf8ByteAtUtf16(state.source, utf16Offset));
  if (entry) selectOccurrence(entry);
}

function showDocument(documentValue) {
  state.occurrence = occurrenceEntries(documentValue.root);
  elements.tree.hidden = false; elements.treeEmpty.hidden = true;
  elements.provenance.hidden = false; elements.provenance.textContent = documentValue.provenance;
  state.treeView = createTree(elements.tree, state.occurrence, (entry, focusSource) => selectOccurrence(entry, focusSource));
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
      status(`Parsed ${state.tokens.length} token${state.tokens.length === 1 ? "" : "s"}`);
    } else {
      state.diagnostics = parsed.diagnostics || [];
      clearDocument("Invalid input has no partial AST.");
      state.diagnostics = parsed.diagnostics || [];
      status(`${state.diagnostics.length} diagnostic${state.diagnostics.length === 1 ? "" : "s"}`);
      renderDiagnostics(elements.diagnostics, state.diagnostics, state.source, (span) => {
        state.selected = { span }; renderOverlay(); selectTextareaSpan(elements.source, state.source, span);
      });
    }
    renderOverlay();
  } catch (error) {
    if (error.name !== "AbortError" && revision === state.revision) status(error.message);
  }
}

function scheduleParse(immediate = false) {
  state.source = elements.source.value;
  state.revision += 1; const revision = state.revision;
  state.controller?.abort(); clearTimeout(state.timer); state.controller = new AbortController();
  state.tokens = []; state.lexErrors = []; state.pretty = null; elements.prettyOutput.textContent = "";
  elements.sourceContext.hidden = true;
  elements.prettyApply.disabled = true; clearDocument(); renderOverlay(); status("Parsing…");
  state.timer = setTimeout(() => parseRevision(revision, state.controller.signal), immediate ? 0 : 140);
}

async function runPretty() {
  const revision = state.revision;
  state.controller?.abort();
  const controller = new AbortController(); state.controller = controller;
  try {
    const result = await post("pretty", {
      source: state.source,
      max_width: Number(elements.prettyWidth.value),
      indent: Number(elements.prettyIndent.value),
    }, controller.signal);
    if (revision !== state.revision) return;
    if (result.status !== "ok") { status(result.message || "Pretty is unavailable for this input."); return; }
    state.pretty = result; elements.prettyApply.disabled = false;
    renderPretty(elements.prettyOutput, result.rendered, result.sourceMap, state.selected?.span || null);
    status("Pretty output is ready.");
  } catch (error) {
    if (error.name !== "AbortError" && revision === state.revision) status(error.message);
  }
}

async function runSample() {
  elements.sampleError.textContent = "";
  const seed = elements.sampleSeed.value.replace(/\s+/gu, "").toLowerCase();
  let bytes;
  try { bytes = parseHexSeed(seed); }
  catch (error) { elements.sampleError.textContent = error.message; return; }
  const revision = ++state.revision; clearTimeout(state.timer); state.controller?.abort(); state.controller = new AbortController();
  clearDocument("Generating deterministic sample…");
  try {
    const result = await post("sample", { bytes }, state.controller.signal);
    if (revision !== state.revision) return;
    if (result.status === "ok") {
      const signature = JSON.stringify(result.document);
      const repeated = state.samples.get(seed) === signature; state.samples.set(seed, signature);
      showDocument(result.document); elements.sourceContext.hidden = false;
      status(repeated ? "Same seed reproduced the identical synthetic tree." : `Synthetic sample from ${bytes.length} seed bytes.`);
    } else {
      elements.sampleError.textContent = result.message || "Sample generation is unavailable.";
      status("Sample generation failed.");
    }
  } catch (error) {
    if (error.name !== "AbortError" && revision === state.revision) elements.sampleError.textContent = error.message;
  }
}

async function start() {
  try {
    state.schema = await getSchema(); renderSchema(elements.schema, state.schema);
    state.tokenClasses = new Map(state.schema.tokens.map((token) => [token.kind, token.highlightClass]));
    elements.prettyPanel.hidden = !state.schema.capabilities.pretty;
    elements.samplePanel.hidden = !state.schema.capabilities.sample;
    status(`Ready · ${state.schema.root.name}`); renderOverlay();
  } catch (error) { status(error.message); }
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
elements.prettyRun.addEventListener("click", runPretty);
elements.prettyApply.addEventListener("click", () => {
  if (!state.pretty) return; elements.source.value = state.pretty.rendered; scheduleParse(true); elements.source.focus();
});
function selectPrettyTarget(target) {
  if (!state.pretty) return;
  const mapped = target.closest("span[data-source-start]");
  if (!mapped || !state.occurrence) return;
  const span = { start: Number(mapped.dataset.sourceStart), end: Number(mapped.dataset.sourceEnd) };
  const entry = deepestContaining(state.occurrence.entries, span.start); if (entry) selectOccurrence(entry);
}
elements.prettyOutput.addEventListener("pointermove", (event) => selectPrettyTarget(event.target));
elements.prettyOutput.addEventListener("focusin", (event) => selectPrettyTarget(event.target));
elements.sampleRun.addEventListener("click", runSample);
elements.sampleSeed.addEventListener("input", () => {
  state.revision += 1; state.controller?.abort(); elements.sampleError.textContent = "";
  if (!elements.provenance.hidden && elements.provenance.textContent === "Synthetic") {
    clearDocument("Seed changed; generate a new sample.");
  }
  status("Sample seed changed.");
});
elements.shutdown.addEventListener("click", async () => {
  elements.shutdown.disabled = true; status("Stopping…");
  try { await post("shutdown", {}); status("Explorer stopped. This tab can be closed."); }
  catch (error) { status(error.message); elements.shutdown.disabled = false; }
});

start();
