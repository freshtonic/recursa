# Handoff: Recursa Explorer — AST workspace redesign

## Overview

This is a redesign of the Recursa Explorer front-end (`recursa-explorer/assets/`) — the local
grammar workspace served on loopback by the `recursa-explorer` crate. It replaces the current
card-stack layout with a three-column workspace and replaces the single indented AST tree with
**one occurrence model rendered five ways**.

The problem it solves: in the current model every level of user-visible nesting costs three or four
tree levels (an enum node, its variant element, the concrete node, then the element holding the
children). The JSON sample `{"items":[true,null,{"name":"café"}],"ok":true}` is **34 occurrence
rows across 18 levels of indent**. At thousands of nodes the indent runs off the panel before the
structure is legible.

The load-bearing idea is **chain folding**: any run of single-child occurrences collapses into one
typographic line — `Value::Object › object › Object › members` — taking those 34 rows to 12 and 18
levels to 4, while every name, variant, role and cardinality stays on screen. Nothing is hidden;
the chain is set on one line instead of four.

## About the design files

The files in this bundle are **design references created in HTML** — a working prototype showing
intended look and behaviour, not production code to lift wholesale. `Recursa Explorer.dc.html`
runs on a bespoke streaming-template runtime (`support.js`) that has no place in the Explorer
codebase.

The task is to **recreate these designs in `recursa-explorer/assets/`** using its existing
environment: vanilla ES modules, no build step, no framework, `index.html` + `style.css` +
`app.js`/`views.js`/`tree.js`/`model.js`/`offsets.js`, assets embedded into the binary by
`src/assets.rs`. Keep that architecture. Read the prototype for layout, geometry, colour and
interaction; write plain DOM code.

The prototype's layout algorithms (graph placement, nested containment measure/place, span→depth
projection) **are** the specification and can be ported near-verbatim — see *Layout algorithms*
below.

## Fidelity

**High fidelity.** Colours, type, spacing, radii and interaction states are final and come from the
Nocturne design system (tokens listed under *Design tokens*; the full stylesheet is bundled as
`nocturne-styles.css`). Recreate pixel-accurately. Exception: the prototype's card widths (1440px,
1000px, etc.) are artboard sizes for side-by-side review — in the real app the workspace fills the
viewport and the columns are the fixed/flexible grid described below.

---

## Screens / Views

There is **one screen**: the workspace. It has three columns and the middle column has five render
modes.

### Chrome — header bar

- Height 52px, padding 0 20px.
- Background `linear-gradient(180deg,#1b1e2f,#161826)`, bottom edge `inset 0 -1px 0 rgba(233,233,237,.08)`.
- Left group, gap 16px:
  - Status dot: 9px circle, `background: var(--color-accent)`, `box-shadow: 0 0 10px var(--color-accent-600)`, animating `opacity .55 → 1 → .55` over 3.2s `ease-in-out` infinite.
  - `recursa` — Inter 500 15px, `letter-spacing:-.01em`.
  - `Explorer` — Inter 400 15px, `var(--color-neutral-500)`.
  - 1px × 18px divider, `rgba(233,233,237,.12)`.
  - Grammar identity: IBM Plex Mono 400 12px `var(--color-neutral-400)`, e.g. `examples/json` then ` · root ` in `var(--color-neutral-700)` then `Value`.
- Right group, gap 14px:
  - Parse status: 5px green dot `#8fd6a8`, then IBM Plex Mono 400 11.5px `var(--color-neutral-400)`: `parsed · 0.41 ms · 19 tokens`.
  - `Stop Explorer` — `.btn.btn-ghost`, font-size 11.5px, padding 5px 11px.

### Workspace grid

`display:grid; grid-template-columns: 352px minmax(0,1fr) 268px;` filling the remaining height.
Column separators are inset box-shadows (`inset -1px 0 0` / `inset 1px 0 0`, `rgba(233,233,237,.08)`),
not borders.

### Column 1 — Source (352px)

Vertical flex.

