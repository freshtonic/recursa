# recursa-diagram Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use cipherpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a new workspace crate `recursa-diagram` that provides an attribute macro `#[railroad]`. Applied to an AST type, it emits inline SVG railroad diagrams into rustdoc.

**Architecture:** Two layers in one crate. Layer 1 is a pure-Rust port of the tabatkins railroad-diagrams layout algorithm with SVG serialization, dependency-free. Layer 2 is a proc macro that walks a type's syntactic structure (struct fields / enum variants, one level deep), recognizes recursa combinators by type-name pattern, builds a Layer 1 tree, serializes it, and emits a `#[doc = "<svg>...</svg>"]` attribute on the type.

**Tech Stack:** Rust 2024 edition, `syn` v2 for type analysis, `quote`/`proc-macro2` for codegen, no third-party layout or SVG crates. Testing via `cargo test` with committed fixtures; macro expansion tests via `trybuild` or snapshot files.

**Reference:** The design doc this plan implements is `docs/plans/2026-04-14-recursa-diagram-design.md`. Read it before starting.

**Project conventions:** Before any Rust work, skim `/Users/jamessadler/projects/recursa/CLAUDE.md`. Key rules: no manual `Parse` impls without justification, derive `Clone`/`Debug` rather than hand-writing them, method call syntax over UFCS, `Surrounded<L, I, R>` for any bracketed content.

---

## Phase 1 — Crate scaffolding

### Task 1: Create the `recursa-diagram` crate

**Files:**
- Create: `recursa-diagram/Cargo.toml`
- Create: `recursa-diagram/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

**Step 1: Add the crate to the workspace**

Edit the root `Cargo.toml`. Change the `members` line to include `recursa-diagram`:

```toml
[workspace]
members = ["recursa-core", "recursa-derive", "recursa-diagram", "pg-sql", "."]
resolver = "3"
```

**Step 2: Write `recursa-diagram/Cargo.toml`**

```toml
[package]
name = "recursa-diagram"
version = "0.1.0"
edition = "2024"
description = "Railroad syntax diagrams for recursa-derived AST types"
license = "MIT"
repository = "https://github.com/freshtonic/recursa"

[lib]
proc-macro = true

[dependencies]
syn = { version = "2", features = ["full", "extra-traits"] }
quote = "1"
proc-macro2 = "1"

[dev-dependencies]
trybuild = "1"
```

**Step 3: Write the empty lib root**

`recursa-diagram/src/lib.rs`:

```rust
//! Railroad syntax diagrams for recursa-derived AST types.
//!
//! See `docs/plans/2026-04-14-recursa-diagram-design.md` for the design.

mod layout;
mod svg;
```

**Step 4: Create empty module files so the crate builds**

`recursa-diagram/src/layout.rs`:

```rust
//! Railroad diagram layout primitives. Port of the tabatkins algorithm.
```

`recursa-diagram/src/svg.rs`:

```rust
//! SVG serialization for railroad layout trees.
```

**Step 5: Verify the workspace builds**

Run: `cargo build -p recursa-diagram`
Expected: clean build with no warnings.

**Step 6: Commit**

```bash
git add recursa-diagram Cargo.toml
git commit -m "feat(recursa-diagram): add empty crate skeleton"
```

---

## Phase 2 — Layout primitives

The layout algorithm mirrors the tabatkins reference implementation. Each primitive computes its bounding box at construction (`width`, `height`, `up`, `down` — `up` and `down` are vertical extents above and below the entry/exit baseline). SVG emission is a second pass.

Reference the JavaScript source in the user-provided link (`github.com/tabatkins/railroad-diagrams`) for exact constant values and layout math. The task descriptions below give target shapes and tests; the implementer should consult the reference for layout constants (padding, arc radius, character width).

### Task 2: Define the `Node` enum and shared geometry type

**Files:**
- Modify: `recursa-diagram/src/layout.rs`
- Create: `recursa-diagram/tests/layout_geometry.rs`

**Step 1: Write the failing test**

`recursa-diagram/tests/layout_geometry.rs`:

```rust
use recursa_diagram::layout::{Node, Terminal};

