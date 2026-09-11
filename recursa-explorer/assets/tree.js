"use strict";

const ROW_INDENT_PX = 22;

/// Returns the run of entries starting at `entry` while each has exactly one child, per the
/// design's chain-folding rule. A single entry (no fold) is a chain of length one.
function chainOf(entry, byKey, fold) {
  const chain = [entry];
  if (!fold) return chain;
  let current = entry;
  while (current.children.length === 1) {
    current = byKey.get(current.children[0]);
    chain.push(current);
  }
  return chain;
}

/// The deepest element occurrence in a folded chain supplies the row's role and cardinality.
function deepestElement(chain) {
  for (let index = chain.length - 1; index >= 0; index -= 1) {
    if (chain[index].kind === "element") return chain[index];
  }
  return null;
}

/// Collects the key of `key` and every ancestor up to the root, inclusive.
function ancestorKeys(key, byKey) {
  const result = new Set();
  let cursor = key ? byKey.get(key) : null;
  while (cursor) {
    result.add(cursor.key);
    cursor = cursor.parentKey ? byKey.get(cursor.parentKey) : null;
  }
  return result;
}

function chainSegment(entry) {
  const span = document.createElement("span");
  span.dataset.key = entry.key;
  if (entry.kind === "element") {
    span.className = "outline-seg outline-seg-element";
    span.textContent = entry.name;
  } else {
    span.className = "outline-seg outline-seg-node";
    span.textContent = entry.name;
    if (entry.variant) {
      const variant = document.createElement("span");
      variant.className = "outline-variant";
      variant.textContent = `::${entry.variant}`;
      span.append(variant);
    }
  }
  return span;
}