1. **Header** — padding 16px 18px 12px, `align-items:flex-end`, space-between.
   - Eyebrow: IBM Plex Mono 600 9.5px, `letter-spacing:.14em`, uppercase, `var(--color-accent-400)`, `margin-bottom:3px` — text `Input`.
   - Title: Inter 500 16px, `letter-spacing:-.01em` — `Source`.
   - `Parse now` — `.btn.btn-primary`, font-size 11.5px, padding 5px 11px.
2. **Code body** — `flex:1; overflow:auto; padding-top:4px`. One row per source line, padding 0 14px:
   - Gutter: 26px wide, right-aligned, `padding-right:12px`, IBM Plex Mono 400 11.5px / line-height 26px, `var(--color-neutral-500)`, `user-select:none`.
   - Code: IBM Plex Mono 400 13px / 26px, `white-space:pre`. Each **token** is its own span so it can be highlighted and hovered.
     - Token colours: punctuation `var(--color-neutral-600)`; keyword/literal (`true`/`false`/`null`) `var(--color-accent-400)`; string `#8bc7ae`.
     - Token inside the selected occurrence's span: `background: color-mix(in srgb, var(--color-accent) 13%, transparent)`; punctuation inside selection lifts to `var(--color-neutral-300)`.
     - Token whose span is **exactly** the selection: `background: color-mix(in srgb, var(--color-accent) 34%, transparent)` plus `box-shadow: inset 0 -1.5px 0 var(--color-accent)`.
     - `border-radius:3px`, `transition: background .16s ease, color .16s ease`.
   - **Hovering a token selects the deepest leaf occurrence containing it** (the reverse-order first match on `span[0] <= tokStart && span[1] >= tokEnd && !children.length`).
3. **Diagnostics footer** — padding 14px 18px 16px, top edge `inset 0 1px 0 rgba(233,233,237,.08)`.
   - Eyebrow `Diagnostics`: IBM Plex Mono 600 9.5px, `.14em`, uppercase, `var(--color-neutral-600)`.
   - Clean state: `✓` in `#8fd6a8` then `No diagnostics · strict parse complete`, IBM Plex Mono 400 12px `var(--color-neutral-500)`.
   - Hint line: Inter 400 11.5px/1.6 `var(--color-neutral-600)` — "Selection maps both ways. Hover a token to select the deepest containing occurrence."
   - The existing `#diagnostics` list keeps its behaviour; render each diagnostic in this type, error rows in the accent-free red equivalent of the same treatment (the prototype has no error state — see *Not covered*).

### Column 2 — Parsed structure (flexible)

Vertical flex, four bands: header / breadcrumb strip / render surface (fixed 600px in the
prototype, `flex:1` in the app) / occurrence footer.

**Header** — padding 16px 20px 10px.
- Eyebrow `Parsed structure` (accent-400 eyebrow style as above).
- Title row: mode name in Inter 500 16px, then the count in IBM Plex Mono 400 11.5px `var(--color-neutral-400)`: `34 occurrences · 12 rows · depth 18`.
- **Mode switcher**, right: padding 3px, `border-radius:8px`, `background: rgba(233,233,237,.05)`, children gap 4px. Each option is a button: no border, `border-radius:6px`, padding 5px 10px, IBM Plex Mono 400 11.5px, `transition: background .18s ease, color .18s ease`.
  - Inactive: transparent, `var(--color-neutral-500)`.
  - Active: `background: color-mix(in srgb, var(--color-accent) 18%, transparent)`, `color: var(--color-accent-200)`.
  - Options in order: **Constellation** (default), **Nest**, **Outline**, **Grain**, **Strata**.

