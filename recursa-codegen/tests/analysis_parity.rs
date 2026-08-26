mod fixtures;

use std::collections::BTreeSet;

use recursa_codegen::{
    AnalysisOutput, AnalysisReport, KindId, ProofId, ProvenanceStep, TypeId, analyze,
};

use fixtures::{arith, json};

fn kinds(output: &AnalysisOutput, type_id: TypeId) -> Vec<u16> {
    output
        .first(type_id)
        .unwrap()
        .kinds()
        .iter()
        .map(KindId::get)
        .collect()
}

fn returned_proof_roots(report: &AnalysisReport) -> BTreeSet<ProofId> {
    let mut roots = BTreeSet::new();
    if let Some(output) = &report.output {
        for positions in output.first_provenance_by_type() {
            for position in positions {
                for kind in position.kinds().iter() {
                    roots.insert(position.proof(kind).unwrap());
                }
            }
        }
        for positions in output.extender_provenance_by_type().iter().flatten() {
            for position in positions {
                for kind in position.kinds().iter() {
                    roots.insert(position.proof(kind).unwrap());
                }
            }
        }
        for follow in output
            .follow_by_type()
            .iter()
            .chain(output.follow_by_site())
        {
            roots.extend(
                follow
                    .kinds()
                    .iter()
                    .map(|kind| follow.proof(kind).unwrap()),
            );
            roots.extend(follow.eof_proof());
        }
    }
    for diagnostic in &report.diagnostics {
        if let Some(conflict) = &diagnostic.conflict {
            roots.insert(conflict.witness.left);
            roots.insert(conflict.witness.right);
        }
    }
    roots
}

#[test]
fn returned_proof_arena_contains_only_nodes_reachable_from_public_facts() {
    let report = analyze(&arith::grammar(false));
    let mut reachable = returned_proof_roots(&report);
    let mut pending = reachable.iter().copied().collect::<Vec<_>>();
    while let Some(proof) = pending.pop() {
        if let Some(parent) = report.proofs().get(proof).unwrap().parent
            && reachable.insert(parent)
        {
            pending.push(parent);
        }
    }

    assert_eq!(reachable.len(), report.proofs().nodes().len());
}

#[test]
fn analysis_succeeds_for_arith() {
    let canonical = arith::grammar(false);
    let reordered = arith::grammar(true);
    assert_eq!(canonical.kinds, reordered.kinds);
    assert_eq!(canonical.declarations, reordered.declarations);
    assert_eq!(canonical.types.len(), reordered.types.len());
    for (index, (left, right)) in canonical.types.iter().zip(&reordered.types).enumerate() {
        assert_eq!(TypeId::new(index as u32).index(), index);
        assert_eq!(left.name, right.name);
        assert_eq!(left.declaration, right.declaration);
    }

    let first = analyze(&canonical);
    let repeated = analyze(&arith::grammar(false));
    let shuffled = analyze(&reordered);

    assert!(first.diagnostics.is_empty());
    assert_eq!(first.output, repeated.output);
    assert_eq!(first.output, shuffled.output);
    assert_eq!(first.proofs(), repeated.proofs());
    assert_eq!(first.proofs(), shuffled.proofs());
}

#[test]
fn computed_first_matches_accepted_arith_token_wrappers() {
    let report = analyze(&arith::grammar(false));
    let output = report.output.unwrap();
    for (type_id, kind) in [
        (arith::PLUS_TOK, arith::PLUS),
        (arith::MINUS_TOK, arith::MINUS),
        (arith::STAR_TOK, arith::STAR),
        (arith::SLASH_TOK, arith::SLASH),
        (arith::LPAREN_TOK, arith::LPAREN),
        (arith::RPAREN_TOK, arith::RPAREN),
        (arith::INT_LIT_TOK, arith::INT_LIT),
        (arith::IDENT_TOK, arith::IDENT),
    ] {
        assert_eq!(kinds(&output, type_id), [kind]);
        assert!(!output.first(type_id).unwrap().nullable);
    }
}

#[test]
fn computed_first_matches_accepted_arith_expr() {
    let output = analyze(&arith::grammar(false)).output.unwrap();

    assert_eq!(
        kinds(&output, arith::EXPR),
        [arith::MINUS, arith::LPAREN, arith::INT_LIT, arith::IDENT],
    );
    assert!(!output.first(arith::EXPR).unwrap().nullable);
}

#[test]
fn computed_extenders_matches_accepted_arith_expr() {
    let report = analyze(&arith::grammar(false));
    let output = report.output.as_ref().unwrap();

    assert_eq!(
        output
            .extenders(arith::EXPR)
            .unwrap()
            .kinds()
            .iter()
            .map(KindId::get)
            .collect::<Vec<_>>(),
        [arith::PLUS, arith::MINUS, arith::STAR, arith::SLASH],
    );
    let provenanced = output.extender_provenance(arith::EXPR).unwrap();
    assert_eq!(provenanced.len(), 1);
    assert_eq!(
        provenanced[0].kinds(),
        output.extenders(arith::EXPR).unwrap().kinds()
    );
    for kind in provenanced[0].kinds().iter() {
        let proof = provenanced[0].proof(kind).unwrap();
        let steps = report
            .proofs()
            .chain(proof)
            .map(|node| &node.step)
            .collect::<Vec<_>>();
        assert!(steps.iter().any(|step| matches!(
            step,
            ProvenanceStep::PrattExtender {
                pratt,
                position: recursa_codegen::PrattPosition::Infix { .. }
                    | recursa_codegen::PrattPosition::Postfix { .. },
                ..
            } if *pratt == arith::EXPR
        )));
        assert!(steps.iter().any(|step| matches!(
            step,
            ProvenanceStep::Reference { owner, .. } if *owner == arith::EXPR
        )));
        assert!(matches!(
            steps.last(),
            Some(ProvenanceStep::DirectDeclaration { .. })
        ));
    }
}

