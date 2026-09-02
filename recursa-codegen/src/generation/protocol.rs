//! Private generation protocol, capability handshake, and plan-family identities.
//!
//! These values participate in generation identity and loader compatibility. Keeping
//! their construction together makes protocol drift explicit and reviewable.

use super::*;

/// Private loader protocol understood by generated roots and the facade.
pub(super) const PRIVATE_PROTOCOL: u32 = 13;
/// Stable emitter-format identity hashed into every generated family identity.
pub(super) const EMITTER_IDENTITY: &str = "recursa-generation-compound-v34";
/// Fingerprint of the production generator sources compiled into this codegen build.
const GENERATOR_SOURCE_FINGERPRINT: &str = env!("RECURSA_CODEGEN_SOURCE_FINGERPRINT");

/// Returns the emitter, release, and protocol prefix that invalidates cached Generations.
pub(super) fn generation_protocol_identity() -> String {
    generation_protocol_identity_for(PRIVATE_PROTOCOL, GENERATOR_SOURCE_FINGERPRINT)
}

/// Renders one candidate protocol revision through the production cache-identity format.
fn generation_protocol_identity_for(protocol: u32, generator_fingerprint: &str) -> String {
    format!(
        "{EMITTER_IDENTITY};release={};protocol={protocol};generator={generator_fingerprint}",
        env!("CARGO_PKG_VERSION")
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Generator/facade compatibility handshake embedded into structural loaders.
pub(super) struct PrivateAbiPlan {
    /// Exact Recursa release expected by generated output.
    pub(super) release: String,
    /// Private generation-protocol revision expected by generated output.
    pub(super) protocol: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One facade capability combination accepted by generated structural loaders.
pub(super) struct FacadeCapability {
    /// Whether the facade provides the diagram loader capability.
    pub(super) diagrams: bool,
    /// Whether the facade provides Explorer runtime contracts.
    pub(super) explorer: bool,
    /// Whether the facade provides generated `Arbitrary` support.
    pub(super) arbitrary: bool,
    /// Whether the facade provides generated Serde support.
    pub(super) serde: bool,
}
/// Hashes parse-plan semantic Debug fields in canonical source order without allocation-local spans.
pub(super) fn parse_plan_identity(nodes: &[NodeParsePlan]) -> u64 {
    canonical_debug_identity(nodes)
}

/// Hashes one capability plan without erasing semantic Debug fields or variant order.
pub(super) fn plan_family_identity(value: &impl std::fmt::Debug) -> u64 {
    canonical_debug_identity(value)
}

/// Hashes semantic Debug structure after removing allocation-local proc-macro span offsets.
fn canonical_debug_identity(value: &(impl std::fmt::Debug + ?Sized)) -> u64 {
    let rendered = format!("{value:?}");
    stable_hash(debug_without_span_offsets(&rendered).as_bytes())
}

/// Removes `bytes(N..M)` span renderings outside quoted Debug strings.
///
/// `syn` includes proc-macro allocation offsets in the derived Debug output for identifiers and
/// lifetimes. Those offsets change when the same plan is joined again in one process, but do not
/// describe generated behavior. Span-looking authored strings remain byte-for-byte significant.
fn debug_without_span_offsets(rendered: &str) -> String {
    const PREFIX: &[u8] = b"bytes(";

    let input = rendered.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < input.len() {
        let byte = input[index];
        if in_string {
            output.push(byte);
            index += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
            output.push(byte);
            index += 1;
            continue;
        }
        if input[index..].starts_with(PREFIX)
            && let Some(relative_end) = input[index + PREFIX.len()..]
                .iter()
                .position(|byte| *byte == b')')
        {
            let body_start = index + PREFIX.len();
            let body_end = body_start + relative_end;
            let body = &input[body_start..body_end];
            let separator = body.windows(2).position(|window| window == b"..");
            if separator.is_some_and(|separator| {
                !body[..separator].is_empty()
                    && body[..separator].iter().all(u8::is_ascii_digit)
                    && !body[separator + 2..].is_empty()
                    && body[separator + 2..].iter().all(u8::is_ascii_digit)
            }) {
                output.extend_from_slice(b"bytes(_)");
                index = body_end + 1;
                continue;
            }
        }
        output.push(byte);
        index += 1;
    }
    String::from_utf8(output).expect("normalizing UTF-8 Debug output preserves UTF-8")
}

/// Returns the compatibility handshake that will be embedded in generated output.
pub(super) fn emitted_private_abi_plan() -> PrivateAbiPlan {
    PrivateAbiPlan {
        release: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PRIVATE_PROTOCOL,
    }
}

/// Reconstructs the generator-owned compatibility handshake for validation.
pub(super) fn authoritative_private_abi_plan() -> PrivateAbiPlan {
    // This contract belongs to the facade/codegen release, independently of
    // the mutable emission plan which later renders the handshake.
    PrivateAbiPlan {
        release: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PRIVATE_PROTOCOL,
    }
}

/// Reconstructs complete generated trait behavior from authored selections and intrinsic traits.
pub(super) fn authoritative_configured_trait_identity(
    lowered: &crate::materialization::LoweredGrammar,
    resolved: &crate::resolution::ResolvedSourceGrammar,
) -> u64 {
    let source = lowered.source_modeled.source_model();
    let mut canonical = String::new();
    for (index, resolved_node) in resolved.nodes.iter().enumerate() {
        canonical.push_str(&format!("node-{index}:"));
        if let Some(node) = source
            .nodes
            .iter()
            .find(|node| node.resolved_node == resolved_node.id)
        {
            for selection in &node.configured_traits.effective {
                canonical.push_str(&format!("{:?},", selection.recipe));
            }
        } else {
            canonical.push_str("<missing>");
        }
        canonical.push(';');
    }
    for (index, content) in resolved.content_tokens.iter().enumerate() {
        canonical.push_str(&format!("token-{index}:"));
        if let Some(token) =
            source
                .token_definitions
                .iter()
                .find_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) if token.semantic == content.id => {
                        Some(token)
                    }
                    TokenDefinitionKind::Content(_)
                    | TokenDefinitionKind::Fixed(_)
                    | TokenDefinitionKind::LexicalBase(_) => None,
                })
        {
            append_content_token_trait_identity(
                &mut canonical,
                token
                    .configured_traits
                    .effective
                    .iter()
                    .map(|selection| selection.recipe),
                PRIVATE_CONTENT_TOKEN_SUPPORT_TRAITS
                    .into_iter()
                    .filter(|recipe| {
                        authoritative_content_token_support(lowered, content.id, *recipe)
                    }),
            );
        } else {
            canonical.push_str("<missing>");
        }
        canonical.push(';');
    }
    stable_hash(canonical.as_bytes())
}