**Breadcrumb strip** — padding 0 20px 10px, min-height 24px, single row.
- Crumbs: IBM Plex Mono 400 11.5px. Ancestors `var(--color-neutral-600)`, current `var(--color-accent-300)`, `transition: color .16s ease`, clickable (selects that ancestor). Separator `›` in `var(--color-neutral-700)`, `margin: 0 7px`, font-size 11px.
- Right side, **mode-specific controls only**:
  - Constellation / Nest: zoom percentage (IBM Plex Mono 400 11px `var(--color-neutral-600)`), then `−`, `+` (`.btn.btn-ghost`, font-size 12px, padding 1px 8px) and `Fit` (`.btn.btn-ghost`, font-size 10.5px, padding 3px 8px, mono).
  - Outline: `Fold chains` toggle — inline-flex, gap 7px, padding 4px 9px, `border-radius:6px`, IBM Plex Mono 400 11px, glyph `◼`/`◻` at 12px. On: border `color-mix(in srgb,var(--color-accent) 45%,transparent)`, background `color-mix(in srgb,var(--color-accent) 15%,transparent)`, text `var(--color-accent-300)`. Off: border `rgba(233,233,237,.14)`, transparent, `var(--color-neutral-400)`.
  - Grain / Strata: no controls.
- Far right, always: selection span — IBM Plex Mono 400 11.5px `var(--color-accent-300)`, `letter-spacing:.02em`, text `bytes 12..38` or `synthetic`.

**Render surface** — top edge `inset 0 1px 0 rgba(233,233,237,.08)`, `overflow:hidden`.

#### Mode A — Constellation (default)

Top-down node-and-edge graph on a pannable, zoomable field.

- Field background: `radial-gradient(120% 90% at 12% 30%, #1c1f33 0%, #141626 55%, #101220 100%)`, plus a dot grid overlay: `radial-gradient(circle at 1px 1px, rgba(233,233,237,.07) 1px, transparent 1px)`, `background-size:26px 26px`, `opacity:.6`. Cursor `grab`.
- One transformed layer holds an absolutely-positioned SVG (edges, `pointer-events:none`) under absolutely-positioned HTML pills (nodes). `transform: translate(x,y) scale(z)`, `transform-origin: 0 0`, `will-change:transform`.
- **Node pills**: `border-radius:11px`, `padding: 0 11px`, `white-space:nowrap`, IBM Plex Mono 400 12px, column flex, `gap:1px`, `justify-content:center`, `align-items:flex-start`.
  - **Element occurrence** — height **37px**, two lines: name in *italic*, then a meta line (`role · cardinality`) at font-size 9.5px, line-height 1.1, `letter-spacing:.03em`, `var(--color-neutral-400)`. Background transparent, ring `0 0 0 1px rgba(233,233,237,.12)`, text `var(--color-neutral-500)`.
  - **Node occurrence** — height **26px**, one line: name in weight 500, and if it has a variant, `::Variant` in `var(--color-accent-400)` weight 500. Background `#232532`, ring `0 0 0 1px var(--color-neutral-800)`, text `var(--color-neutral-200)`.
  - **Selected** — background `color-mix(in srgb,var(--color-accent) 26%,transparent)`, ring `0 0 0 1px var(--color-accent), 0 0 22px color-mix(in srgb,var(--color-accent) 45%,transparent)`, text `var(--color-accent-100)`.
  - **Opacity**: 1 on the selected ancestor path, `0.5` everywhere else. `transition: background .25s, box-shadow .25s, color .25s, opacity .25s`.
- **Edges**: cubic Bézier from the parent's bottom-centre to the child's top-centre with control points at the vertical midpoint —
  `M x1,y1 C x1,my x2,my x2,y2` where `my = (y1+y2)/2`.
  - Default `stroke: rgba(233,233,237,.13)`, `stroke-width:1`.
  - Both ends on the selected path: `stroke: var(--color-accent)`, `stroke-width:1.6`. `transition: stroke .25s ease`.
- Hint text bottom-left, 18px/12px inset: IBM Plex Mono 400 10.5px `var(--color-neutral-500)` — "drag to pan · scroll to zoom".

#### Mode B — Nest

Containment boxes, no edges. Every subtree is a rectangle labelled with its own type; containment
carries the parent relation.