#[test]
fn terminal_geometry_is_nonzero() {
    let t = Node::Terminal(Terminal::new("SELECT"));
    assert!(t.width() > 0);
    assert!(t.height() > 0);
    assert!(t.up() >= 0);
    assert!(t.down() >= 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p recursa-diagram --test layout_geometry`
Expected: FAIL — `layout` module is not public, types don't exist.

**Step 3: Add the types**

Make the module public and add a stub. In `recursa-diagram/src/lib.rs`:

```rust
pub mod layout;
mod svg;
```

In `recursa-diagram/src/layout.rs`:

```rust
#[derive(Clone, Debug)]
pub enum Node {
    Terminal(Terminal),
    NonTerminal(NonTerminal),
    Sequence(Sequence),
    Choice(Choice),
    Optional(Optional),
    OneOrMore(OneOrMore),
}

impl Node {
    pub fn width(&self) -> u32 {
        match self {
            Node::Terminal(n) => n.width,
            Node::NonTerminal(n) => n.width,
            Node::Sequence(n) => n.width,
            Node::Choice(n) => n.width,
            Node::Optional(n) => n.width,
            Node::OneOrMore(n) => n.width,
        }
    }

    pub fn height(&self) -> u32 { /* likewise */ 0 }
    pub fn up(&self) -> u32 { 0 }
    pub fn down(&self) -> u32 { 0 }
}

#[derive(Clone, Debug)]
pub struct Terminal { pub text: String, pub width: u32, pub height: u32 }

impl Terminal {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        // Character width ~8 px + horizontal padding 20 px each side.
        let width = (text.chars().count() as u32) * 8 + 40;
        Self { text, width, height: 22 }
    }
}

// Stub remaining types (NonTerminal, Sequence, Choice, Optional, OneOrMore) with empty bodies for now.
#[derive(Clone, Debug)] pub struct NonTerminal { pub text: String, pub href: Option<String>, pub width: u32, pub height: u32 }
#[derive(Clone, Debug)] pub struct Sequence { pub children: Vec<Node>, pub width: u32, pub height: u32 }
#[derive(Clone, Debug)] pub struct Choice { pub children: Vec<Node>, pub default_idx: usize, pub width: u32, pub height: u32 }
#[derive(Clone, Debug)] pub struct Optional { pub child: Box<Node>, pub width: u32, pub height: u32 }
#[derive(Clone, Debug)] pub struct OneOrMore { pub child: Box<Node>, pub separator: Option<Box<Node>>, pub width: u32, pub height: u32 }
```

**Step 4: Flesh out `height`, `up`, `down`**

Update `Node::height/up/down` to delegate to the variants' fields. For now, hardcode `up = 11, down = 11` on Terminal (half-height).

**Step 5: Run tests**

Run: `cargo test -p recursa-diagram`
Expected: PASS.

**Step 6: Commit**

```bash
git add recursa-diagram
git commit -m "feat(recursa-diagram): layout node types and terminal geometry"
```

---

### Task 3: Implement `NonTerminal` geometry

**Files:**
- Modify: `recursa-diagram/src/layout.rs`
- Modify: `recursa-diagram/tests/layout_geometry.rs`

**Step 1: Add the failing test**

Append to the test file:

```rust
use recursa_diagram::layout::NonTerminal;

#[test]
fn non_terminal_width_scales_with_text() {
    let short = Node::NonTerminal(NonTerminal::new("Expr", None));
    let long = Node::NonTerminal(NonTerminal::new("VeryLongTypeName", None));
    assert!(long.width() > short.width());
}

#[test]
fn non_terminal_preserves_href() {
    let nt = NonTerminal::new("Expr", Some("Expr.html".into()));
    assert_eq!(nt.href.as_deref(), Some("Expr.html"));
}
```

**Step 2: Run — expect FAIL** (`NonTerminal::new` missing).

Run: `cargo test -p recursa-diagram`

**Step 3: Implement**

```rust
impl NonTerminal {
    pub fn new(text: impl Into<String>, href: Option<String>) -> Self {
        let text = text.into();
        let width = (text.chars().count() as u32) * 8 + 40;
        Self { text, href, width, height: 22 }
    }
}
```

**Step 4: Run — expect PASS.**

**Step 5: Commit**

```bash
git add recursa-diagram
git commit -m "feat(recursa-diagram): non-terminal geometry"
```

---

### Task 4: Implement `Sequence` geometry

**Files:**
- Modify: `recursa-diagram/src/layout.rs`
- Modify: `recursa-diagram/tests/layout_geometry.rs`

**Step 1: Failing test**

```rust
#[test]
fn sequence_width_sums_children_plus_spacing() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("B"));
    let wa = a.width();
    let wb = b.width();
    let seq = Node::Sequence(Sequence::new(vec![a, b]));
    // 10 px spacer between adjacent children.
    assert_eq!(seq.width(), wa + wb + 10);
}

#[test]
fn empty_sequence_has_zero_body_width() {
    let seq = Node::Sequence(Sequence::new(vec![]));
    // Entry/exit stubs: 20 px total.
    assert_eq!(seq.width(), 20);
}
```

Import `Sequence` in the test file.

**Step 2: Run — expect FAIL**

**Step 3: Implement**

```rust
impl Sequence {
    pub fn new(children: Vec<Node>) -> Self {
        let width = if children.is_empty() {
            20
        } else {
            let child_sum: u32 = children.iter().map(|c| c.width()).sum();
            child_sum + 10 * (children.len() as u32 - 1)
        };
        let height = children.iter().map(|c| c.height()).max().unwrap_or(22);
        Self { children, width, height }
    }
}
```

**Step 4: Run — expect PASS.**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): sequence geometry"
```

---

### Task 5: Implement `Choice` geometry

**Files:**
- Modify: `recursa-diagram/src/layout.rs`
- Modify: `recursa-diagram/tests/layout_geometry.rs`

**Step 1: Failing test**

```rust
#[test]
fn choice_width_is_max_child_plus_rails() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("LONGER_OPTION"));
    let wb = b.width();
    let ch = Node::Choice(Choice::new(0, vec![a, b]));
    // 20 px for entry/exit rails.
    assert_eq!(ch.width(), wb + 20);
}

#[test]
fn choice_height_sums_children_plus_vertical_gap() {
    let a = Node::Terminal(Terminal::new("A"));
    let b = Node::Terminal(Terminal::new("B"));
    let ha = a.height();
    let hb = b.height();
    let ch = Node::Choice(Choice::new(0, vec![a, b]));
    // 10 px vertical gap between branches.
    assert_eq!(ch.height(), ha + hb + 10);
}
```

Import `Choice`.

**Step 2: Run — expect FAIL**

**Step 3: Implement**

```rust
impl Choice {
    pub fn new(default_idx: usize, children: Vec<Node>) -> Self {
        assert!(!children.is_empty(), "Choice must have at least one child");
        assert!(default_idx < children.len());
        let width = children.iter().map(|c| c.width()).max().unwrap() + 20;
        let height: u32 =
            children.iter().map(|c| c.height()).sum::<u32>()
            + 10 * (children.len() as u32 - 1);
        Self { default_idx, children, width, height }
    }
}
```

**Step 4: Run — expect PASS.**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): choice geometry"
```

---

### Task 6: Implement `Optional` and `OneOrMore` geometry

**Files:**
- Modify: `recursa-diagram/src/layout.rs`
- Modify: `recursa-diagram/tests/layout_geometry.rs`

**Step 1: Failing tests**

```rust
use recursa_diagram::layout::{Optional, OneOrMore};

#[test]
fn optional_adds_skip_branch() {
    let child = Node::Terminal(Terminal::new("X"));
    let cw = child.width();
    let opt = Node::Optional(Optional::new(child));
    // skip rail adds 20 px of rails; height grows by 20 px (skip line + gap).
    assert_eq!(opt.width(), cw + 20);
    assert!(opt.height() > 22);
}

#[test]
fn one_or_more_with_separator() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let sep = Node::Terminal(Terminal::new(","));
    let max_w = child.width().max(sep.width());
    let oom = Node::OneOrMore(OneOrMore::new(child, Some(sep)));
    assert_eq!(oom.width(), max_w + 20);
}

