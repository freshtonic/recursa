"use strict";

export function createTree(element, model, onSelect) {
  element.replaceChildren();
  const byKey = new Map(model.entries.map((entry) => [entry.key, entry]));
  const rows = new Map();
  const expanders = new Map();

  function build(entry, expanded) {
    const item = document.createElement("li");
    const row = document.createElement("div");
    row.className = "tree-row"; row.id = entry.key; row.role = "treeitem"; row.tabIndex = -1;
    row.dataset.key = entry.key; row.setAttribute("aria-level", String(entry.depth + 1));
    const toggle = document.createElement("span");
    toggle.className = "tree-toggle"; toggle.setAttribute("aria-hidden", "true");
    toggle.textContent = entry.children.length ? (expanded ? "−" : "+") : "·";
    const kind = document.createElement("span"); kind.className = "tree-kind"; kind.textContent = entry.type;
    const label = document.createElement("span"); label.textContent = entry.label;
    const meta = document.createElement("span"); meta.className = "tree-meta"; meta.textContent = entry.meta;
    row.append(toggle, kind, label, meta); item.append(row); rows.set(entry.key, row);
    let group = null;
    const populate = () => {
      if (group || !entry.children.length) return;
      group = document.createElement("ul"); group.className = "tree-group"; group.role = "group";
      for (const key of entry.children) group.append(build(byKey.get(key), false));
      item.append(group);
    };
    const setExpanded = (value) => {
      if (!entry.children.length) return;
      populate(); group.hidden = !value; row.setAttribute("aria-expanded", String(value));
      toggle.textContent = value ? "−" : "+";
    };
    expanders.set(entry.key, setExpanded);
    if (entry.children.length) { row.setAttribute("aria-expanded", String(expanded)); if (expanded) { populate(); } }
    row.addEventListener("click", () => onSelect(entry, false));
    row.addEventListener("dblclick", () => setExpanded(row.getAttribute("aria-expanded") !== "true"));
    row.addEventListener("pointerenter", () => onSelect(entry, false));
    if (expanded) setExpanded(true);
    return item;
  }

  const rootGroup = document.createElement("ul"); rootGroup.className = "tree-group"; rootGroup.role = "group";
  rootGroup.append(build(byKey.get(model.rootKey), true)); element.append(rootGroup);
  rows.get(model.rootKey).tabIndex = 0;

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
        const parent = byKey.get(current.dataset.key).parentKey;
        target = parent ? rows.get(parent) : null;
      }
    }
    if (event.key === "Enter" || event.key === " ") onSelect(byKey.get(current.dataset.key), false);
    if (target) { current.tabIndex = -1; target.tabIndex = 0; target.focus(); }
    if (target || ["ArrowLeft", "ArrowRight", "Enter", " "].includes(event.key)) event.preventDefault();
  });

  return {
    select(key) {
      const ancestors = []; let cursor = byKey.get(key);
      while (cursor?.parentKey) { cursor = byKey.get(cursor.parentKey); ancestors.unshift(cursor.key); }
      for (const ancestor of ancestors) expanders.get(ancestor)?.(true);
      for (const row of rows.values()) {
        const selected = row.dataset.key === key;
        row.classList.toggle("is-selected", selected); row.setAttribute("aria-selected", String(selected));
      }
      const row = rows.get(key); if (row) row.scrollIntoView({ block: "nearest" });
    },
  };
}
