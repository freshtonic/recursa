# Keep Flat ASTs semantic-only

Status: accepted

Flat AST construction is a parse-performance experiment, so it retains semantic values while
omitting syntax-only tokens, spans, occurrence provenance, and comments. Literals, identifiers,
and semantic presence values remain available for direct editing; rendering generates SQL that
preserves the edited semantics, without preserving whitespace or comment fidelity. Flat parsing
must suppress occurrence capture even where the primary parser would normally require it.

Temporary lexer token positions used to obtain literal/identifier text and locate parse failures
are separate from AST occurrence tracking. They do not become a retained Flat AST sidecar.