- Field background `radial-gradient(110% 80% at 50% 0%, #1a1d2e 0%, #131523 60%, #101220 100%)`; same pan/zoom layer, no dot grid.
- **Box**: `box-sizing:border-box`, `border-radius:7px`, `border:1px`, `transition: background .22s ease, border-color .22s ease`.
  - Fill tint compounds with depth: `color-mix(in srgb, var(--color-accent) T%, transparent)` where `T = min(9, 2 + depth*0.45)`.
  - Border colour: `color-mix(in srgb, var(--color-accent) E%, transparent)` where `E = min(64, 16 + depth*2.8)`; style **dotted**.
  - On the selected path: border solid, `color-mix(in srgb,var(--color-accent) 70%,transparent)`.
  - Selected: border solid `var(--color-accent)`, fill `color-mix(in srgb,var(--color-accent) 24%,transparent)`, label `var(--color-accent-100)`.
- **Label**: absolute at left 7px / top 1px, IBM Plex Mono 400 11px / 18px, `white-space:nowrap`, gap 5px. Element names italic in `var(--color-neutral-400)`; node names weight 500 in `var(--color-neutral-200)`; `::Variant` in `var(--color-accent-400)`; leaf token text in `#8bc7ae`.
- **Meta** (elements only, and only when the box is wider than 176px): absolute right 8px / top 2px, Inter 400 9.5px/16px, `letter-spacing:.06em`, `var(--color-neutral-400)`, text `role · cardinality`.
- Hint text bottom-left: "each frame is a subtree · tint compounds with nesting".

#### Mode C — Outline

The chain-folded ledger: a tree of one-line rows with dotted leaders, depth guides, and a shape
strip on the right edge.

- Layout: `display:flex` — tree (`flex:1`, `overflow:auto`, padding 8px 0 24px) + 76px shape strip.
- **Row**: `position:relative`, `display:flex`, `align-items:center`, height = density (26px compact / 30px comfortable / 34px airy), `padding-right:18px`, `cursor:pointer`, entering with `rowIn .18s ease-out` (`opacity 0→1`, `translateY(-3px)→0`).
  - Indent: a 22px-per-depth spacer span, then a 20px toggle cell, then the label.
  - **Depth guides**: one 1px full-height absolute span per level at `left: depth*22 + 9`. On the selected ancestor path `color-mix(in srgb,var(--color-accent) 34%,transparent)`, otherwise `rgba(233,233,237,.07)`; `transition: background .2s`.
  - **Toggle glyph**: 20px cell, centred, IBM Plex Mono 400 10px. `▼` expanded / `▶` collapsed / `·` leaf. Colour: selected `var(--color-accent-300)`, has children `var(--color-neutral-600)`, leaf `var(--color-neutral-800)`. Clicking it toggles collapse and must `stopPropagation`.
  - **Label** — IBM Plex Mono 400 13px, `white-space:nowrap`. A folded chain renders as multiple segments joined by `›` (`var(--color-neutral-700)`, `margin: 0 6px`, 10.5px):
    - element segment: `var(--color-neutral-500)`, italic;
    - node segment: `var(--color-neutral-100)`, weight 500;
    - variant: `::` in `var(--color-neutral-700)` then the variant in `var(--color-accent-400)` weight 500.
  - **Leaf text** (single-token leaves): `margin-left:12px`, IBM Plex Mono 400 13px, `#8bc7ae`.
  - **Dotted leader**: `flex:1; min-width:16px; height:1px; margin: 0 12px`, `background-image: radial-gradient(circle at 1px 50%, rgba(233,233,237,.22) 1px, transparent 1px)`, `background-size:5px 1px`, `repeat-x`. Opacity 0.9 on the selected row, 0.45 otherwise.
  - **Role** (from the deepest element in the folded chain): Inter 400 10px, `letter-spacing:.1em`, uppercase, `var(--color-neutral-600)`, `margin-right:10px`.
  - **Cardinality pill**: IBM Plex Mono 500 10.5px `var(--color-accent-2-300)`, padding 2px 7px, `border-radius:999px`, `background: color-mix(in srgb,var(--color-accent) 12%,transparent)`, `box-shadow: inset 0 0 0 1px color-mix(in srgb,var(--color-accent) 22%,transparent)`. Values are the real cardinality strings: `One`, `Vec⟨,⟩`, `Option⟨Vec1⟨,⟩⟩`.
  - **Row background**: selected `linear-gradient(90deg, color-mix(in srgb,var(--color-accent) 17%,transparent), color-mix(in srgb,var(--color-accent) 4%,transparent) 70%, transparent)` plus `box-shadow: inset 2px 0 0 var(--color-accent)`; on the selected path `color-mix(in srgb,var(--color-accent) 4%,transparent)`; otherwise transparent.
