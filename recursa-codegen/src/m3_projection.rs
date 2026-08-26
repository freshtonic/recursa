#![allow(
    dead_code,
    reason = "private M3 semantic projections are consumed by acceptance fixtures and M4"
)]

use std::fmt::Write as _;

use crate::decision_plan::{ExpectedRecipe, FollowRecipe};
use crate::ir::ContainerCardinality;
use crate::m3::AnalyzedGrammar;
use crate::materialization::{LoweredGrammar, MaterializedFieldLowering};
use crate::resolution::ResolvedNodeBody;
use crate::source_model::TokenDefinitionKind;
use crate::{FirstPosition, GrammarDescription, KindSet, PrattPosition, SiteId, TypeShape};

pub(crate) fn canonical_semantic_projection(analyzed: &AnalyzedGrammar) -> String {
    let lowered = &analyzed.lowered;
    let grammar = &lowered.grammar;
    let output = analyzed
        .analysis
        .output
        .as_ref()
        .expect("an AnalyzedGrammar contains successful M2 output");
    let mut rendered = String::from("recursa-semantic-projection-v1\n");
    let build = &lowered.source_modeled.resolved().build_inputs;
    writeln!(
        rendered,
        "grammar edition={:?} anchor={} max_lookahead={} equivalence={:?}",
        build.discovery.edition,
        build.module_anchor.segments.join("::"),
        build.max_lookahead,
        build.text_equivalence,
    )
    .expect("writing to a String cannot fail");
    rendered.push_str("\n[lexical]\n");
    rendered.push_str(&lowered.lexical_plan.canonical_lexical_projection());
    rendered.push_str("\n[source-token-definitions]\n");
    for definition in &lowered.source_modeled.source_model().token_definitions {
        match &definition.kind {
            TokenDefinitionKind::Fixed(fixed) => {
                let canonical = lowered
                    .lexical_plan
                    .keywords
                    .iter()
                    .find(|keyword| {
                        lowered
                            .lexical_plan
                            .text_equivalence
                            .equivalent(&keyword.canonical_spelling, &fixed.spelling)
                    })
                    .map_or(fixed.spelling.as_str(), |keyword| {
                        keyword.canonical_spelling.as_str()
                    });
                let mut classes = fixed
                    .authored_classifications
                    .iter()
                    .map(|classification| format!("{:?}", classification.class))
                    .collect::<Vec<_>>();
                classes.sort();
                classes.dedup();
                writeln!(
                    rendered,
                    "fixed {} spelling={:?} canonical={:?} class={}",
                    definition.name,
                    fixed.spelling,
                    canonical,
                    classes.join("|"),
                )
                .expect("writing to a String cannot fail");
            }
            TokenDefinitionKind::Content(content) => {
                writeln!(
                    rendered,
                    "content {} pattern={:?} text={:?}",
                    definition.name, content.pattern, content.text,
                )
                .expect("writing to a String cannot fail");
            }
            TokenDefinitionKind::LexicalBase(base) => {
                writeln!(
                    rendered,
                    "lexical-base {} pattern={:?} priority={:?}",
                    definition.name, base.pattern, base.priority,
                )
                .expect("writing to a String cannot fail");
            }
        }
    }

    rendered.push_str("\n[types]\n");
    let mut terminal_types = grammar
        .types
        .iter()
        .enumerate()
        .filter(|(_, description)| matches!(description.shape, TypeShape::Token { .. }))
        .collect::<Vec<_>>();
    terminal_types.sort_by(|(_, left), (_, right)| left.name.cmp(&right.name));
    for (index, description) in terminal_types {
        let owner = crate::TypeId::new(index as u32);
        writeln!(
            rendered,
            "type {} = {}",
            description.name,
            render_shape(owner, &description.shape, grammar),
        )
        .expect("writing to a String cannot fail");
    }
    for application in &lowered.applications {
        let correlation = lowered
            .lowering_map
            .applications
            .iter()
            .find(|correlation| correlation.application == application.id)
            .expect("every materialized application has a normalized type");
        let description = &grammar.types[correlation.lowered.index()];
        writeln!(
            rendered,
            "type {} = {}",
            description.name,
            render_shape(correlation.lowered, &description.shape, grammar),
        )
        .expect("writing to a String cannot fail");
    }

    rendered.push_str("\n[lowering-correlations]\n");
    for correlation in &lowered.lowering_map.fields {
        let source = source_field_name(lowered, correlation.application, correlation.field);
        let target = match correlation.lowered {
            MaterializedFieldLowering::ParseSite(site) => {
                let site =
                    projected_field_site(lowered, correlation.application, correlation.field, site);
                format!("parse-site({})", projected_site_name(site, grammar))
            }
            MaterializedFieldLowering::PrattLhs { owner, variant } => format!(
                "pratt-lhs(owner={}, variant={})",
                grammar.types[owner.index()].name,
                grammar.declarations[variant.index()].name,
            ),
        };
        writeln!(rendered, "field {source} -> {target}").expect("writing to a String cannot fail");
    }

    rendered.push_str("\n[analysis]\n");
    let mut facts = grammar
        .types
        .iter()
        .enumerate()
        .filter(|(_, description)| !is_attachment_type(description))
        .map(|(index, description)| (description.name.as_str(), index))
        .collect::<Vec<_>>();
    facts.sort_unstable();
    for (name, index) in facts {
        let first = &output.first_by_type()[index];
        let extenders = output.extenders_by_type()[index].as_ref().map_or_else(
            || "-".to_owned(),
            |facts| render_kinds(facts.kinds(), grammar),
        );
        let follow = &output.follow_by_type()[index];
        writeln!(
            rendered,
            "facts {name} first=[{}] nullable={} extenders=[{}] follow=[{}] first^k={}",
            render_kinds(first.kinds(), grammar),
            first.nullable,
            extenders,
            render_follow_facts(follow, grammar),
            render_first_sequences(&first.sequences, grammar),
        )
        .expect("writing to a String cannot fail");
    }

    rendered.push_str("\n[decisions]\n");
    for dispatch in output.decisions().dispatches() {
        writeln!(rendered, "{}", dispatch.canonical_facts(grammar))
            .expect("writing to a String cannot fail");
    }
    for (index, recipe) in output
        .decisions()
        .follow_recipes_by_site()
        .iter()
        .enumerate()
    {
        let site = SiteId::new(index as u32);
        if let Some((outer, _)) = attachment_outer_for_inner(site, grammar) {
            if direct_attachment_unit(outer, grammar).is_some() {
                continue;
            }
            writeln!(
                rendered,
                "follow {} {}",
                projected_site_name(site, grammar),
                render_follow_recipe(recipe, grammar),
            )
            .expect("writing to a String cannot fail");
            continue;
        }
        if let Some(inner) = direct_attachment_unit(site, grammar) {
            for inner_site in inner {
                writeln!(
                    rendered,
                    "follow {} {}",
                    projected_site_name(*inner_site, grammar),
                    render_follow_recipe(
                        &output.decisions().follow_recipes_by_site()[inner_site.index()],
                        grammar,
                    ),
                )
                .expect("writing to a String cannot fail");
            }
        } else {
            writeln!(
                rendered,
                "follow {} {}",
                semantic_site_name(site, grammar),
                render_follow_recipe(recipe, grammar),
            )
            .expect("writing to a String cannot fail");
        }
    }
    for machine in output.decisions().container_machines() {
        if let Some((outer, _)) = attachment_outer_for_inner(machine.site(), grammar) {
            if direct_attachment_unit(outer, grammar).is_some() {
                continue;
            }
            render_container_machine(&mut rendered, machine, grammar, true);
            continue;
        }
        if let Some(inner) = direct_attachment_unit(machine.site(), grammar) {
            for inner_site in inner {
                let inner_machine = output
                    .decisions()
                    .container_machines()
                    .iter()
                    .find(|candidate| candidate.site() == *inner_site)
                    .expect("attachment unit site has a frozen container machine");
                render_container_machine(&mut rendered, inner_machine, grammar, true);
            }
            continue;
        }
        render_container_machine(&mut rendered, machine, grammar, false);
    }

    rendered.push_str("\n[failures]\n");
    for edge in output.decisions().failure_edges() {
        if let Some(site) = edge.site()
            && let Some((outer, _)) = attachment_outer_for_inner(site, grammar)
        {
            if direct_attachment_unit(outer, grammar).is_none() {
                render_failure_edge(&mut rendered, edge, grammar, true);
            }
            continue;
        }
        if let Some(site) = edge.site()
            && let Some(inner) = direct_attachment_unit(site, grammar)
        {
            for inner_site in inner {
                for inner_edge in output
                    .decisions()
                    .failure_edges()
                    .iter()
                    .filter(|candidate| candidate.site() == Some(*inner_site))
                {
                    render_failure_edge(&mut rendered, inner_edge, grammar, true);
                }
            }
            continue;
        }
        render_failure_edge(&mut rendered, edge, grammar, false);
    }
    rendered
}

