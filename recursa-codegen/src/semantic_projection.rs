#![allow(
    dead_code,
    reason = "private source analysis semantic projections are consumed by acceptance fixtures and generation"
)]
#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Stable, location-free projections of analyzed grammar meaning.
//!
//! Projections are review artifacts, not serialization or generated output.
//! They omit source locations and incidental dense IDs while retaining the
//! facts whose changes require semantic review.

use std::fmt::Write as _;

use crate::ir::ContainerCardinality;
use crate::materialization::{
    AttachmentUnitCorrelation, LoweredGrammar, MaterializedFieldLowering,
};
use crate::resolution::ResolvedNodeBody;
use crate::source_analysis::AnalyzedGrammar;
use crate::source_model::TokenDefinitionKind;
use crate::{GrammarDescription, KindSet, SiteId, TypeShape};

/// Renders semantic projection in canonical order.
pub(crate) fn canonical_semantic_projection(analyzed: &AnalyzedGrammar) -> String {
    // Project only source-independent meaning and sort map-like facts so the artifact remains location and ID neutral.
    let lowered = &analyzed.lowered;
    let grammar = &lowered.grammar;
    let atom_kinds = crate::lr::lowering::expand_atoms(grammar);
    let (first, nullable) = crate::lr::lowering::first_sets(grammar, &atom_kinds);
    let mut rendered = String::from("recursa-semantic-projection-v1\n");
    let build = &lowered.source_modeled.resolved().build_inputs;
    writeln!(
        rendered,
        "grammar edition={:?} anchor={} equivalence={:?}",
        build.discovery.edition,
        build.module_anchor.segments.join("::"),
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
            render_shape(owner, &description.shape, lowered),
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
            render_shape(correlation.lowered, &description.shape, lowered),
        )
        .expect("writing to a String cannot fail");
    }

    rendered.push_str("\n[lowering-correlations]\n");
    for correlation in &lowered.lowering_map.fields {
        let source = source_field_name(lowered, correlation.application, correlation.field);
        let MaterializedFieldLowering::ParseSite(site) = correlation.lowered;
        let site = projected_field_site(lowered, correlation.application, correlation.field, site);
        let target = format!("parse-site({})", projected_site_name(site, lowered));
        writeln!(rendered, "field {source} -> {target}").expect("writing to a String cannot fail");
    }

    rendered.push_str("\n[analysis]\n");
    let mut facts = grammar
        .types
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            attachment_for_unit(crate::TypeId::new(*index as u32), lowered).is_none()
        })
        .map(|(index, description)| (description.name.as_str(), index))
        .collect::<Vec<_>>();
    facts.sort_unstable();
    for (name, index) in facts {
        writeln!(
            rendered,
            "facts {name} first=[{}] nullable={}",
            render_kinds(&first[index], grammar),
            nullable[index],
        )
        .expect("writing to a String cannot fail");
    }

    rendered
}

/// Selects the authored site corresponding to a projected semantic field.
fn projected_field_site(
    lowered: &LoweredGrammar,
    _application: crate::materialization::MaterializedApplicationId,
    _field: crate::resolution::ResolvedFieldId,
    outer: SiteId,
) -> SiteId {
    let Some(attachment) = attachment_for_outer(outer, lowered) else {
        return outer;
    };
    attachment.sites[attachment.semantic_index]
}

/// Returns the authored field name, falling back to its positional index.
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

/// Renders fixture projection in canonical order.
pub(crate) fn canonical_fixture_projection(name: &str, analyzed: &AnalyzedGrammar) -> String {
    let mut rendered = format!(
        "provenance recursa-spike=b410d2f123cd7510a8a40e9e5b971936a2fd8463 source=src/grammars/{name}.rs description=src/descriptions/{name}.rs\ntransform fixed-wrappers=positional-or-presence sql=flat-ll2 precedence=declared repetitions=typed-cardinality\n",
    );
    rendered.push_str(&canonical_semantic_projection(analyzed));
    rendered
}

