# Make the Explorer and document framing strict only

Status: accepted

Supersedes [ADR 0002](0002-separate-explorer-recovery-projection.md).

## Context

ADR 0002 kept construction-free recovery separate from authored AST values.
The recovery projection needed its own generated plan for every root: dispatch
commitments, restart and stop expectation sets, and per-node recovery recipes.
In a large grammar those statics were about 11 MB of generated source that
every ordinary build compiled, and the recovery interpreter was the largest
module of the runtime. The only consumer that relied on recovery was document
framing, which used it to choose a boundary after a strict failure and to
replay failed islands.

## Decision

Strict parsing is the only source operation. `Adapter::parse` returns one
complete Explorer document or structured diagnostics; there is no `explore`
operation, no `Exploration`, and no recovery document. The generator emits no
recovery plan, commitment, restart, or stop data, and decision plans carry no
recovery commitment.

Document framing is strict as well. A candidate boundary ends an island only
when the island parses strictly up to that exact token. When no candidate
completes strictly, the strict attempt over the remaining source names the
failure, and the document is rejected with a `FrameRejection` that carries the
failing island extent, that island's strict diagnostics, and the framing cause
when the island itself parsed but lacked its boundary or payload terminator.
Fatal invariant violations remain `FrameError`; the two are joined by
`FrameFailure`.

## Consequences

Generated source shrinks, the runtime no longer contains an interpreter for
partial structure, and diagnostics keep the strict codes and messages. Invalid
input is reported at its first failing island only; later islands are not
described. Consumers that displayed recovered structure must use the strict
diagnostics instead.