#[test]
fn one_or_more_without_separator() {
    let child = Node::Terminal(Terminal::new("EXPR"));
    let cw = child.width();
    let oom = Node::OneOrMore(OneOrMore::new(child, None));
    assert_eq!(oom.width(), cw + 20);
}
```

**Step 2: Run — expect FAIL**

**Step 3: Implement**

```rust
impl Optional {
    pub fn new(child: Node) -> Self {
        let width = child.width() + 20;
        let height = child.height() + 20;
        Self { child: Box::new(child), width, height }
    }
}

impl OneOrMore {
    pub fn new(child: Node, separator: Option<Node>) -> Self {
        let sep_w = separator.as_ref().map(|s| s.width()).unwrap_or(0);
        let width = child.width().max(sep_w) + 20;
        let sep_h = separator.as_ref().map(|s| s.height()).unwrap_or(10);
        let height = child.height() + sep_h + 10;
        Self {
            child: Box::new(child),
            separator: separator.map(Box::new),
            width,
            height,
        }
    }
}
```

Also add a convenience `ZeroOrMore::new(child, sep) -> Node` that returns `Node::Optional(Optional::new(Node::OneOrMore(OneOrMore::new(child, sep))))` so call sites can build zero-or-more without manual nesting.

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): optional and one-or-more geometry"
```

---

## Phase 3 — SVG serialization

### Task 7: Emit SVG for a single `Terminal`

**Files:**
- Modify: `recursa-diagram/src/svg.rs`
- Modify: `recursa-diagram/src/lib.rs` (re-export `render`)
- Create: `recursa-diagram/tests/svg_basic.rs`

**Step 1: Failing test**

`recursa-diagram/tests/svg_basic.rs`:

```rust
use recursa_diagram::{render, layout::{Node, Terminal}};

#[test]
fn terminal_svg_contains_text() {
    let svg = render(&Node::Terminal(Terminal::new("SELECT")));
    assert!(svg.starts_with("<svg"), "should be an svg: {svg}");
    assert!(svg.contains("SELECT"), "should contain the literal: {svg}");
    assert!(svg.ends_with("</svg>"));
    assert!(svg.contains("<!-- railroad -->"));
}
```

**Step 2: Run — expect FAIL**

**Step 3: Implement**

Export `render` from `lib.rs`:

```rust
pub use svg::render;
```

In `svg.rs`:

```rust
use crate::layout::{Node, Terminal, NonTerminal, Sequence, Choice, Optional, OneOrMore};

pub fn render(root: &Node) -> String {
    let mut out = String::new();
    let pad = 10;
    let total_w = root.width() + pad * 2;
    let total_h = root.height() + pad * 2;

    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><!-- railroad --><style>.railroad rect{{fill:#fff;stroke:#333;stroke-width:1}} .railroad text{{font-family:monospace;font-size:12px;fill:#000}} .railroad path{{stroke:#333;stroke-width:1;fill:none}}</style><g class="railroad">"#,
        w = total_w,
        h = total_h,
    ));

    render_node(root, pad as i32, (pad + root.up()) as i32, &mut out);

    out.push_str("</g></svg>");
    out
}

fn render_node(node: &Node, x: i32, y: i32, out: &mut String) {
    match node {
        Node::Terminal(t) => render_terminal(t, x, y, out),
        Node::NonTerminal(nt) => render_non_terminal(nt, x, y, out),
        Node::Sequence(s) => render_sequence(s, x, y, out),
        Node::Choice(c) => render_choice(c, x, y, out),
        Node::Optional(o) => render_optional(o, x, y, out),
        Node::OneOrMore(om) => render_one_or_more(om, x, y, out),
    }
}

fn render_terminal(t: &Terminal, x: i32, y: i32, out: &mut String) {
    let w = t.width as i32;
    out.push_str(&format!(
        r##"<rect x="{x}" y="{ry}" width="{w}" height="22" rx="10" ry="10"/><text x="{tx}" y="{ty}" text-anchor="middle">{text}</text>"##,
        ry = y - 11,
        tx = x + w / 2,
        ty = y + 4,
        text = escape(&t.text),
    ));
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// Stubs for the other variants — real impls come in later tasks.
fn render_non_terminal(_: &NonTerminal, _: i32, _: i32, _: &mut String) {}
fn render_sequence(_: &Sequence, _: i32, _: i32, _: &mut String) {}
fn render_choice(_: &Choice, _: i32, _: i32, _: &mut String) {}
fn render_optional(_: &Optional, _: i32, _: i32, _: &mut String) {}
fn render_one_or_more(_: &OneOrMore, _: i32, _: i32, _: &mut String) {}
```

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git add recursa-diagram
git commit -m "feat(recursa-diagram): render terminal to svg"
```

---

### Task 8: SVG for `NonTerminal` (with optional `<a>` wrapper)

**Files:**
- Modify: `recursa-diagram/src/svg.rs`
- Modify: `recursa-diagram/tests/svg_basic.rs`

**Step 1: Failing tests**

```rust
use recursa_diagram::layout::NonTerminal;

#[test]
fn non_terminal_svg_without_href() {
    let svg = render(&Node::NonTerminal(NonTerminal::new("Expr", None)));
    assert!(svg.contains("Expr"));
    assert!(!svg.contains("<a "));
}

