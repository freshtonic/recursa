"use strict";

/// Highlights, within a just-rendered railroad `<svg>`, the specific path through the diagram
/// that `rawNode` (the raw server JSON for the currently selected occurrence, i.e. `entry.raw`)
/// actually took: which Choice branch its variant selected, whether an Optional element was
/// present, how many times a Repeat iterated, and which Node application an inlined reference
/// expands to. Walks the SVG's own structural classes (`.choice`/`.sequence`/`.optional`/
/// `.repeat`/`.terminal`/`.nonterminal`/`.labeledbox`) in lockstep with the occurrence JSON, since
/// both are independently derived from the same declared element/variant order and — for
/// Choice specifically — the branch's own `<text class="comment">` label names the variant
/// directly, so branch matching doesn't even depend on that order lining up.
///
/// Best-effort throughout: wherever the SVG's shape and the occurrence data stop lining up (a
/// mismatch this function doesn't recognize), it simply stops marking further, leaving the rest
/// of the diagram unhighlighted rather than guessing.
export function highlightTakenPath(svg, rawNode) {
  const top = topLevelProductionGroup(svg);
  if (top && rawNode) matchNodeProduction(top, rawNode);
}

function directGroups(element) {
  return [...element.children].filter((child) => child.tagName === "g");
}

function directPaths(element) {
  return [...element.children].filter((child) => child.tagName === "path");
}

/// Picks the one child that carries real content out of a `.sequence` wrapper — either the sole
/// non-`<text>` child left after a `Comment`-only wrapper (the `0+`/`1+`/`optional 1+` cardinality
/// wrappers), or the one non-terminal child sitting between attached fixed tokens (an attachment
/// unit, e.g. a field wrapped `(this)` — `diagram_attachment_unit` in `recursa-codegen` always
/// emits `[beforeTerminals..., content, afterTerminals...]`, so "grab the last child" (which only
/// happens to work for the former, comment-only case) silently grabs a closing terminal instead of
/// the content whenever fixed tokens surround it on both sides).
function contentGroup(groups) {
  return groups.find((group) => !group.classList.contains("terminal")) || groups[groups.length - 1] || null;
}

function mark(element) {
  if (element) element.classList.add("is-on-path");
}

function markDirectPaths(element) {
  for (const path of directPaths(element)) mark(path);
}

function topLevelProductionGroup(svg) {
  const outer = directGroups(svg)[0];
  if (!outer) return null;
  return directGroups(outer)[0] || null;
}

/// Matches one Node occurrence's production: a Choice of variants (find the branch whose comment
/// label names the active variant) or a plain element sequence.
function matchNodeProduction(svgGroup, rawNode) {
  mark(svgGroup);
  if (svgGroup.classList.contains("choice")) {
    for (const branch of directGroups(svgGroup)) {
      const label = branch.querySelector(":scope > text.comment");
      if (label && rawNode.activeVariant && label.textContent === rawNode.activeVariant) {
        mark(branch);
        const content = directGroups(branch)[0];
        if (content) matchElementSequence(content, rawNode.elements || []);
        return;
      }
    }
    return;
  }
  matchElementSequence(svgGroup, rawNode.elements || []);
}

/// Matches a source-order run of elements against the shape produced for it: the shape *is* the
/// one element's own shape when there is only one (the codegen collapses singleton sequences),
/// otherwise a `.sequence` group with one direct child per element, in the same order.
function matchElementSequence(svgGroup, rawElements) {
  if (!rawElements.length) return;
  mark(svgGroup);
  markDirectPaths(svgGroup);
  if (rawElements.length === 1) {
    matchElement(svgGroup, rawElements[0]);
    return;
  }
  const groups = directGroups(svgGroup);
  const count = Math.min(groups.length, rawElements.length);
  for (let index = 0; index < count; index += 1) matchElement(groups[index], rawElements[index]);
}

/// Matches one element occurrence against its shape, dispatching purely on the shape's own class
/// so cardinality-specific wrapping (the `0+`/`1+`/`optional 1+` comment + Optional/Repeat combos)
/// never needs to be predicted — only `rawElement.children.length` (presence/iteration count) does.
function matchElement(svgGroup, rawElement) {
  mark(svgGroup);
  const classes = svgGroup.classList;
  if (classes.contains("terminal") || classes.contains("nonterminal")) return;
  if (classes.contains("optional")) {
    markDirectPaths(svgGroup);
    if (!rawElement.children.length) return;
    const inner = directGroups(svgGroup)[0];
    if (inner) matchElement(inner, rawElement);
    return;
  }
  if (classes.contains("repeat")) {
    matchRepeat(svgGroup, rawElement);
    return;
  }
  if (classes.contains("labeledbox")) {
    matchLabeledBox(svgGroup, rawElement);
    return;
  }
  if (classes.contains("sequence")) {
    markDirectPaths(svgGroup);
    const content = contentGroup(directGroups(svgGroup));
    if (content) matchElement(content, rawElement);
    return;
  }
  if (classes.contains("annotated")) {
    const inner = directGroups(svgGroup)[0];
    if (inner) matchElement(inner, rawElement);
  }
}

/// A Repeat's direct `<g>` children are always `[separator, item]` (or just `[item]` when there is
/// no separator) — the separator only actually appears on the page when more than one iteration
/// occurred.
function matchRepeat(svgGroup, rawElement) {
  mark(svgGroup);
  markDirectPaths(svgGroup);
  const count = rawElement.children.length;
  if (!count) return;
  const groups = directGroups(svgGroup);
  const [separatorGroup, itemGroup] = groups.length >= 2 ? groups : [null, groups[0]];
  if (itemGroup) matchRepeatItem(itemGroup, rawElement.children[0]);
  if (separatorGroup && count > 1) mark(separatorGroup);
}

/// One repeated instance's shape — railroad draws the loop once regardless of iteration count, so
/// this always represents the *first* actual child occurrence. The item itself may carry attached
/// fixed tokens (e.g. a comma-separated list of `(expr)`), rendered the same `.sequence`-wrapped
/// way as any other attachment unit, so this unwraps that exactly like `matchElement` does.
function matchRepeatItem(svgGroup, rawChildNode) {
  mark(svgGroup);
  const classes = svgGroup.classList;
  if (classes.contains("labeledbox")) {
    matchLabeledBoxForNode(svgGroup, rawChildNode);
    return;
  }
  if (classes.contains("sequence")) {
    markDirectPaths(svgGroup);
    const content = contentGroup(directGroups(svgGroup));
    if (content) matchRepeatItem(content, rawChildNode);
  }
}

/// A single (`One`/`Optional`) element inline-expanded as a labeled box.
function matchLabeledBox(svgGroup, rawElement) {
  mark(svgGroup);
  if (!rawElement.children.length) return;
  matchLabeledBoxForNode(svgGroup, rawElement.children[0]);
}

function matchLabeledBoxForNode(svgGroup, rawChildNode) {
  mark(svgGroup);
  markDirectPaths(svgGroup);
  for (const group of directGroups(svgGroup)) matchNodeProduction(group, rawChildNode);
}
