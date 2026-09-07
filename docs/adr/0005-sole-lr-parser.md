# Make the LALR parser Recursa's sole parser

Status: accepted

Recursa has one implicit LALR/table-driven parser. The earlier dual-parser experiment established that its value lies in the LR machine, not in keeping a recursive-descent implementation or parser-selection API alive. Remove recursive descent, named parsers, parser markers, `parser_style`, `parsers(...)`, and `max_lookahead`; retain an explicit migration diagnostic for those declarations. Parser construction remains an implementation choice rather than a simultaneous runtime API: a future IELR or canonical-LR option, if needed, selects one LR construction algorithm for the one Recursa parser. This supersedes ADR 0004's new-parser-style, recursive-descent-default, and named-parser decisions; its LALR conflict and table-layout rationale remains historical context.

## Consequences

Every retained grammar, example, and generated behavior fixture is LALR-clean and runs through the same parser. Predictive-dispatch-only behavior and RD/LR equivalence evidence are removed rather than emulated. Text predicates remain rejected until an independently useful LR lowering exists. The generated protocol and emitter identity change so stale dual-parser output cannot link. The completion gates are a full Recursa suite, pg-sql's zero-conflict 234/234 differential, forced generation at most ten seconds, reuse at most one second, and release rebuild below sixty seconds.