#[test]
fn non_terminal_svg_with_href_wraps_in_anchor() {
    let svg = render(&Node::NonTerminal(NonTerminal::new("Expr", Some("Expr.html".into()))));
    assert!(svg.contains(r#"<a xlink:href="Expr.html""#) || svg.contains(r#"<a href="Expr.html""#));
    assert!(svg.contains("Expr"));
}
```

**Step 2: Run — expect FAIL**

**Step 3: Implement**

Replace the `render_non_terminal` stub:

```rust
fn render_non_terminal(nt: &NonTerminal, x: i32, y: i32, out: &mut String) {
    if let Some(href) = &nt.href {
        out.push_str(&format!(r#"<a href="{h}">"#, h = escape(href)));
    }
    let w = nt.width as i32;
    out.push_str(&format!(
        r##"<rect x="{x}" y="{ry}" width="{w}" height="22"/><text x="{tx}" y="{ty}" text-anchor="middle">{text}</text>"##,
        ry = y - 11,
        tx = x + w / 2,
        ty = y + 4,
        text = escape(&nt.text),
    ));
    if nt.href.is_some() {
        out.push_str("</a>");
    }
}
```

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): render non-terminal with intra-doc link"
```

---

### Task 9: SVG for `Sequence`

**Files:**
- Modify: `recursa-diagram/src/svg.rs`
- Modify: `recursa-diagram/tests/svg_basic.rs`

**Step 1: Failing test**

```rust
use recursa_diagram::layout::Sequence;

#[test]
fn sequence_renders_children_in_order() {
    let seq = Node::Sequence(Sequence::new(vec![
        Node::Terminal(Terminal::new("SELECT")),
        Node::NonTerminal(NonTerminal::new("Column", None)),
    ]));
    let svg = render(&seq);
    let i_select = svg.find("SELECT").expect("SELECT present");
    let i_column = svg.find("Column").expect("Column present");
    assert!(i_select < i_column, "SELECT should appear before Column");
}
```

**Step 2: Run — expect FAIL**

**Step 3: Implement**

```rust
fn render_sequence(s: &Sequence, mut x: i32, y: i32, out: &mut String) {
    for (i, child) in s.children.iter().enumerate() {
        if i > 0 {
            // Connector path between previous child and this one.
            out.push_str(&format!(r#"<path d="M{x1} {y} h10"/>"#, x1 = x - 10));
        }
        render_node(child, x, y, out);
        x += child.width() as i32 + 10;
    }
}
```

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): render sequence"
```

---

### Task 10: SVG for `Choice`

**Files:**
- Modify: `recursa-diagram/src/svg.rs`
- Modify: `recursa-diagram/tests/svg_basic.rs`

**Step 1: Failing test**

```rust
use recursa_diagram::layout::Choice;

#[test]
fn choice_renders_all_branches() {
    let ch = Node::Choice(Choice::new(0, vec![
        Node::Terminal(Terminal::new("TRUE")),
        Node::Terminal(Terminal::new("FALSE")),
        Node::Terminal(Terminal::new("NULL")),
    ]));
    let svg = render(&ch);
    assert!(svg.contains("TRUE"));
    assert!(svg.contains("FALSE"));
    assert!(svg.contains("NULL"));
}
```

**Step 2: Run — expect FAIL**

**Step 3: Implement**

```rust
fn render_choice(c: &Choice, x: i32, y: i32, out: &mut String) {
    let inner_w = c.width - 20;
    let mut branch_y = y;
    for (i, child) in c.children.iter().enumerate() {
        if i > 0 {
            branch_y += child.height() as i32 / 2 + 10;
        }
        let child_x = x + 10 + ((inner_w - child.width()) as i32) / 2;
        render_node(child, child_x, branch_y, out);
        branch_y += child.height() as i32 / 2;
        // Entry and exit rails (simple horizontal lines — real layout uses arcs,
        // but a straight connector is sufficient for a first pass and may be
        // polished later by inspecting rendered output).
        out.push_str(&format!(
            r#"<path d="M{x} {y} Q{x1} {y} {x1} {by}"/>"#,
            x1 = x + 10,
            by = branch_y,
        ));
    }
}
```

This draws branches stacked vertically, centered, with simple quadratic entry curves. Visual polish (real arcs matching the tabatkins style) can happen in a later pass once the smoke test in Task 16 reveals what looks wrong.

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): render choice (first pass)"
```

---

### Task 11: SVG for `Optional` and `OneOrMore`

**Files:**
- Modify: `recursa-diagram/src/svg.rs`
- Modify: `recursa-diagram/tests/svg_basic.rs`

**Step 1: Failing tests**

```rust
use recursa_diagram::layout::{Optional, OneOrMore};

#[test]
fn optional_renders_child() {
    let opt = Node::Optional(Optional::new(Node::Terminal(Terminal::new("WHERE"))));
    let svg = render(&opt);
    assert!(svg.contains("WHERE"));
}

#[test]
fn one_or_more_renders_child_and_separator() {
    let oom = Node::OneOrMore(OneOrMore::new(
        Node::NonTerminal(NonTerminal::new("Expr", None)),
        Some(Node::Terminal(Terminal::new(","))),
    ));
    let svg = render(&oom);
    assert!(svg.contains("Expr"));
    assert!(svg.contains(',') || svg.contains("&#44;") || svg.contains(">,<"));
}
```

**Step 2: Run — expect FAIL**

**Step 3: Implement**

```rust
fn render_optional(o: &Optional, x: i32, y: i32, out: &mut String) {
    // Skip line above.
    out.push_str(&format!(r#"<path d="M{x} {y} v-15 h{w} v15"/>"#, w = o.width as i32));
    // Child below, offset by 10 px so rails don't overlap.
    render_node(&o.child, x + 10, y, out);
}

fn render_one_or_more(om: &OneOrMore, x: i32, y: i32, out: &mut String) {
    render_node(&om.child, x + 10, y, out);
    if let Some(sep) = &om.separator {
        // Separator on the back-edge below the child.
        let sep_y = y + om.child.height() as i32 + 10;
        render_node(sep, x + 10, sep_y, out);
    }
    // Loop back rails.
    out.push_str(&format!(
        r#"<path d="M{x} {y} v15 h{w} v-15"/>"#,
        w = om.width as i32,
    ));
}
```

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): render optional and one-or-more"
```

---

### Task 12: Snapshot fixtures for layout regressions

**Files:**
- Create: `recursa-diagram/tests/fixtures/select_stmt.svg`
- Create: `recursa-diagram/tests/svg_snapshot.rs`

**Step 1: Failing test**

`recursa-diagram/tests/svg_snapshot.rs`:

```rust
use recursa_diagram::{render, layout::*};

fn select_stmt_tree() -> Node {
    Node::Sequence(Sequence::new(vec![
        Node::Terminal(Terminal::new("SELECT")),
        Node::OneOrMore(OneOrMore::new(
            Node::NonTerminal(NonTerminal::new("Column", Some("Column.html".into()))),
            Some(Node::Terminal(Terminal::new(","))),
        )),
        Node::Optional(Optional::new(
            Node::NonTerminal(NonTerminal::new("FromClause", Some("FromClause.html".into()))),
        )),
    ]))
}

#[test]
fn select_stmt_snapshot() {
    let actual = render(&select_stmt_tree());
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/select_stmt.svg");

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::write(&fixture_path, &actual).unwrap();
    }

    let expected = std::fs::read_to_string(&fixture_path)
        .expect("fixture missing — run with UPDATE_SNAPSHOTS=1 once");
    assert_eq!(actual, expected, "snapshot mismatch — inspect rendered svg or re-run with UPDATE_SNAPSHOTS=1");
}
```

**Step 2: Run — expect FAIL** (fixture missing).

**Step 3: Generate the fixture**

Run: `UPDATE_SNAPSHOTS=1 cargo test -p recursa-diagram --test svg_snapshot`

Inspect `recursa-diagram/tests/fixtures/select_stmt.svg` by opening it in a browser. Confirm it looks reasonable.

**Step 4: Run — expect PASS**

Run: `cargo test -p recursa-diagram --test svg_snapshot`

**Step 5: Commit**

```bash
git add recursa-diagram
git commit -m "test(recursa-diagram): snapshot fixture for select stmt"
```

---

## Phase 4 — Proc macro

The `#[railroad]` attribute macro walks a type definition, builds a layout tree from its immediate structure, renders it, and emits a `#[doc = "<svg>..."]` attribute. Because it's a proc-macro, it must live in the same crate as the layout code (`recursa-diagram` is already `proc-macro = true`).

### Task 13: `#[railroad]` on a unit struct with an explicit label

**Files:**
- Modify: `recursa-diagram/src/lib.rs`
- Create: `recursa-diagram/src/macro_impl.rs`
- Create: `recursa-diagram/tests/trybuild/unit_terminal.rs`
- Create: `recursa-diagram/tests/macro_tests.rs`

**Step 1: Failing test**

`recursa-diagram/tests/macro_tests.rs`:

```rust
#[test]
fn compiles_unit_terminal() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/unit_terminal.rs");
}
```

`recursa-diagram/tests/trybuild/unit_terminal.rs`:

```rust
use recursa_diagram::railroad;

#[railroad(label = "SELECT")]
pub struct SelectKw;

fn main() {}
```

**Step 2: Run — expect FAIL**

Run: `cargo test -p recursa-diagram --test macro_tests`
Expected: fails because `railroad` attribute doesn't exist.

**Step 3: Scaffold the macro**

In `recursa-diagram/src/lib.rs`:

```rust
pub mod layout;
mod svg;
mod macro_impl;

pub use svg::render;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn railroad(attr: TokenStream, item: TokenStream) -> TokenStream {
    macro_impl::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
```

In `recursa-diagram/src/macro_impl.rs`:

```rust
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, DeriveInput, Data, Fields, Meta, MetaNameValue, Expr, ExprLit, Lit, LitStr};
use crate::layout::{Node, Terminal, NonTerminal, Sequence, Choice, Optional, OneOrMore};

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let attrs = parse_type_attrs(attr)?;

    let node = build_node(&input, &attrs)?;
    let svg = crate::svg::render(&node);
    let doc = format!("\n\n{}\n\n", svg);

    let ident = &input.ident;
    let generics = &input.generics;
    let vis = &input.vis;
    let body = strip_body(&input);

    Ok(quote! {
        #[doc = #doc]
        #vis #body
    })
}

#[derive(Default)]
struct TypeAttrs {
    label: Option<String>,
}

fn parse_type_attrs(attr: TokenStream) -> syn::Result<TypeAttrs> {
    if attr.is_empty() {
        return Ok(TypeAttrs::default());
    }
    let meta: Meta = parse2(quote! { railroad(#attr) })?;
    // ... parse label = "..." ...
    todo!("parse label")
}

fn build_node(_input: &DeriveInput, attrs: &TypeAttrs) -> syn::Result<Node> {
    // For this task, only handle unit structs with an explicit label.
    if let Some(label) = &attrs.label {
        return Ok(Node::Terminal(Terminal::new(label)));
    }
    Ok(Node::NonTerminal(NonTerminal::new("TODO", None)))
}

fn strip_body(input: &DeriveInput) -> TokenStream {
    // Re-emit the original item without our #[railroad] attribute.
    // Easiest: clone input, clear attrs we own, to_tokens.
    let mut clone = input.clone();
    clone.attrs.retain(|a| !a.path().is_ident("railroad"));
    quote! { #clone }
}
```

Fill in `parse_type_attrs` properly:

```rust
fn parse_type_attrs(attr: TokenStream) -> syn::Result<TypeAttrs> {
    let mut out = TypeAttrs::default();
    if attr.is_empty() {
        return Ok(out);
    }
    let nvs = syn::punctuated::Punctuated::<MetaNameValue, syn::Token![,]>::parse_terminated
        .parse2(attr)?;
    for nv in nvs {
        if nv.path.is_ident("label") {
            if let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = nv.value {
                out.label = Some(s.value());
            } else {
                return Err(syn::Error::new_spanned(nv.value, "expected string literal"));
            }
        } else {
            return Err(syn::Error::new_spanned(nv.path, "unknown attribute key"));
        }
    }
    Ok(out)
}
```

Add `use syn::parse::Parser;` at the top of `macro_impl.rs`.

**Step 4: Run — expect PASS**

Run: `cargo test -p recursa-diagram --test macro_tests`

**Step 5: Commit**

```bash
git add recursa-diagram
git commit -m "feat(recursa-diagram): #[railroad] on unit struct with label"
```

---

### Task 14: `#[railroad]` on a struct with fields → sequence

**Files:**
- Modify: `recursa-diagram/src/macro_impl.rs`
- Create: `recursa-diagram/tests/trybuild/struct_sequence.rs`

**Step 1: Failing test**

`recursa-diagram/tests/trybuild/struct_sequence.rs`:

```rust
use recursa_diagram::railroad;

pub struct Ident;
pub struct Comma;

#[railroad]
pub struct ArgList {
    name: Ident,
    comma: Comma,
    value: Ident,
}

fn main() {}
```

Add to `macro_tests.rs`:

```rust
#[test]
fn compiles_struct_sequence() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/struct_sequence.rs");
}
```

**Step 2: Run — expect FAIL** (build_node's stub ignores fields).

**Step 3: Implement**

In `build_node`, handle `Data::Struct` with named fields:

```rust
fn build_node(input: &DeriveInput, attrs: &TypeAttrs) -> syn::Result<Node> {
    if let Some(label) = &attrs.label {
        return Ok(Node::Terminal(Terminal::new(label)));
    }
    match &input.data {
        Data::Struct(s) => build_from_fields(&s.fields),
        Data::Enum(_) => Ok(Node::NonTerminal(NonTerminal::new(input.ident.to_string(), None))),
        Data::Union(_) => Err(syn::Error::new_spanned(input, "unions are not supported")),
    }
}

fn build_from_fields(fields: &Fields) -> syn::Result<Node> {
    let iter: Box<dyn Iterator<Item = &syn::Field>> = match fields {
        Fields::Named(n) => Box::new(n.named.iter()),
        Fields::Unnamed(u) => Box::new(u.unnamed.iter()),
        Fields::Unit => return Ok(Node::NonTerminal(NonTerminal::new("()", None))),
    };
    let mut children = Vec::new();
    for field in iter {
        let field_attrs = parse_field_attrs(&field.attrs)?;
        if field_attrs.skip { continue; }
        children.push(node_for_field_type(&field.ty, &field_attrs));
    }
    Ok(Node::Sequence(Sequence::new(children)))
}

#[derive(Default)]
struct FieldAttrs { label: Option<String>, skip: bool }

fn parse_field_attrs(attrs: &[syn::Attribute]) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs::default();
    for a in attrs {
        if !a.path().is_ident("railroad") { continue; }
        a.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                let s: LitStr = meta.value()?.parse()?;
                out.label = Some(s.value());
            } else if meta.path.is_ident("skip") {
                out.skip = true;
            } else {
                return Err(meta.error("unknown key"));
            }
            Ok(())
        })?;
    }
    if out.skip && out.label.is_some() {
        return Err(syn::Error::new_spanned(
            &attrs[0],
            "`skip` and `label` are mutually exclusive",
        ));
    }
    Ok(out)
}

fn node_for_field_type(ty: &syn::Type, field_attrs: &FieldAttrs) -> Node {
    if let Some(label) = &field_attrs.label {
        return Node::NonTerminal(NonTerminal::new(label, None));
    }
    let name = type_label(ty);
    let href = type_href(ty);
    Node::NonTerminal(NonTerminal::new(name, href))
}

fn type_label(ty: &syn::Type) -> String {
    if let syn::Type::Path(p) = ty {
        if let Some(last) = p.path.segments.last() {
            return last.ident.to_string();
        }
    }
    quote!(#ty).to_string()
}

fn type_href(ty: &syn::Type) -> Option<String> {
    Some(format!("{}.html", type_label(ty)))
}
```

Import `syn::Attribute` and `syn::Type` at the top.

Also remove the `todo!` from `parse_type_attrs` if any remains.

**Step 4: Run — expect PASS**

Run: `cargo test -p recursa-diagram`

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): struct fields render as sequence"
```

---

### Task 15: Combinator recognition (`Option`, `Seq`, `Surrounded`, `Box`)

**Files:**
- Modify: `recursa-diagram/src/macro_impl.rs`
- Create: `recursa-diagram/tests/trybuild/combinators.rs`

**Step 1: Failing test**

`recursa-diagram/tests/trybuild/combinators.rs`:

```rust
use recursa_diagram::railroad;

pub struct Expr;
pub struct Comma;
pub struct LParen;
pub struct RParen;
pub struct FromClause;

// Dummy stand-ins for recursa combinators — the macro only cares about the
// outermost path segment, not the actual trait bounds.
pub struct Option<T>(core::marker::PhantomData<T>);
pub struct Seq<T, Sep>(core::marker::PhantomData<(T, Sep)>);
pub struct Surrounded<L, I, R>(core::marker::PhantomData<(L, I, R)>);

#[railroad]
pub struct FuncCall {
    name: Expr,
    args: Surrounded<LParen, Seq<Expr, Comma>, RParen>,
    from: Option<FromClause>,
}

fn main() {}
```

Add `compiles_combinators` test entry in `macro_tests.rs`.

**Step 2: Run — expect FAIL** (combinators not recognized — get rendered as labeled boxes of `Option`/`Seq`/`Surrounded`).

Actually this *compiles* already since the test is `t.pass(...)`. To make this meaningful, add a runtime assertion on the generated SVG. That means using a test binary that checks the doc attribute on the generated type.

Simpler approach — forget trybuild here, use an inline test:

```rust
// recursa-diagram/tests/combinator_shapes.rs
// This test walks the layout construction directly via a helper the macro_impl exposes for testing.
```

Since `macro_impl` currently isn't public, either expose it via a `#[cfg(test)]` helper or write the test against a helper function.

Add to `recursa-diagram/src/macro_impl.rs`:

```rust
#[cfg(any(test, feature = "__test_hooks"))]
pub fn build_node_for_tokens(item: TokenStream) -> syn::Result<Node> {
    let input: DeriveInput = parse2(item)?;
    build_node(&input, &TypeAttrs::default())
}
```

And expose the module for tests:

```rust
#[doc(hidden)]
pub mod macro_impl;
```

Then:

`recursa-diagram/tests/combinator_shapes.rs`:

```rust
use recursa_diagram::layout::Node;
use quote::quote;

#[test]
fn option_renders_as_optional() {
    let tokens = quote! {
        pub struct S { x: Option<Foo> }
    };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(seq) => {
            assert_eq!(seq.children.len(), 1);
            assert!(matches!(seq.children[0], Node::Optional(_)));
        }
        _ => panic!("expected Sequence, got {:?}", node),
    }
}

#[test]
fn seq_renders_as_one_or_more() {
    let tokens = quote! { pub struct S { xs: Seq<Foo, Comma> } };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(seq) => assert!(matches!(seq.children[0], Node::OneOrMore(_))),
        _ => panic!(),
    }
}

#[test]
fn surrounded_renders_as_sequence_of_three() {
    let tokens = quote! { pub struct S { g: Surrounded<LParen, Foo, RParen> } };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(outer) => match &outer.children[0] {
            Node::Sequence(inner) => assert_eq!(inner.children.len(), 3),
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn box_is_transparent() {
    let tokens = quote! { pub struct S { b: Box<Foo> } };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(seq) => match &seq.children[0] {
            Node::NonTerminal(nt) => assert_eq!(nt.text, "Foo"),
            _ => panic!("expected unwrapped Foo, got {:?}", seq.children[0]),
        },
        _ => panic!(),
    }
}
```

Add `quote` to `recursa-diagram` dev-dependencies.

**Step 3: Run — expect FAIL**

**Step 4: Implement recognition**

Replace `node_for_field_type`:

```rust
fn node_for_field_type(ty: &syn::Type, field_attrs: &FieldAttrs) -> Node {
    if let Some(label) = &field_attrs.label {
        return Node::NonTerminal(NonTerminal::new(label, None));
    }
    recognize(ty)
}

fn recognize(ty: &syn::Type) -> Node {
    if let Some((ident, args)) = outer_generic(ty) {
        match ident.to_string().as_str() {
            "Option" if args.len() == 1 => {
                return Node::Optional(crate::layout::Optional::new(recognize(&args[0])));
            }
            "Seq" if args.len() == 2 => {
                let child = recognize(&args[0]);
                let sep = recognize(&args[1]);
                return Node::OneOrMore(crate::layout::OneOrMore::new(child, Some(sep)));
            }
            "Surrounded" if args.len() == 3 => {
                return Node::Sequence(crate::layout::Sequence::new(vec![
                    recognize(&args[0]),
                    recognize(&args[1]),
                    recognize(&args[2]),
                ]));
            }
            "Box" | "Rc" | "Arc" if args.len() == 1 => {
                return recognize(&args[0]);
            }
            _ => {}
        }
    }
    let name = type_label(ty);
    let href = Some(format!("{}.html", name));
    Node::NonTerminal(NonTerminal::new(name, href))
}

fn outer_generic(ty: &syn::Type) -> Option<(&syn::Ident, Vec<syn::Type>)> {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                let args: Vec<_> = ab.args.iter().filter_map(|a| {
                    if let syn::GenericArgument::Type(t) = a { Some(t.clone()) } else { None }
                }).collect();
                return Some((&seg.ident, args));
            }
        }
    }
    None
}
```

**Step 5: Run — expect PASS**

**Step 6: Commit**

```bash
git commit -am "feat(recursa-diagram): recognize Option, Seq, Surrounded, Box"
```

---

### Task 16: Enum → choice

**Files:**
- Modify: `recursa-diagram/src/macro_impl.rs`
- Modify: `recursa-diagram/tests/combinator_shapes.rs`

**Step 1: Failing test**

```rust
#[test]
fn enum_renders_as_choice() {
    let tokens = quote! {
        pub enum Statement {
            Select(SelectStmt),
            Insert(InsertStmt),
            Update(UpdateStmt),
        }
    };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Choice(ch) => assert_eq!(ch.children.len(), 3),
        _ => panic!("expected Choice, got {:?}", node),
    }
}
```

**Step 2: Run — expect FAIL** (enum branch returns a bare NonTerminal).

**Step 3: Implement**

```rust
Data::Enum(e) => {
    let children: Vec<Node> = e.variants.iter().map(|v| {
        // Per CLAUDE.md, every variant is a single-field tuple variant. Render the inner type.
        if let Fields::Unnamed(u) = &v.fields {
            if u.unnamed.len() == 1 {
                return recognize(&u.unnamed[0].ty);
            }
        }
        // Fallback for unit / multi-field variants: label with variant name.
        Node::NonTerminal(NonTerminal::new(v.ident.to_string(), None))
    }).collect();
    if children.is_empty() {
        return Err(syn::Error::new_spanned(input, "empty enum"));
    }
    Ok(Node::Choice(Choice::new(0, children)))
}
```

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): enum variants render as choice"
```

---

### Task 17: `#[railroad(skip)]` and label/skip conflict