/// Renders the folded-chain Outline mode: one row per chain, a shape strip beside it, both driven
/// by the flat occurrence model. Rebuilt wholesale on a new document, a fold toggle, or Expand all
/// — this is a dev tool, not a production render loop, so a full rebuild stays simple and correct.
export function createTree(element, shapeElement, model, options, onSelect) {
  const { fold, expandAll } = options;
  element.replaceChildren();
  shapeElement.replaceChildren();
  const byKey = new Map(model.entries.map((entry) => [entry.key, entry]));
  const rows = new Map();
  const expanders = new Map();
  let rowCount = 0;

  function build(headEntry, rowDepth, forceExpanded) {
    rowCount += 1;
    const chain = chainOf(headEntry, byKey, fold);
    const tail = chain[chain.length - 1];
    const roleElement = deepestElement(chain);
    const rowKey = tail.key;

    const item = document.createElement("li");
    item.className = "outline-item";
    const row = document.createElement("div");
    row.className = "outline-row";
    row.id = rowKey;
    row.role = "treeitem";
    row.tabIndex = -1;
    row.dataset.key = rowKey;
    row.dataset.chainKeys = chain.map((entry) => entry.key).join(",");
    row.setAttribute("aria-level", String(rowDepth + 1));
    row.style.position = "relative";

    for (let level = 0; level < rowDepth; level += 1) {
      const guide = document.createElement("span");
      guide.className = "outline-guide";
      guide.style.left = `${level * ROW_INDENT_PX + 9}px`;
      row.append(guide);
    }

    const indent = document.createElement("span");
    indent.className = "outline-indent";
    indent.style.width = `${rowDepth * ROW_INDENT_PX}px`;
    row.append(indent);

    const toggle = document.createElement("span");
    toggle.className = "outline-toggle";
    toggle.setAttribute("aria-hidden", "true");
    toggle.textContent = tail.children.length ? "▶" : "·";
    row.append(toggle);

    const label = document.createElement("span");
    label.className = "outline-label";
    chain.forEach((entry, index) => {
      if (index > 0) {
        const sep = document.createElement("span");
        sep.className = "outline-sep";
        sep.textContent = "›";
        label.append(sep);
      }
      label.append(chainSegment(entry));
    });
    row.append(label);

    if (tail.text != null && !tail.children.length) {
      const leaf = document.createElement("span");
      leaf.className = "outline-leaf";
      leaf.textContent = JSON.stringify(tail.text);
      row.append(leaf);
    }

    const leader = document.createElement("span");
    leader.className = "outline-leader";
    row.append(leader);

    if (roleElement) {
      const role = document.createElement("span");
      role.className = "outline-role";
      role.textContent = roleElement.role;
      row.append(role);
      const cardinality = document.createElement("span");
      cardinality.className = "outline-cardinality";
      cardinality.textContent = roleElement.cardinality;
      row.append(cardinality);
    }

    item.append(row);
    rows.set(rowKey, row);

    let group = null;
    const populate = () => {
      if (group || !tail.children.length) return;
      group = document.createElement("ul");
      group.className = "outline-group";
      group.role = "group";
      for (const key of tail.children) group.append(build(byKey.get(key), rowDepth + 1, expandAll));
      item.append(group);
    };
    const setExpanded = (value) => {
      if (!tail.children.length) return;
      populate();
      group.hidden = !value;
      row.setAttribute("aria-expanded", String(value));
      toggle.textContent = value ? "▼" : "▶";
    };
    expanders.set(rowKey, setExpanded);
    if (tail.children.length) {
      row.setAttribute("aria-expanded", String(forceExpanded));
      if (forceExpanded) populate();
    }
    if (forceExpanded) setExpanded(true);

    row.addEventListener("click", (event) => {
      const segment = event.target.closest("[data-key]");
      onSelect(byKey.get(segment ? segment.dataset.key : rowKey));
    });
    toggle.addEventListener("click", (event) => {
      event.stopPropagation();
      setExpanded(row.getAttribute("aria-expanded") !== "true");
    });

    for (const entry of chain) entryToRow.set(entry.key, rowKey);
    return item;
  }

  const entryToRow = new Map();
  const rootGroup = document.createElement("ul");
  rootGroup.className = "outline-group outline-root";
  rootGroup.role = "group";
  rootGroup.append(build(byKey.get(model.rootKey), 0, true));
  element.append(rootGroup);
  rows.get(entryToRow.get(model.rootKey)).tabIndex = 0;

  for (const entry of model.entries) {
    const bar = document.createElement("div");
    bar.className = `outline-shape-bar outline-shape-${entry.kind}`;
    bar.style.marginLeft = `${entry.depth * 2}px`;
    bar.style.width = `${Math.max(4, 40 - entry.depth * 2)}px`;
    bar.dataset.key = entry.key;
    bar.addEventListener("click", () => onSelect(entry));
    shapeElement.append(bar);
  }

  element.addEventListener("keydown", (event) => {
    const current = event.target.closest("[role=treeitem]");
    if (!current) return;
    const visible = [...element.querySelectorAll("[role=treeitem]")].filter((row) => row.offsetParent !== null);
    const index = visible.indexOf(current);
    let target = null;
    if (event.key === "ArrowDown") target = visible[index + 1];
    if (event.key === "ArrowUp") target = visible[index - 1];
    if (event.key === "Home") target = visible[0];
    if (event.key === "End") target = visible.at(-1);
    if (event.key === "ArrowRight") {
      if (current.getAttribute("aria-expanded") === "false") expanders.get(current.dataset.key)(true);
      else if (current.getAttribute("aria-expanded") === "true") target = visible[index + 1];
    }
    if (event.key === "ArrowLeft") {
      if (current.getAttribute("aria-expanded") === "true") expanders.get(current.dataset.key)(false);
      else {
        const parentEntry = byKey.get(current.dataset.key).parentKey;
        const parentRow = parentEntry ? entryToRow.get(parentEntry) : null;
        target = parentRow ? rows.get(parentRow) : null;
      }
    }
    if (event.key === "Enter" || event.key === " ") onSelect(byKey.get(current.dataset.key));
    if (target) { current.tabIndex = -1; target.tabIndex = 0; target.focus(); }
    if (target || ["ArrowLeft", "ArrowRight", "Enter", " "].includes(event.key)) event.preventDefault();
  });

  function applyHighlight(key) {
    const selectedRow = entryToRow.get(key);
    const onPath = ancestorKeys(key, byKey);
    for (const [rowKey, row] of rows) {
      const chainKeys = row.dataset.chainKeys.split(",");
      const isSelected = rowKey === selectedRow;
      const isOnPath = !isSelected && chainKeys.some((chainKey) => onPath.has(chainKey));
      row.classList.toggle("is-selected", isSelected);
      row.classList.toggle("is-on-path", isOnPath);
      for (const guide of row.querySelectorAll(".outline-guide")) {
        guide.classList.toggle("is-on-path", isSelected || isOnPath);
      }
      for (const segment of row.querySelectorAll("[data-key]")) {
        segment.classList.toggle("is-selected", segment.dataset.key === key);
      }
    }
    for (const bar of shapeElement.querySelectorAll(".outline-shape-bar")) {
      const barKey = bar.dataset.key;
      bar.classList.toggle("is-selected", barKey === key);
      bar.classList.toggle("is-on-path", barKey !== key && onPath.has(barKey));
    }
  }

  return {
    get rowCount() { return rowCount; },
    select(key) {
      let ancestorRow = entryToRow.get(byKey.get(key)?.parentKey);
      const seen = new Set();
      while (ancestorRow && !seen.has(ancestorRow)) {
        seen.add(ancestorRow);
        expanders.get(ancestorRow)?.(true);
        const parentEntry = byKey.get(ancestorRow).parentKey;
        ancestorRow = parentEntry ? entryToRow.get(parentEntry) : null;
      }
      applyHighlight(key);
      const row = rows.get(entryToRow.get(key));
      if (row) row.scrollIntoView({ block: "nearest" });
    },
  };
}