/// Hashes complete Node and content-token trait behavior in canonical order.
pub(super) fn configured_trait_plan_identity(
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
) -> u64 {
    let mut canonical = String::new();
    for (index, node) in nodes.iter().enumerate() {
        canonical.push_str(&format!("node-{index}:"));
        for recipe in &node.configured_traits {
            canonical.push_str(&format!("{recipe:?},"));
        }
        canonical.push(';');
    }
    for (index, token) in tokens.iter().enumerate() {
        canonical.push_str(&format!("token-{index}:"));
        append_content_token_trait_identity(
            &mut canonical,
            token.configured_traits.iter().copied(),
            PRIVATE_CONTENT_TOKEN_SUPPORT_TRAITS
                .into_iter()
                .filter(|recipe| content_token_support_required_in(nodes, token.semantic, *recipe)),
        );
        canonical.push(';');
    }
    stable_hash(canonical.as_bytes())
}

/// Records intrinsic content-token traits before their optional configured recipes.
fn append_content_token_trait_identity(
    canonical: &mut String,
    configured: impl IntoIterator<Item = ConfiguredTraitRecipe>,
    support: impl IntoIterator<Item = ConfiguredTraitRecipe>,
) {
    canonical.push_str("intrinsic[");
    for recipe in INTRINSIC_CONTENT_TOKEN_VALUE_TRAITS {
        canonical.push_str(&format!("{recipe:?},"));
    }
    canonical.push_str("];configured[");
    for recipe in configured {
        canonical.push_str(&format!("{recipe:?},"));
    }
    canonical.push_str("];support[");
    for recipe in support {
        canonical.push_str(&format!("{recipe:?},"));
    }
    canonical.push(']');
}

