//! Validates finite nested storage after generic applications and aliases have been closed.

use crate::materialization::{ClosedType, MaterializedFieldView, MaterializedNodeApplication};
use crate::resolution::ResolvedFieldId;

/// One by-value dependency, retaining the authored field that introduces it.
struct LayoutEdge {
    /// Dense application index whose layout must be embedded in the owner.
    target: usize,
    /// Source field to blame if this dependency closes an infinite-size cycle.
    field: ResolvedFieldId,
}

/// Reports cycles that require explicit indirection in the nested AST representation.
///
/// Closed applications make generic substitutions and aliases transparent to this check.
/// `Option` embeds its payload; boxes and both vector representations allocate it elsewhere.
/// The iterative traversal retains deterministic source order without consuming one stack
/// frame per declaration in a large grammar. The returned field identities let materialization
/// attach its normal source diagnostics without coupling this layout pass to source discovery.
pub(crate) fn validate_declaration_layouts(
    applications: &[MaterializedNodeApplication],
) -> Vec<(ResolvedFieldId, String)> {
    let graph = applications
        .iter()
        .map(|application| {
            application
                .fields()
                .filter_map(|MaterializedFieldView::Parsed { field, ty, .. }| {
                    inline_target(ty).map(|target| LayoutEdge { target, field })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // White, gray, and black distinguish untouched, active, and completed applications.
    let mut state = vec![0_u8; graph.len()];
    let mut diagnostics = Vec::new();
    for root in 0..graph.len() {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        let mut stack = vec![(root, 0)];
        while let Some((owner, next)) = stack.last_mut() {
            let Some(edge) = graph[*owner].get(*next) else {
                state[*owner] = 2;
                stack.pop();
                continue;
            };
            *next += 1;
            match state[edge.target] {
                0 => {
                    state[edge.target] = 1;
                    stack.push((edge.target, 0));
                }
                1 => {
                    let start = stack
                        .iter()
                        .position(|(index, _)| *index == edge.target)
                        .expect("a gray dependency is on the active layout path");
                    let cycle = stack[start..]
                        .iter()
                        .map(|(index, _)| applications[*index].display_name.as_str())
                        .chain(std::iter::once(
                            applications[edge.target].display_name.as_str(),
                        ))
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    diagnostics.push((
                        edge.field,
                        format!(
                            "recursive AST storage has infinite size ({cycle}); add boxed!(...) to a field on this cycle"
                        ),
                    ));
                }
                _ => {}
            }
        }
    }
    diagnostics
}

/// Follows wrappers that store a complete payload inline, stopping at storage indirection.
fn inline_target(mut ty: &ClosedType) -> Option<usize> {
    loop {
        match ty {
            ClosedType::Node(node) => return Some(node.application.index()),
            ClosedType::Optional(inner) => ty = inner,
            ClosedType::ContentToken(_)
            | ClosedType::Boxed(_)
            | ClosedType::ZeroOrMore(_)
            | ClosedType::OneOrMore(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::discovery::{DiscoveryInput, discover_module_tree};
    use crate::lowering::RustEdition;
    use crate::materialization::{MaterializationReport, materialize};
    use crate::resolution::{ResolutionInput, resolve_local_semantics};
    use crate::source_model::{SourceModelInput, lower_source_model};
    use crate::source_syntax::CargoCfgSnapshot;
    use std::path::Path;

    fn analyze(body: &str, arena: bool) -> (String, MaterializationReport) {
        let source = format!(
            "recursa::grammar! {{ module = crate, {} }}\nrecursa::ast_node! {{ {body} }}",
            if arena { "arena_ast," } else { "" },
        );
        let package = tempfile::TempDir::new().unwrap();
        std::fs::write(package.path().join("lib.rs"), &source).unwrap();
        let cfg = CargoCfgSnapshot::default();
        let discovery = discover_module_tree(DiscoveryInput {
            manifest_dir: package.path(),
            selected_root: Path::new("lib.rs"),
            edition: RustEdition::Rust2024,
            cfg: &cfg,
        });
        assert!(
            discovery.diagnostics.is_empty(),
            "{:?}",
            discovery.diagnostics
        );
        let resolution = resolve_local_semantics(ResolutionInput {
            discovery_inputs: discovery.discovery_inputs,
            tree: discovery.tree.unwrap(),
            cfg: &cfg,
        });
        assert!(
            resolution.diagnostics.is_empty(),
            "{:?}",
            resolution.diagnostics
        );
        let modeled = lower_source_model(SourceModelInput {
            resolved: resolution.resolved.unwrap(),
            cfg: &cfg,
        });
        assert!(modeled.diagnostics.is_empty(), "{:?}", modeled.diagnostics);
        (source, materialize(modeled.source_modeled.unwrap()))
    }

    #[test]
    fn rejects_direct_mutual_optional_and_generic_storage_cycles_at_authored_fields() {
        for body in [
            "struct Root { child: Self }",
            "struct Root { child: Other } struct Other { parent: Root }",
            "struct Root { child: Option<Self> }",
            "struct Wrap<T> { value: T } struct Root { child: Wrap<Self> }",
        ] {
            let (source, report) = analyze(body, false);
            let diagnostic = report
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.message.contains("infinite size"))
                .unwrap_or_else(|| panic!("{body}: {:?}", report.diagnostics));
            assert!(report.lowered.is_none());
            assert!(diagnostic.message.contains("boxed!(...)"));
            let range = diagnostic.range.clone().unwrap();
            assert!(!source[range.start as usize..range.end as usize].is_empty());
        }
    }

    #[test]
    fn explicit_boxes_and_repetitions_break_nested_storage_cycles_in_both_modes() {
        for arena in [false, true] {
            for body in [
                "struct Root { child: Option<boxed!(Self)> }",
                "struct Root { child: zero_or_many!(Self) }",
                "struct Root { child: one_or_many!(Self) }",
                "struct Wrap<T> { value: T } struct Root { child: Wrap<boxed!(Self)> }",
                "struct Root { child: Other } struct Other { parent: boxed!(Root) }",
            ] {
                let (_, report) = analyze(body, arena);
                assert!(
                    report.diagnostics.is_empty(),
                    "{body}: {:?}",
                    report.diagnostics
                );
                assert!(report.lowered.is_some());
            }
        }
    }
}