fn projected_field_site(
    lowered: &LoweredGrammar,
    application: crate::materialization::MaterializedApplicationId,
    field: crate::resolution::ResolvedFieldId,
    outer: SiteId,
) -> SiteId {
    let Some(inner) = direct_attachment_unit(outer, &lowered.grammar) else {
        return outer;
    };
    let before = lowered.applications[application.index()]
        .parsed_fields()
        .find_map(|element| match element {
            crate::materialization::MaterializedElement::SemanticField {
                field: candidate,
                attachment: Some(attachment),
                ..
            } if *candidate == field => Some(attachment.before.len()),
            _ => None,
        })
        .expect("an attachment field correlation retains materialized attachment syntax");
    inner[before]
}

fn render_container_machine(
    rendered: &mut String,
    machine: &crate::decision_plan::ContainerMachine,
    grammar: &GrammarDescription,
    projected_attachment: bool,
) {
    writeln!(
        rendered,
        "container {} {:?} element=[{}] separator={} separator-first=[{}] trailing={} stop={}",
        if projected_attachment {
            projected_site_name(machine.site(), grammar)
        } else {
            semantic_site_name(machine.site(), grammar)
        },
        machine.operation(),
        render_positions(machine.element_first(), grammar),
        machine
            .separator()
            .map_or("-", |separator| grammar.types[separator.type_id.index()]
                .name
                .as_str()),
        render_positions(machine.separator_first(), grammar),
        machine.optional_trailing(),
        render_follow_recipe(machine.stop(), grammar),
    )
    .expect("writing to a String cannot fail");
    for transition in machine.transitions() {
        writeln!(
            rendered,
            "  transition {:?} --{:?}/{}--> {:?}",
            transition.from(),
            transition.action(),
            render_expected(transition.selector(), grammar),
            transition.to(),
        )
        .expect("writing to a String cannot fail");
    }
}