/// Appends one normalized grammar shape and its semantic decisions.
fn render_shape(owner: crate::TypeId, shape: &TypeShape, lowered: &LoweredGrammar) -> String {
    let grammar = &lowered.grammar;
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
            format!("alias({})", render_site(*site, lowered))
        }
        TypeShape::Sequence { sites } => format!(
            "sequence({})",
            sites
                .iter()
                .flat_map(|site| render_projected_sites(*site, lowered))
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
                        .flat_map(|site| render_projected_sites(*site, lowered))
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

/// Appends source-aware parse sites in normalized order.
fn render_projected_sites(site: SiteId, lowered: &LoweredGrammar) -> Vec<String> {
    direct_attachment_unit(site, lowered).map_or_else(
        || vec![render_site(site, lowered)],
        |inner| {
            inner
                .iter()
                .map(|inner| render_site(*inner, lowered))
                .collect()
        },
    )
}

/// Appends one parse site with its source correlation.
fn render_site(site: SiteId, lowered: &LoweredGrammar) -> String {
    let grammar = &lowered.grammar;
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
    let target_name = if let Some(attachment) = attachment_for_unit(site.target, lowered) {
        format!(
            "attachment({})",
            attachment
                .sites
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
        "{}:{} target={} cardinality={} separator={}",
        projected_site_name(site_id, lowered),
        projected_owner_name(site.owner, lowered),
        target_name,
        cardinality,
        semantics.separator.map_or("-", |separator| {
            grammar.types[separator.type_id.index()].name.as_str()
        }),
    )
}

/// Renders a token-kind set in stable identity order.
fn render_kinds(kinds: &KindSet, grammar: &GrammarDescription) -> String {
    let mut names = kinds
        .iter()
        .map(|kind| grammar.kinds[kind.index()].name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.join(",")
}

/// Resolves a normalized declaration identity to its stable projected name.
fn declaration_name(id: crate::DeclId, grammar: &GrammarDescription) -> &str {
    &grammar.declarations[id.index()].name
}

/// Finds the source attachment whose repeated unit owns the projected type.
fn attachment_for_unit(
    unit: crate::TypeId,
    lowered: &LoweredGrammar,
) -> Option<&AttachmentUnitCorrelation> {
    lowered
        .lowering_map
        .attachments
        .iter()
        .find(|attachment| attachment.unit == unit)
}

/// Finds the source attachment whose outer parse site matches the projection.
fn attachment_for_outer(
    outer: SiteId,
    lowered: &LoweredGrammar,
) -> Option<&AttachmentUnitCorrelation> {
    lowered
        .lowering_map
        .attachments
        .iter()
        .find(|attachment| attachment.outer == outer)
}

/// Finds a directly correlated attachment unit for a normalized type.
fn direct_attachment_unit(site: SiteId, lowered: &LoweredGrammar) -> Option<&[SiteId]> {
    let parsed = &lowered.grammar.sites[site.index()];
    if parsed
        .container
        .semantics()
        .is_none_or(|semantics| semantics.cardinality != ContainerCardinality::One)
    {
        return None;
    }
    attachment_for_outer(site, lowered).map(|attachment| attachment.sites.as_slice())
}

/// Finds an attachment outer site from one of its nested semantic sites.
fn attachment_outer_for_inner(
    site: SiteId,
    lowered: &LoweredGrammar,
) -> Option<(&AttachmentUnitCorrelation, usize)> {
    lowered
        .lowering_map
        .attachments
        .iter()
        .find_map(|attachment| {
            attachment
                .sites
                .iter()
                .position(|candidate| *candidate == site)
                .map(|index| (attachment, index))
        })
}

/// Produces the stable source-aware label for one projected parse site.
fn projected_site_name(site: SiteId, lowered: &LoweredGrammar) -> String {
    let (authored, inner_offset) = attachment_outer_for_inner(site, lowered)
        .map_or((site, 0), |(attachment, index)| (attachment.outer, index));
    let grammar = &lowered.grammar;
    for description in &grammar.types {
        match &description.shape {
            TypeShape::Sequence { sites } => {
                if let Some(index) = expanded_site_index(sites, authored, lowered) {
                    return format!("{}::element{}", description.name, index + inner_offset);
                }
            }
            TypeShape::Enum { alternatives } => {
                for alternative in alternatives {
                    if let Some(index) = expanded_site_index(&alternative.sites, authored, lowered)
                    {
                        return format!(
                            "{}::element{}",
                            declaration_name(alternative.declaration, grammar),
                            index + inner_offset,
                        );
                    }
                }
            }
            TypeShape::Token { .. } | TypeShape::Alias { .. } => {}
        }
    }
    semantic_site_name(site, grammar)
}
/// Maps a nested semantic index to its expanded source element position.
fn expanded_site_index(
    siblings: &[SiteId],
    target: SiteId,
    lowered: &LoweredGrammar,
) -> Option<usize> {
    let mut ordinal = 0;
    for site in siblings {
        if *site == target {
            return Some(ordinal);
        }
        ordinal += direct_attachment_unit(*site, lowered).map_or(1, <[SiteId]>::len);
    }
    None
}

/// Resolves a projected site's owning normalized type to its display name.
fn projected_owner_name(owner: crate::TypeId, lowered: &LoweredGrammar) -> &str {
    let grammar = &lowered.grammar;
    let Some(attachment) = attachment_for_unit(owner, lowered) else {
        return &grammar.types[owner.index()].name;
    };
    &grammar.types[grammar.sites[attachment.outer.index()].owner.index()].name
}

/// Produces a source-aware label for a normalized semantic parse site.
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
            TypeShape::Token { .. } | TypeShape::Alias { .. } => {}
        }
    }
    format!(
        "unmapped-site-owner={}",
        grammar.sites[site.index()].owner.index()
    )
}
