# Eliminate Manual Parse Implementations

## Context

There are 19 manual Parse implementations in pg-sql/src/ast/ (excluding RawStatement). Project policy requires derived Parse everywhere — manual impls indicate either a recursa gap or an AST design issue. This plan categorizes each manual impl by blocking pattern and proposes fixes.

## Already Derivable (6 types)

These should work now with Option try-parse and leading-Optional peek fixes already in place.

| Type | File | Why it works now |
|---|---|---|
| UpdateStmt | update.rs | Option try-parse handles optional alias |
| ExplainStmt | explain.rs | Body is `Box<Statement>` — derive handles directly |
| WithStatement | with_clause.rs | Body is `Box<Statement>` — derive handles directly |
| CastType | expr.rs | `Option<TypePrecision>` try-parse handles optional precision |
| CteDefinition | with_clause.rs | `Option<MaterializedOption>` try-parse handles disambiguation |
| InsertStmt | insert.rs | `Option<ColumnList>` try-parse handles optional columns |

**Action**: Convert to derived. No recursa changes needed.

## Enum Restructuring (4 types)

These enums need struct-like variants converted to single-field tuple variants per CLAUDE.md conventions.

| Type | File | Change |
|---|---|---|
| InsertSource | insert.rs | Already has tuple variants — should derive as-is with longest-match |
| SetAssignment | update.rs | Split `Single{...}` / `Tuple{...}` into wrapper structs |
| ArrayExpr | expr.rs | Split `Bracket{...}` into wrapper struct |
| WhenClause | merge.rs | Already has tuple variants — verify ordering for disambiguation |

**Action**: Restructure enums, then derive.

## Bool → Option\<PhantomData\<Keyword\>\> (5 types)

Bool fields recording optional keyword presence prevent both Parse and Visit derivation.

| Type | File | Bool fields |
|---|---|---|
| CreateViewStmt | create_view.rs | `or_replace`, `temp`, `recursive` |
| DropViewStmt | create_view.rs | `if_exists` |
| CreateFunctionStmt | create_function.rs | `or_replace` |
| SetStmt | set_reset.rs | `local` |
| FuncCall | expr.rs | `star_arg` (special case — see Complex section) |

**Action**: Replace `bool` with `Option<PhantomData<Keyword>>`. Also fixes manual Visit impls. FuncCall needs different treatment (see below).

## Vec → Seq\<T, (), OptionalTrailing\> (2 types)

| Type | File | Field |
|---|---|---|
| MergeStmt | merge.rs | `when_clauses: Vec<WhenClause>` → `Seq<WhenClause, (), OptionalTrailing>` |
| ColumnDef | create_table.rs | `constraints: Vec<ColumnConstraint>` (complex — see below) |

**Action**: MergeStmt is straightforward. ColumnDef needs individual treatment.

## First-pattern Through Optionals — Recursa Change (4 types)

Types with optional keyword prefixes before a distinguishing keyword. The derive macro's first_pattern chain stops at Optional fields, producing too-short patterns for enum disambiguation.

Example: `CREATE [OR REPLACE] [TEMP] [RECURSIVE] VIEW` — first_pattern stops at `CREATE`, can't reach `VIEW`.

| Type | File | Prefix pattern |
|---|---|---|
| CreateViewStmt | create_view.rs | `CREATE [OR REPLACE] [TEMP] [RECURSIVE] VIEW` |
| DropViewStmt | create_view.rs | `DROP VIEW [IF EXISTS]` |
| CreateFunctionStmt | create_function.rs | `CREATE [OR REPLACE] FUNCTION` |
| CreateTableStmt | create_table.rs | `CREATE [TEMP] TABLE` |

**Fix**: Modify `derive_parse_named_struct` and `derive_parse_tuple_struct` in `recursa-derive/src/parse_derive.rs` to include `Option<T>` fields as optional regex groups (`(?:sep T::first_pattern())?`) in the first_pattern chain when the inner type is terminal. This lets the pattern reach through optional prefixes to the distinguishing keyword.

**Action**: Fix derive macro first, then restructure bool fields, then derive.

## Complex / Individual Solutions (4 types)

### FuncCall (expr.rs)
`count(*)` detection — `star_arg: bool` records whether args is `*` vs expression list. Can't simply use `Option<PhantomData<Star>>` because `*` conflicts with `Seq<Expr, Comma>` args parsing.

**Fix**: Restructure args as an enum: `FuncArgs::Star(punct::Star)` vs `FuncArgs::Exprs(Option<DistinctKw>, Seq<Expr, Comma>)`.

### ColumnDef (create_table.rs)
Constraints are unordered optional clauses consumed in a loop. Not a homogeneous sequence.

**Fix**: Restructure as individual optional fields: `pk: Option<PrimaryKeyConstraint>`, `not_null: Option<NotNullConstraint>`, `unique: Option<UniqueConstraint>`, etc. Each constraint type is a derivable struct.

### SetOpCombiner (values.rs)
Keyword then modifier: `UNION [ALL|DISTINCT]`. Must consume keyword first, then check modifier.

**Fix**: Restructure SetOp variants as structs containing the keyword + optional modifier. E.g., `UnionAll { _union, _all }` with first_pattern chaining `UNION + ALL`.

### OnConflictClause (insert.rs)
`DO UPDATE` vs `DO NOTHING` dispatch after `ON CONFLICT`.

**Fix**: Restructure ConflictAction as derivable enum with `DoUpdate(DoUpdateAction)` and `DoNothing(DoNothingAction)` where DoNothingAction is `{ _do, _nothing }`. Longest-match-wins: `DO NOTHING` > `DO UPDATE`.

## Implementation Order

1. Convert the 6 already-derivable types (quick wins)
2. Fix first_pattern-through-Optionals in derive macro
3. Restructure bool fields → Option\<PhantomData\> (5 types)
4. Restructure enums (4 types)
5. Vec → Seq conversions (2 types)
6. Complex individual fixes (4 types)

## Verification

After each batch: `cargo test --all --all-targets` — same 12 pre-existing failures, no regressions.