#[test]
fn non_pratt_arith_types_have_no_extenders_entry() {
    let output = analyze(&arith::grammar(false)).output.unwrap();

    assert!(output.extenders(arith::PLUS_TOK).is_none());
    assert!(output.extenders(arith::INT_LIT_TOK).is_none());
    assert!(output.extender_provenance(arith::PLUS_TOK).is_none());
    assert!(output.extender_provenance(arith::INT_LIT_TOK).is_none());
    assert_eq!(
        output
            .extenders_by_type()
            .iter()
            .filter(|facts| facts.is_some())
            .count(),
        1,
    );
    assert_eq!(
        output
            .extender_provenance_by_type()
            .iter()
            .filter(|facts| facts.is_some())
            .count(),
        1,
    );
}

#[test]
fn valid_pratt_led_recursion_is_accepted() {
    let grammar = arith::grammar(false);
    assert!(grammar.sites.iter().any(|site| site.target == arith::EXPR));
    let report = analyze(&grammar);

    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

#[test]
fn arithmetic_pratt_operand_sites_carry_binding_power_thresholds() {
    let grammar = arith::grammar(false);
    let recursa_codegen::TypeShape::Pratt { variants } = &grammar.types[arith::EXPR.index()].shape
    else {
        panic!("Expr is Pratt");
    };
    let neg = variants
        .iter()
        .find(|variant| {
            matches!(
                variant.position,
                recursa_codegen::PrattPosition::Prefix { .. }
            )
        })
        .unwrap();
    let add = variants
        .iter()
        .find(|variant| {
            matches!(
                variant.position,
                recursa_codegen::PrattPosition::Infix { lbp: 10, rbp: 11 }
            )
        })
        .unwrap();

    assert_eq!(grammar.sites[neg.sites[1].index()].min_bp, 30);
    assert_eq!(grammar.sites[add.sites[1].index()].min_bp, 11);
}

#[test]
fn arith_canonical_facts_are_stable_and_sorted() {
    let output = analyze(&arith::grammar(false)).output.unwrap();

    assert_eq!(
        output.canonical_facts(),
        concat!(
            "type[0] FIRST={0} NULLABLE=false EXTENDERS=-\n",
            "type[1] FIRST={1} NULLABLE=false EXTENDERS=-\n",
            "type[2] FIRST={2} NULLABLE=false EXTENDERS=-\n",
            "type[3] FIRST={3} NULLABLE=false EXTENDERS=-\n",
            "type[4] FIRST={4} NULLABLE=false EXTENDERS=-\n",
            "type[5] FIRST={5} NULLABLE=false EXTENDERS=-\n",
            "type[6] FIRST={6} NULLABLE=false EXTENDERS=-\n",
            "type[7] FIRST={7} NULLABLE=false EXTENDERS=-\n",
            "type[8] FIRST={1,4,6,7} NULLABLE=false EXTENDERS={0,1,2,3}\n",
        ),
    );
}

#[test]
fn arith_expr_contains_atom_starters_and_prefix_minus() {
    let output = analyze(&arith::grammar(false)).output.unwrap();

    assert_eq!(
        kinds(&output, arith::EXPR),
        [arith::MINUS, arith::LPAREN, arith::INT_LIT, arith::IDENT],
    );
}

#[test]
fn analysis_succeeds_for_json() {
    let report = analyze(&json::grammar());

    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

#[test]
fn json_has_no_left_recursion_diagnostic() {
    let report = analyze(&json::grammar());

    assert!(report.diagnostics.is_empty());
}

#[test]
fn computed_first_matches_accepted_json_value() {
    let output = analyze(&json::grammar()).output.unwrap();

    assert_eq!(kinds(&output, json::VALUE), [0, 2, 6, 7, 8, 9, 10]);
    assert!(!output.first(json::VALUE).unwrap().nullable);
}

#[test]
fn computed_first_matches_accepted_json_object() {
    let output = analyze(&json::grammar()).output.unwrap();

    assert_eq!(kinds(&output, json::OBJECT), [0]);
    assert!(!output.first(json::OBJECT).unwrap().nullable);
}

#[test]
fn computed_first_matches_accepted_json_array() {
    let output = analyze(&json::grammar()).output.unwrap();

    assert_eq!(kinds(&output, json::ARRAY), [2]);
    assert!(!output.first(json::ARRAY).unwrap().nullable);
}

#[test]
fn computed_first_matches_accepted_json_member() {
    let output = analyze(&json::grammar()).output.unwrap();

    assert_eq!(kinds(&output, json::MEMBER), [6]);
    assert!(!output.first(json::MEMBER).unwrap().nullable);
}

#[test]
fn json_has_no_extenders() {
    let output = analyze(&json::grammar()).output.unwrap();

    assert!(output.extenders_by_type().iter().all(Option::is_none));
}

#[test]
fn nullable_is_false_for_every_json_type() {
    let output = analyze(&json::grammar()).output.unwrap();

    assert!(output.first_by_type().iter().all(|facts| !facts.nullable));
}

#[test]
fn json_value_first_is_the_union_of_each_variant() {
    let output = analyze(&json::grammar()).output.unwrap();
    let mut variant_union = recursa_codegen::KindSet::empty(11);
    for type_id in [
        json::OBJECT,
        json::ARRAY,
        json::STRING_LITERAL,
        json::NUMBER_LITERAL,
        json::BOOL_LITERAL,
        TypeId::new(10),
    ] {
        variant_union = variant_union
            .union(output.first(type_id).unwrap().kinds())
            .unwrap();
    }

    assert_eq!(output.first(json::VALUE).unwrap().kinds(), &variant_union);
}