- **Shape strip** (right, 76px, left edge `inset 1px 0 0 rgba(233,233,237,.08)`, padding 10px 10px 10px 0):
  - Heading `Shape`: Inter 600 8.5px, `.14em`, uppercase, `var(--color-neutral-500)`, padding-left 10px, margin-bottom 8px.
  - One 5px-tall bar **per occurrence in document order** (not per row — the whole tree, folded or not), gap 1px, `border-radius:1px`, `margin-left: depth*2`px, `width: max(4, 40 - depth*2)`px.
  - Colour: selected `var(--color-accent)`; on path `var(--color-accent-600)`; node `rgba(233,233,237,.20)`; element `rgba(233,233,237,.09)`. Clickable, `transition: background .2s, width .2s`.

#### Mode D — Grain

No separate tree: structure drawn as depth columns in a gutter beside the code.

- `overflow:auto`, padding 20px 24px. Flex row, gap 16px: bars gutter (`width: (maxDepth+1)*8`px) then the same token-highlighted source as column 1 (without line numbers), IBM Plex Mono 400 13px/26px.
- **Bar** per occurrence with a span: 3px wide, `border-radius:2px`, `left: depth*8`px, `top: firstLine*26 + 3`px, `height: (lastLine - firstLine + 1)*26 - 6`px. Line index is derived from the byte offset via the line-start table.
- Colour: selected `var(--color-accent)`; on path `var(--color-accent-600)`; node `rgba(233,233,237,.24)`; element `rgba(233,233,237,.10)`.
- Hovering a bar names it in the footer strip below (top edge `inset 0 1px 0 rgba(233,233,237,.08)`, margin-top 20px, padding-top 12px): title IBM Plex Mono 500 12.5px `var(--color-neutral-100)` (`Name::Variant`), meta IBM Plex Mono 400 11px `var(--color-neutral-600)` (`role · cardinality · bytes a..b`).

#### Mode E — Strata

Every occurrence at once: x = byte span, y = depth. The silhouette *is* the shape of the parse.

