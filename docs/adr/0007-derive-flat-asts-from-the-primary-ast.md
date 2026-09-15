# Derive Flat ASTs from the primary AST

Status: source-of-truth decision superseded by [ADR 0010](0010-declare-nodes-independently-of-ast-representation.md)

The original rationale and milestone notes follow. Node declarations now replace authored nested
Rust types as the canonical source; the flat-specific design is outside the declaration redesign.

The authored primary AST remains the canonical grammar representation from which the parser is
derived. Generate the corresponding Flat AST structs and enums mechanically from that primary
representation, including typed-index relationships such as `items: Vec<Item>` becoming
`items: Vec<ItemIndex>`, so grammar authors maintain one schema. This work is an experiment in
parse performance; the ability to modify the AST and render the modified value back to SQL must
remain available.

Flat values are constructed directly during LR reductions. The first milestone supports read-only
inspection and direct rendering; design of invariant-preserving mutation follows after parsing
works. The field transformation, storage/API decisions, and acceptance criteria are recorded in
[the Flat AST design](../flat-ast-design.md).