/// Reconstructs one private hidden-token support edge from materialized authored structure.
fn authoritative_content_token_support(
    lowered: &crate::materialization::LoweredGrammar,
    token: ContentTokenId,
    recipe: ConfiguredTraitRecipe,
) -> bool {
    let source = lowered.source_modeled.source_model();
    lowered.applications.iter().any(|application| {
        let configured = source
            .nodes
            .iter()
            .find(|node| node.resolved_node == application.node)
            .is_some_and(|node| {
                node.configured_traits
                    .effective
                    .iter()
                    .any(|selection| selection.recipe == recipe)
            });
        configured
            && application.parsed_fields().any(|element| {
                matches!(
                    element,
                    MaterializedElement::SemanticField {
                        projection: MaterializedSemanticFieldProjection::Direct { ty, .. },
                        ..
                    } if closed_content_token_terminal(ty) == Some(token)
                )
            })
    })
}

/// Resolves a hidden terminal below supported storage wrappers.
fn closed_content_token_terminal(ty: &ClosedType) -> Option<ContentTokenId> {
    match ty {
        ClosedType::ContentToken(token) => Some(*token),
        ClosedType::Boxed(inner)
        | ClosedType::Optional(inner)
        | ClosedType::ZeroOrMore(inner)
        | ClosedType::OneOrMore(inner) => closed_content_token_terminal(inner),
        ClosedType::Node(_) => None,
    }
}

/// Enumerates facade capability combinations that satisfy requested generated behavior.
pub(super) fn accepted_capabilities(
    requested_arbitrary: bool,
    requested_serde: bool,
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let mut accepted = Vec::new();
    for explorer_enabled in [false, true] {
        for arbitrary in [false, true] {
            for serde in [false, true] {
                if (!explorer_enabled || explorer)
                    && (!requested_arbitrary || arbitrary)
                    && (!requested_serde || serde)
                {
                    accepted.push(FacadeCapability {
                        diagrams,
                        explorer: explorer_enabled,
                        arbitrary,
                        serde,
                    });
                }
            }
        }
    }
    accepted
}

/// Derives accepted facade handshakes from the frozen emitted trait recipes.
pub(super) fn emitted_capability_plan(
    nodes: &[NodeParsePlan],
    tokens: &[TokenParsePlan],
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let recipes = nodes
        .iter()
        .flat_map(|node| &node.configured_traits)
        .chain(tokens.iter().flat_map(|token| &token.configured_traits));
    capability_plan_from_recipes(recipes.copied(), diagrams, explorer)
}

/// Reconstructs accepted facade handshakes directly from authored configured traits.
pub(super) fn authoritative_capability_plan(
    lowered: &crate::materialization::LoweredGrammar,
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let source = lowered.source_modeled.source_model();
    let recipes = source
        .nodes
        .iter()
        .flat_map(|node| &node.configured_traits.effective)
        .chain(
            source
                .token_definitions
                .iter()
                .flat_map(|definition| match &definition.kind {
                    TokenDefinitionKind::Content(token) => {
                        token.configured_traits.effective.as_slice()
                    }
                    TokenDefinitionKind::Fixed(_) | TokenDefinitionKind::LexicalBase(_) => &[],
                }),
        )
        .map(|selection| selection.recipe);
    capability_plan_from_recipes(recipes, diagrams, explorer)
}