- `overflow:auto`, padding 22px 24px. Track width `SW` = the available surface width (720px in the prototype's pane, 800px in the standalone card).
- **Cell** per occurrence with a span: absolute, `left: span[0]/totalBytes*SW`, `top: depth*19`, `width: max(3, (span[1]-span[0])/totalBytes*SW - 1.5)`, height 16px, `border-radius:3px`, `overflow:hidden`.
- Fill `color-mix(in srgb, var(--color-accent) P%, transparent)` where P = `min(30, 5 + depth*1.4)` for nodes and `min(16, 2 + depth*0.8)` for elements. Selected: `var(--color-accent-600)` with ring `0 0 0 1px var(--color-accent-300)`; on path ring `0 0 0 1px color-mix(in srgb,var(--color-accent) 45%,transparent)`.
- Label inside the cell, padding 0 5px, IBM Plex Mono 400 10px, `white-space:nowrap` — **omitted when the cell is narrower than `label.length*6.2 + 12`px**. Node labels `var(--color-neutral-300)`, elements `var(--color-neutral-600)`, selected `var(--color-accent-100)`.
- Axis strip below (margin-top 14px): `byte 0` — hovered occurrence name — `byte {total}`, IBM Plex Mono 400 10px `var(--color-neutral-400)`, space-between.

**Occurrence footer** (shared by all five modes) — padding 14px 20px 16px, top edge
`inset 0 1px 0 rgba(233,233,237,.08)`, background `#13152233`.
- Title row, gap 12px, baseline-aligned: eyebrow `Occurrence` (IBM Plex Mono 600 9.5px, `.14em`, uppercase, `var(--color-neutral-600)`), name (IBM Plex Mono 500 14px `var(--color-neutral-100)`), variant (IBM Plex Mono 500 12.5px `var(--color-accent-400)`, prefixed `::`).
- Field grid: wrapping flex, gap 36px. Each field is a two-line stack, gap 2px: key in Inter 600 9px, `letter-spacing:.13em`, uppercase, `var(--color-neutral-400)`; value in IBM Plex Mono 400 12.5px `var(--color-neutral-200)`.
- Fields in order: `kind` (node|element), `span` (`12..38  (26 bytes)` or `synthetic`), `depth`, `role` (elements only), `cardinality` (elements only), `text` (leaf token or `—`), `children` (count).

### Column 3 — Grammar (268px)

`overflow:auto`, left edge `inset 1px 0 0 rgba(233,233,237,.08)`.

- Header, padding 16px 18px 12px: eyebrow `Static contract` (accent-400), title `Grammar` (Inter 500 16px).
- Body, padding 0 18px 18px, column gap 18px.
- **Nodes · N** section heading: Inter 600 9px, `.13em`, uppercase, `var(--color-neutral-400)`, margin-bottom 8px. Then one entry per node type, gap 9px: name IBM Plex Mono 500 12px `var(--color-neutral-200)` — **lifting to `var(--color-accent-300)` when the selected occurrence is of that type**, `transition: color .2s` — and shape below in IBM Plex Mono 400 11px/1.5 `var(--color-neutral-600)`, `word-break:break-word` (e.g. `{ members: Vec⟨Member, COMMA⟩ }`, `Object | Array | String | Number | Bool | Null`).
- **Tokens · N**: wrapping flex, gap 5px. Each token IBM Plex Mono 400 11px, padding 2px 7px, `border-radius:4px`, `var(--color-neutral-300)` on `rgba(233,233,237,.07)`.
- **Capabilities**: rows at padding 9px 11px, `border-radius:7px`, `background: rgba(233,233,237,.05)`, space-between. Label IBM Plex Mono 400 12px `var(--color-neutral-300)`; value on the right in `var(--color-neutral-600)` (Inter 10.5px for prose like `width 80 · indent 2`, mono for the hex seed). These rows are the collapsed form of the existing Pretty and Sample panels — clicking one should expand it into the existing controls (number inputs, seed field, Render/Generate/Apply buttons) styled as `.input` / `.btn`.

---

## Interactions & behaviour

**Selection is one shared piece of state across the whole workspace.** Selecting anywhere updates
everywhere: source highlight, breadcrumb, every render mode, the occurrence footer, the grammar
panel's type highlight.

- Click a graph pill / nest box / outline row / grain bar / strata cell / shape bar / breadcrumb crumb → select that occurrence.
- Hover a source token → select the deepest leaf occurrence containing it.
- Hover a grain bar or strata cell → sets a separate `hover` id that drives only the grain/strata caption (selection unchanged).
- **Switching mode never loses your place**: mode is independent of selection, and each mode keeps its own pan/zoom state.
- Outline `Fold chains` toggles chain folding. Outline row toggles collapse the subtree below a row; `Expand all` clears all collapse state.
- Pan: `pointerdown` on the field, `pointermove` on window, released on `pointerup`. Zoom: `wheel` (`preventDefault`, factor 1.08 per notch) and the `−`/`+` buttons (factor 1.2). Zoom is clamped to **0.3–1.4**. `Fit` recentres: `z = clamp(min((fw-36)/cw, (fh-36)/ch))`, `x = (fw - cw*z)/2`, `y = (fh - ch*z)/2`.
- Transitions: 0.16s on source token highlight, 0.18s on switcher/toggle, 0.2s on guides/bars/strata, 0.22s on nest boxes, 0.25s on graph pills and edges. All `ease`.
- Focus: `:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px }` — never the browser default.
- Keyboard: the tree keeps `role="tree"` / `role="treeitem"`; arrow-key navigation over the folded row list should move selection (the prototype only has `tabindex="-1"` scaffolding — see *Not covered*).

## State

| State | Type | Notes |
| --- | --- | --- |
| `sel` | occurrence id | the shared selection |
| `hover` | occurrence id | grain/strata caption only |
| `mode` | `graph`\|`nest`\|`outline`\|`grain`\|`strata` | defaults to `graph` (Constellation) |
| `fold` | boolean | chain folding, default **on** |
| `collapsed` | `{ id: boolean }` | outline subtree collapse |
| per-field `z`,`x`,`y` | numbers | one triple per pan/zoom field, initialised by `fit` |

Density (`compact` 26px / `comfortable` 30px / `airy` 34px row height), guides on/off and leaders
on/off are prototype-level options; ship `compact`, guides on, leaders on, and expose the rest only
if you want a preferences surface.

## Layout algorithms

Port these from `Recursa Explorer.dc.html` (the `Component` class); they are the spec.

**Occurrence model.** Two kinds: `node` (`{name, variant, tokens, children}`) and `element`
(`{name, role, cardinality, tokens, children}`). Annotation pass assigns `id`, `depth`, `parent`,
then computes `span` bottom-up as the min/max over own token spans and children's spans (`null` =
synthetic, no source bytes), and `text` = the token text when the occurrence is a single-token leaf.
The Explorer's existing `model.js`/`offsets.js` already produce spans — reuse them.