fn render_failure_edge(
    rendered: &mut String,
    edge: &crate::decision_plan::FailureEdge,
    grammar: &GrammarDescription,
    projected_attachment: bool,
) {
    let site_name = edge.site().map_or_else(
        || "-".to_owned(),
        |site| {
            if projected_attachment {
                projected_site_name(site, grammar)
            } else {
                semantic_site_name(site, grammar)
            }
        },
    );
    writeln!(
        rendered,
        "failure {} {} {:?} expected={}",
        projected_owner_name(edge.owner(), grammar),
        site_name,
        edge.kind(),
        render_expected(edge.expected(), grammar),
    )
    .expect("writing to a String cannot fail");
}

fn source_field_name(
    lowered: &LoweredGrammar,
    application_id: crate::materialization::MaterializedApplicationId,
    field_id: crate::resolution::ResolvedFieldId,
) -> String {
    let application = &lowered.applications[application_id.index()];
    let node = lowered
        .source_modeled
        .resolved()
        .nodes
        .iter()
        .find(|node| node.id == application.node)
        .expect("materialized application retains its resolved Node");
    match &node.body {
        ResolvedNodeBody::Struct { .. } => {
            let (ordinal, field) = node
                .fields
                .iter()
                .enumerate()
                .find(|(_, field)| field.id == field_id)
                .expect("field correlation retains its resolved field");
            format!(
                "{}::{}",
                application.display_name,
                field
                    .syntax
                    .ident
                    .as_ref()
                    .map_or_else(|| format!("field{ordinal}"), ToString::to_string),
            )
        }
        ResolvedNodeBody::Enum { variants } => {
            let (variant, ordinal) = variants
                .iter()
                .find_map(|variant| {
                    variant
                        .fields
                        .iter()
                        .position(|field| *field == field_id)
                        .map(|ordinal| (variant, ordinal))
                })
                .expect("field correlation retains its resolved variant field");
            let field = node
                .fields
                .iter()
                .find(|field| field.id == field_id)
                .expect("variant field belongs to the resolved Node");
            format!(
                "{}::{}::{}",
                application.display_name,
                variant.ident,
                field
                    .syntax
                    .ident
                    .as_ref()
                    .map_or_else(|| format!("field{ordinal}"), ToString::to_string),
            )
        }
    }
}