/// Reduces configured-trait recipes to required optional facade capabilities.
pub(super) fn capability_plan_from_recipes(
    recipes: impl Iterator<Item = ConfiguredTraitRecipe>,
    diagrams: bool,
    explorer: bool,
) -> Vec<FacadeCapability> {
    let mut requested_arbitrary = false;
    let mut requested_serde = false;
    for recipe in recipes {
        requested_arbitrary |= recipe == ConfiguredTraitRecipe::Arbitrary;
        requested_serde |= matches!(
            recipe,
            ConfiguredTraitRecipe::Serialize | ConfiguredTraitRecipe::Deserialize
        );
    }
    accepted_capabilities(requested_arbitrary, requested_serde, diagrams, explorer)
}

#[cfg(test)]
mod tests {
    use super::{
        EMITTER_IDENTITY, GENERATOR_SOURCE_FINGERPRINT, PRIVATE_PROTOCOL,
        debug_without_span_offsets, generation_protocol_identity, generation_protocol_identity_for,
    };

    #[test]
    fn generated_public_shape_revision_changes_the_generation_cache_identity() {
        assert_eq!(PRIVATE_PROTOCOL, 13);
        assert_eq!(EMITTER_IDENTITY, "recursa-generation-compound-v34");
        assert_eq!(
            generation_protocol_identity(),
            generation_protocol_identity_for(13, GENERATOR_SOURCE_FINGERPRINT)
        );
        assert_ne!(
            generation_protocol_identity(),
            generation_protocol_identity_for(12, GENERATOR_SOURCE_FINGERPRINT)
        );
    }

    #[test]
    fn generator_source_fingerprint_changes_the_generation_cache_identity() {
        assert_eq!(GENERATOR_SOURCE_FINGERPRINT.len(), 16);
        assert!(
            GENERATOR_SOURCE_FINGERPRINT
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_ne!(
            generation_protocol_identity_for(8, "1111111111111111"),
            generation_protocol_identity_for(8, "2222222222222222")
        );
    }

    #[test]
    fn canonical_debug_ignores_spans_without_erasing_span_shaped_authored_text() {
        let first = r#"Ident { sym: Word, span: bytes(10..14), authored: "bytes(1..2)" }"#;
        let second = r#"Ident { sym: Word, span: bytes(90..94), authored: "bytes(1..2)" }"#;
        let changed_text = r#"Ident { sym: Word, span: bytes(10..14), authored: "bytes(3..4)" }"#;

        assert_eq!(
            debug_without_span_offsets(first),
            debug_without_span_offsets(second)
        );
        assert_ne!(
            debug_without_span_offsets(first),
            debug_without_span_offsets(changed_text)
        );
    }

    #[test]
    fn canonical_debug_preserves_escaped_and_malformed_span_shaped_text() {
        let escaped_first =
            r#"Node { span: bytes(1..2), text: "quote: \"bytes(3..4)\"; slash: \\bytes(5..6)" }"#;
        let escaped_second =
            r#"Node { span: bytes(8..9), text: "quote: \"bytes(3..4)\"; slash: \\bytes(5..6)" }"#;
        assert_eq!(
            debug_without_span_offsets(escaped_first),
            debug_without_span_offsets(escaped_second),
        );
        assert!(debug_without_span_offsets(escaped_first).contains(r#"\"bytes(3..4)\""#));
        assert!(debug_without_span_offsets(escaped_first).contains(r#"\\bytes(5..6)"#));

        for malformed in [
            "bytes(..2)",
            "bytes(1..)",
            "bytes(1.2)",
            "bytes(x..2)",
            "bytes(1..y)",
            "bytes(1..2",
        ] {
            assert_eq!(debug_without_span_offsets(malformed), malformed);
        }
    }
}