**Files:**
- Modify: `recursa-diagram/tests/combinator_shapes.rs`
- Create: `recursa-diagram/tests/trybuild/conflict.rs`

**Step 1: Failing tests**

Add to `combinator_shapes.rs`:

```rust
#[test]
fn skip_attribute_omits_field() {
    let tokens = quote! {
        pub struct S {
            a: Foo,
            #[railroad(skip)] b: Bar,
            c: Baz,
        }
    };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(seq) => {
            assert_eq!(seq.children.len(), 2);
            match (&seq.children[0], &seq.children[1]) {
                (Node::NonTerminal(a), Node::NonTerminal(c)) => {
                    assert_eq!(a.text, "Foo");
                    assert_eq!(c.text, "Baz");
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn field_label_overrides_type_name() {
    let tokens = quote! {
        pub struct S { #[railroad(label = "SELECT")] kw: SelectKw }
    };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(seq) => match &seq.children[0] {
            Node::NonTerminal(nt) => assert_eq!(nt.text, "SELECT"),
            _ => panic!(),
        },
        _ => panic!(),
    }
}
```

For the conflict case, use `trybuild` compile-fail:

`recursa-diagram/tests/trybuild/conflict.rs`:

```rust
use recursa_diagram::railroad;

#[railroad]
pub struct S {
    #[railroad(label = "X", skip)] field: u32,
}

fn main() {}
```