**Chain folding.** `chainOf(n)`: start with `[n]`; while the current occurrence has exactly one
child, append that child and continue. The chain renders as one row; the *tail* of the chain owns
the children, the collapse toggle and the leaf text; the deepest *element* in the chain supplies
the row's role and cardinality.

**Constellation layout.** Assign each leaf a sequential index (`gy`); every internal occurrence gets
the mean of its first and last child's `gy`. Then `x = 20 + gy*172`, `y = 20 + depth*46`. Pill width
= `max(nameWidth, metaWidth) + 22` where `nameWidth = (name.length + variant.length + 2) * 7.35` and
`metaWidth = meta.length * 5.85`; height 37 for elements, 26 for nodes. Content size is the max
extent + 20. **Guard every division and `Math.max` against non-finite values** — the prototype wraps
them in a `fin(v, fallback)` helper precisely so no `NaN` reaches an SVG path or a `translate()`.

**Nest layout.** Constants: header 20px, pad 9px, gap 5px, char width 7.35px.
`measure(n)` bottom-up — a leaf is `labelWidth × (header+3)`; an occurrence with **one** child stacks
(`w = max(labelW, child.w) + 2*pad`, `h = header + child.h + pad`); with **several** children it lays
them across (`w = max(labelW, Σchild.w + gap*(n-1)) + 2*pad`, `h = header + max(child.h) + pad`).
`place(n,x,y)` then walks down: row children start at `x+pad, y+header` advancing by `w+gap`;
a single child goes to `x+pad, y+header`.

**Grain / Strata projection.** Grain maps byte offset → line via the line-start table, x by depth
(8px per level). Strata maps byte offset → x by `offset/totalBytes*trackWidth`, y by `depth*19`.

## Design tokens

From Nocturne (`nocturne-styles.css`, bundled). Use the CSS variables; the hexes are listed so you
can match without the file.

**Roles** — `--color-bg` `#161826`, `--color-surface` `#232532`, `--color-text` `#e9e9ed`,
`--color-accent` `#9184d9`, `--color-divider` `color-mix(in srgb,#e9e9ed 16%,transparent)`.

**Neutral ramp** — 100 `#f3f5fe`, 200 `#e4e7f5`, 300 `#cfd3e5`, 400 `#b2b6ca`, 500 `#9397ab`,
600 `#75798c`, 700 `#595d6c`, 800 `#3f424d`, 900 `#292b31`.

**Accent ramp** — 100 `#f5f4ff`, 200 `#e7e5fe`, 300 `#d2cefd`, 400 `#b5abfc`, 500 `#968ae0`,
600 `#796cbf`, 700 `#5d5294`, 800 `#423a6a`, 900 `#2b2741`.

**Accent-2 ramp** (mono palette — reads the same as accent; only `--color-accent-2-300` `#d2cefd`
is used here, on the cardinality pill).