pub(crate) fn canonical_fixture_projection(name: &str, analyzed: &AnalyzedGrammar) -> String {
    let mut rendered = format!(
        "provenance recursa-spike=b410d2f123cd7510a8a40e9e5b971936a2fd8463 source=src/grammars/{name}.rs description=src/descriptions/{name}.rs\ntransform fixed-wrappers=positional-or-presence sql=flat-ll2 pratt=typed-binding-powers repetitions=typed-cardinality\n",
    );
    rendered.push_str(&canonical_semantic_projection(analyzed));
    rendered
}

fn render_shape(owner: crate::TypeId, shape: &TypeShape, grammar: &GrammarDescription) -> String {
    match shape {
        TypeShape::Token {
            atom,
            text_predicate,
        } => format!(
            "token(atom={}, text={})",
            grammar.atoms[atom.index()].name,
            text_predicate.map_or_else(
                || "any".to_owned(),
                |predicate| format!("{:?}", grammar.text_predicates[predicate.index()]),
            ),
        ),
        TypeShape::Alias { site } => {
            format!("alias({})", render_site(*site, grammar))
        }
        TypeShape::Sequence { sites } => format!(
            "sequence({})",
            sites
                .iter()
                .flat_map(|site| render_projected_sites(*site, grammar))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeShape::Enum { alternatives } => format!(
            "enum({})",
            alternatives
                .iter()
                .map(|alternative| format!(
                    "{} => [{}]",
                    declaration_name(alternative.declaration, grammar),
                    alternative
                        .sites
                        .iter()
                        .flat_map(|site| render_projected_sites(*site, grammar))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        TypeShape::Pratt { variants } => format!(
            "pratt({})",
            variants
                .iter()
                .map(|variant| format!(
                    "{} {:?} => [{}]",
                    declaration_name(variant.declaration, grammar),
                    variant.position,
                    variant
                        .sites
                        .iter()
                        .flat_map(|site| render_projected_sites(*site, grammar))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
    .replace(
        &format!("type[{}]", owner.index()),
        &grammar.types[owner.index()].name,
    )
}

fn render_projected_sites(site: SiteId, grammar: &GrammarDescription) -> Vec<String> {
    direct_attachment_unit(site, grammar).map_or_else(
        || vec![render_site(site, grammar)],
        |inner| {
            inner
                .iter()
                .map(|inner| render_site(*inner, grammar))
                .collect()
        },
    )
}

fn render_site(site: SiteId, grammar: &GrammarDescription) -> String {
    let site_id = site;
    let site = &grammar.sites[site_id.index()];
    let semantics = site
        .container
        .semantics()
        .expect("analyzed containers are normalized");
    let cardinality = match semantics.cardinality {
        ContainerCardinality::One => "one",
        ContainerCardinality::Optional => "optional",
        ContainerCardinality::ZeroOrMore => "zero-or-more",
        ContainerCardinality::OneOrMore => "one-or-more",
        ContainerCardinality::OptionalOneOrMore => "optional-one-or-more",
    };
    let target = &grammar.types[site.target.index()];
    let target_name = if is_attachment_type(target) {
        let TypeShape::Sequence { sites } = &target.shape else {
            unreachable!("attachment types are synthetic sequences")
        };
        format!(
            "attachment({})",
            sites
                .iter()
                .map(
                    |site| grammar.types[grammar.sites[site.index()].target.index()]
                        .name
                        .as_str()
                )
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        target.name.clone()
    };
    format!(
        "{}:{} target={} cardinality={} separator={} min_bp={}",
        projected_site_name(site_id, grammar),
        projected_owner_name(site.owner, grammar),
        target_name,
        cardinality,
        semantics.separator.map_or("-", |separator| {
            grammar.types[separator.type_id.index()].name.as_str()
        }),
        site.min_bp,
    )
}

fn render_kinds(kinds: &KindSet, grammar: &GrammarDescription) -> String {
    let mut names = kinds
        .iter()
        .map(|kind| grammar.kinds[kind.index()].name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.join(",")
}

fn render_follow_facts(follow: &crate::FollowFacts, grammar: &GrammarDescription) -> String {
    let mut facts = follow
        .kinds()
        .iter()
        .map(|kind| grammar.kinds[kind.index()].name.clone())
        .collect::<Vec<_>>();
    facts.sort();
    if follow.eof() {
        facts.push("EOF".to_owned());
    }
    facts.join(",")
}

fn render_positions(positions: &[FirstPosition], grammar: &GrammarDescription) -> String {
    positions
        .iter()
        .map(|position| {
            let kinds = render_kinds(&position.kinds, grammar);
            position.text_predicate.map_or(kinds.clone(), |predicate| {
                format!("{kinds}@{:?}", grammar.text_predicates[predicate.index()])
            })
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn render_first_sequences(
    sequences: &[crate::FirstSequence],
    grammar: &GrammarDescription,
) -> String {
    sequences
        .iter()
        .map(|sequence| {
            let mut rendered = sequence
                .positions
                .iter()
                .map(|position| render_positions(std::slice::from_ref(position), grammar))
                .collect::<Vec<_>>()
                .join(" ");
            if sequence.ends() {
                rendered.push_str(" -> caller-follow");
            }
            if rendered.is_empty() {
                rendered.push_str("<empty> -> caller-follow");
            }
            rendered
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn render_follow_recipe(recipe: &FollowRecipe, grammar: &GrammarDescription) -> String {
    match recipe {
        FollowRecipe::Caller => "caller".to_owned(),
        FollowRecipe::Static { first } => {
            format!("static[{}]", render_positions(first, grammar))
        }
        FollowRecipe::StaticAndCaller { first } => {
            format!("static+caller[{}]", render_positions(first, grammar))
        }
    }
}

fn render_expected(expected: &ExpectedRecipe, grammar: &GrammarDescription) -> String {
    match expected {
        ExpectedRecipe::Static(first) => format!("static[{}]", render_positions(first, grammar)),
        ExpectedRecipe::Follow(recipe) => render_follow_recipe(recipe, grammar),
        ExpectedRecipe::ContinueOrStop { continuation, stop } => format!(
            "continue[{}] or stop[{}]",
            render_positions(continuation, grammar),
            render_follow_recipe(stop, grammar),
        ),
        ExpectedRecipe::PrattExtendersAtOrAboveMinBp { branches } => format!(
            "pratt-extenders[{}]",
            branches
                .iter()
                .map(|branch| format!(
                    "{}:{:?}",
                    declaration_name(branch.declaration(), grammar),
                    branch.position().unwrap_or(PrattPosition::Atom),
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn declaration_name(id: crate::DeclId, grammar: &GrammarDescription) -> &str {
    &grammar.declarations[id.index()].name
}

fn is_attachment_type(description: &crate::TypeDescription) -> bool {
    description.name.ends_with("::__recursa_attachment")
}

fn direct_attachment_unit(site: SiteId, grammar: &GrammarDescription) -> Option<&[SiteId]> {
    let site = &grammar.sites[site.index()];
    if site
        .container
        .semantics()
        .is_none_or(|semantics| semantics.cardinality != ContainerCardinality::One)
    {
        return None;
    }
    let target = &grammar.types[site.target.index()];
    if !is_attachment_type(target) {
        return None;
    }
    let TypeShape::Sequence { sites } = &target.shape else {
        unreachable!("attachment types are synthetic sequences")
    };
    Some(sites)
}

fn attachment_outer_for_inner(
    site: SiteId,
    grammar: &GrammarDescription,
) -> Option<(SiteId, usize)> {
    let owner = grammar.sites[site.index()].owner;
    is_attachment_type(&grammar.types[owner.index()]).then(|| {
        let outer = grammar
            .sites
            .iter()
            .enumerate()
            .find(|(_, candidate)| candidate.target == owner)
            .map(|(index, _)| SiteId::new(index as u32))
            .expect("an attachment unit has one authored outer site");
        let TypeShape::Sequence { sites } = &grammar.types[owner.index()].shape else {
            unreachable!("attachment types are synthetic sequences")
        };
        let index = sites
            .iter()
            .position(|candidate| *candidate == site)
            .expect("attachment owner contains its inner site");
        (outer, index)
    })
}

fn projected_site_name(site: SiteId, grammar: &GrammarDescription) -> String {
    let Some((outer, inner_index)) = attachment_outer_for_inner(site, grammar) else {
        return semantic_site_name(site, grammar);
    };
    let outer_name = semantic_site_name(outer, grammar);
    let Some((prefix, index)) = outer_name.rsplit_once("::element") else {
        return format!("{outer_name}::attachment-element{inner_index}");
    };
    let base = index.parse::<usize>().unwrap_or(0);
    format!("{prefix}::element{}", base + inner_index)
}

fn projected_owner_name(owner: crate::TypeId, grammar: &GrammarDescription) -> &str {
    if !is_attachment_type(&grammar.types[owner.index()]) {
        return &grammar.types[owner.index()].name;
    }
    grammar
        .sites
        .iter()
        .find(|site| site.target == owner)
        .map(|site| grammar.types[site.owner.index()].name.as_str())
        .expect("an attachment unit has one authored owner")
}

fn semantic_site_name(site: SiteId, grammar: &GrammarDescription) -> String {
    for description in &grammar.types {
        match &description.shape {
            TypeShape::Alias { site: candidate } if *candidate == site => {
                return format!("{}::alias", description.name);
            }
            TypeShape::Sequence { sites } => {
                if let Some(index) = sites.iter().position(|candidate| *candidate == site) {
                    return format!("{}::element{index}", description.name);
                }
            }
            TypeShape::Enum { alternatives } => {
                for alternative in alternatives {
                    if let Some(index) = alternative
                        .sites
                        .iter()
                        .position(|candidate| *candidate == site)
                    {
                        return format!(
                            "{}::element{index}",
                            declaration_name(alternative.declaration, grammar),
                        );
                    }
                }
            }
            TypeShape::Pratt { variants } => {
                for variant in variants {
                    if let Some(index) = variant
                        .sites
                        .iter()
                        .position(|candidate| *candidate == site)
                    {
                        return format!(
                            "{}::element{index}",
                            declaration_name(variant.declaration, grammar),
                        );
                    }
                }
            }
            TypeShape::Token { .. } | TypeShape::Alias { .. } => {}
        }
    }
    format!(
        "unmapped-site-owner={}",
        grammar.sites[site.index()].owner.index()
    )
}