Create alongside it `recursa-diagram/tests/trybuild/conflict.stderr` by running `TRYBUILD=overwrite cargo test -p recursa-diagram` after the compile-fail test is added.

Add test entry:

```rust
#[test]
fn rejects_label_and_skip() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/conflict.rs");
}
```

**Step 2: Run — expect FAIL** (label override and skip behaviors exist per earlier code but trybuild stderr fixture missing; the positive tests should pass if prior tasks are correct).

**Step 3: Generate the stderr fixture**

Run: `TRYBUILD=overwrite cargo test -p recursa-diagram --test macro_tests rejects_label_and_skip`

Inspect the generated `conflict.stderr` to confirm the error message is reasonable.

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git add recursa-diagram
git commit -m "feat(recursa-diagram): support skip attribute and conflict detection"
```

---

### Task 18: Best-effort relative hrefs for types in other modules

**Files:**
- Modify: `recursa-diagram/src/macro_impl.rs`
- Modify: `recursa-diagram/tests/combinator_shapes.rs`

**Step 1: Failing test**

```rust
#[test]
fn qualified_path_becomes_relative_href() {
    let tokens = quote! {
        pub struct S { x: crate::ast::other::Thing }
    };
    let node = recursa_diagram::macro_impl::build_node_for_tokens(tokens).unwrap();
    match node {
        Node::Sequence(seq) => match &seq.children[0] {
            Node::NonTerminal(nt) => {
                assert_eq!(nt.text, "Thing");
                let href = nt.href.as_deref().unwrap();
                // Relative path — adjust the exact shape based on what looks right.
                assert!(href.ends_with("Thing.html"));
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}
```

**Step 2: Run — expect FAIL** (current `type_href` doesn't handle qualified paths).

**Step 3: Implement**

Rewrite `type_href`:

```rust
fn type_href(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(p) = ty {
        let segs: Vec<_> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs.is_empty() { return None; }
        let name = segs.last().unwrap();
        if segs.len() == 1 {
            return Some(format!("{}.html", name));
        }
        // Skip a leading `crate` segment; intermediate modules become `../`.
        let start = if segs[0] == "crate" { 1 } else { 0 };
        let mid = &segs[start..segs.len() - 1];
        let ups: String = mid.iter().map(|_| "../").collect();
        // Intermediate modules as dirs in the rustdoc tree: mod_a/mod_b/Thing.html
        let mid_path = mid.join("/");
        if mid_path.is_empty() {
            Some(format!("{}.html", name))
        } else {
            Some(format!("{}{}/{}.html", ups, mid_path, name))
        }
    } else {
        None
    }
}
```

Note: rustdoc's actual path scheme is `module/TypeName.html` relative to the current module, so the "../" prefix is correct when navigating up and then back down.

**Step 4: Run — expect PASS**

**Step 5: Commit**

```bash
git commit -am "feat(recursa-diagram): best-effort relative hrefs for qualified paths"
```

---

## Phase 5 — pg-sql integration

### Task 19: Apply `#[railroad]` to a single pg-sql type as a smoke test

**Files:**
- Modify: `pg-sql/Cargo.toml` (add dependency on `recursa-diagram`)
- Modify: one existing type in `pg-sql/src/ast/` (pick `SelectStmt` or the most central statement type — use `rg '#\[derive\(Parse\)\]' pg-sql/src/ast` to find a good candidate)

**Step 1: Add dependency**

In `pg-sql/Cargo.toml`:

```toml
recursa-diagram = { path = "../recursa-diagram" }
```

**Step 2: Add the attribute**

Find one self-contained AST type and add `use recursa_diagram::railroad;` and `#[railroad]` above the type. Keep it to one type for this smoke-test task.

**Step 3: Build docs**

Run: `cargo doc -p pg-sql --no-deps --open`
Expected: pg-sql's rustdoc opens; navigate to the type you annotated and confirm an SVG is rendered in its documentation.

**Step 4: Inspect visually**

Write down what looks wrong (overlapping rails, misaligned text, etc.) — these become bug reports for the layout algorithm, not blockers for this task. The smoke-test goal is only: is there a diagram in rustdoc at all?

**Step 5: Commit**

```bash
git add pg-sql/Cargo.toml pg-sql/src
git commit -m "feat(pg-sql): smoke-test #[railroad] on one ast type"
```

---

### Task 20: Apply to keyword definition macro

**Files:**
- Modify: wherever pg-sql defines its keyword types (use `rg 'macro_rules! .*[Kk]eyword' pg-sql/src` or `rg 'struct.*Kw' pg-sql/src/ast` to locate)

**Step 1: Find the keyword-defining macro**

If pg-sql has a `define_keyword!` macro or similar, extend it to inject `#[railroad(label = "SELECT")]` (using the literal string it already knows). If keywords are declared individually, add the attribute to each.

**Step 2: Build docs and inspect a few keyword pages**

Run: `cargo doc -p pg-sql --no-deps --open`
Open the rustdoc pages for a few keyword types. Confirm each shows a rounded box with the literal keyword text.

**Step 3: Commit**

```bash
git commit -am "feat(pg-sql): #[railroad(label)] on keyword types"
```

---

### Task 21: Sweep remaining Parse types

**Files:**
- Modify: `pg-sql/src/ast/**`

**Step 1: Enumerate types**

Run: `rg -l '#\[derive\(Parse' pg-sql/src/ast`

**Step 2: Add `#[railroad]` to each**

For each file, add `use recursa_diagram::railroad;` at the top and `#[railroad]` above every Parse-derived type definition. Leave keyword types (already handled in Task 20) alone.

**Step 3: Build**

Run: `cargo build -p pg-sql`
Expected: clean build.

**Step 4: Build docs**

Run: `cargo doc -p pg-sql --no-deps`
Expected: clean, no errors.

**Step 5: Commit**

```bash
git add pg-sql
git commit -m "feat(pg-sql): apply #[railroad] across all ast types"
```

---

## Phase 6 — Polish and review

### Task 22: Code review

Invoke the code-reviewer sub-agent on the changes. See `cipherpowers:requesting-code-review`.

Areas to flag for focused review:
- SVG output: is it well-formed, self-contained, and visually acceptable? Check a rendered example in a browser.
- Crate boundaries: does `recursa-diagram` avoid leaking into `recursa-derive` or `recursa-core`?
- Attribute parsing: does the macro handle malformed input with clear `compile_error!` messages rather than panics?
- Intra-doc hrefs: do any point at paths rustdoc can't resolve?

Address review findings as follow-up commits.

---

### Task 23: Retrospective

After the branch is merged, invoke the retrospective-writer sub-agent via `/summarise` to capture decisions, dead ends, and things worth knowing for next time.

---

## YAGNI cuts confirmed in the design

- **No CLI.** `pg-sql diagram …` is not built.
- **No feature flag on `recursa-diagram`.**
- **No precedence visualization for Pratt enums** — flat choice only.
- **No fully inlined recursive expansion** — one level deep always.
- **Layout polish is iterative**, not upfront. First pass can look rough; fix in follow-up commits once the smoke test in Task 19 reveals real problems.