**Off-token colours used by this design** (semantic, not in the system): parse-ok / diagnostics-ok
green `#8fd6a8`; string-literal green `#8bc7ae`; graph field gradient stops `#1c1f33`, `#141626`,
`#101220`; nest field stops `#1a1d2e`, `#131523`, `#101220`; header gradient `#1b1e2f` → `#161826`;
page ground `#101220`; occurrence-footer wash `#13152233`. Add these to `style.css` as named
variables rather than scattering hexes.

**Type** — headings and UI chrome in **Inter** (`--font-heading`/`--font-body`), all code, values,
identifiers, spans, roles and cardinalities in **IBM Plex Mono** 400/500/600
(`https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600`). Sizes used: 9 / 9.5 /
10 / 10.5 / 11 / 11.5 / 12 / 12.5 / 13 / 14 / 15 / 16px. Do not bolden headings past 500.

**Radii** — 3px (token highlight, strata cell), 4px (token chip), 6px (switcher option, toggle),
7px (nest box, capability row), 8px (switcher shell), 10px (card), 11px (graph pill), 999px
(cardinality pill).

**Spacing** — Nocturne's `--space-*` scale at density 0.7×. The literal values above come from that
scale; prefer the variables where they line up.

**Shadows** — elevation via `--shadow-sm/md/lg`; separators via inset 1px box-shadows at
`rgba(233,233,237,.08)`, never `border`.

## Assets

None. No images, no icon files. Every glyph in the design is a text character (`›`, `▼`, `▶`, `·`,
`◼`, `◻`, `✓`, `−`, `+`) and every graphic is CSS or inline SVG paths generated at runtime. If you
want real icons, Nocturne specifies **Phosphor** (https://phosphoricons.com) — but this design does
not currently use any.

## Not covered by the prototype (decide in implementation)

- **Failed parse / diagnostics with errors.** The prototype only shows the clean state.
- **Virtualisation.** The design is explicitly aimed at thousands of nodes; the prototype renders
  every occurrence as a DOM element. Outline and Strata will need windowing, and Constellation/Nest
  will need viewport culling at real scale.
- **Keyboard navigation** of the tree beyond the existing `role="tree"` scaffolding.
- **Empty state** (`#tree-empty`, "Enter valid source to inspect its tree.") — keep the existing
  copy, restyle it to Inter 400 12px `var(--color-neutral-600)`.
- **Responsive collapse** below ~1100px: the three-column grid needs a rule for narrow windows
  (suggestion: drop the Grammar column to a toggle).

## Files

In this bundle:

- `Recursa Explorer.dc.html` — the design prototype. All five render modes, both design turns. Open
  it in a browser (needs `support.js` and `_ds/` beside it — see below) and click through it; the
  interactions are real.
- `support.js` — the prototype's runtime. Reference only; **do not port**.
- `_ds/nocturne-.../styles.css` — the Nocturne token sheet and component layer. This is the source
  of truth for colours, type, spacing, radii, shadows and interaction states.
- `_ds/nocturne-.../readme.md` — the Nocturne guide (direction, do/don't).

To run the prototype: serve this folder over HTTP (`python3 -m http.server`) and open
`Recursa Explorer.dc.html`. Section `2a` is the workspace; `2b` is Nest standalone; sections `1a`–`1d`
are the earlier turn, kept for the Ledger/Constellation/Grain/Strata explorations.

In the Explorer repo, the code to change:

| Design area | Repo files |
| --- | --- |
| Header, three-column shell, panel structure | `recursa-explorer/assets/index.html`, `style.css` |
| All colour / type / spacing tokens | `recursa-explorer/assets/style.css` |
| Outline mode (rows, folding, guides, leaders, shape strip) | `recursa-explorer/assets/tree.js` |
| Constellation, Nest, Grain, Strata — new render modes | new modules beside `tree.js` (e.g. `graph.js`, `nest.js`, `grain.js`, `strata.js`) |
| Mode switcher, shared selection, breadcrumb, occurrence footer | `recursa-explorer/assets/views.js`, `app.js` |
| Occurrence model, spans, line-start table | `recursa-explorer/assets/model.js`, `offsets.js` |
| Asset embedding (add any new module here) | `recursa-explorer/src/assets.rs` |
