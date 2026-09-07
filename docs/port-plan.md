# Port plan status

The original staged port plan described a temporary dual-parser experiment and
is no longer an active implementation plan. Recursa now has one implicit
LALR(1) parser as accepted in [ADR 0005](adr/0005-sole-lr-parser.md).

Consumer migrations should use the current facade API, remove obsolete parser
configuration, resolve LALR conflicts in the grammar, and validate against the
acceptance gates in [the parser design](table-driven-parsing.md). Historical
trade-offs are retained in the ADR record rather than as prescriptive porting
instructions.
