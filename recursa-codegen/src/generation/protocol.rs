//! Private generation protocol, capability handshake, and plan-family identities.
//!
//! These values participate in generation identity and loader compatibility. Keeping
//! their construction together makes protocol drift explicit and reviewable.

use super::*;

/// Private loader protocol understood by generated roots and the facade.
pub(super) const PRIVATE_PROTOCOL: u32 = 3;
/// Stable emitter-format identity hashed into every generated family identity.
pub(super) const EMITTER_IDENTITY: &str = "recursa-generation-compound-v3";

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
/// Hashes the parse plans in their canonical Debug field and source order.
pub(super) fn parse_plan_identity(nodes: &[NodeParsePlan]) -> u64 {
    stable_hash(format!("{nodes:?}").as_bytes())
}

/// Hashes one capability plan without erasing Debug field or variant order.
pub(super) fn plan_family_identity(value: &impl std::fmt::Debug) -> u64 {
    stable_hash(format!("{value:?}").as_bytes())
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

/// Enumerates accepted facade combinations in deterministic boolean-product order.
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
            for selection in &token.configured_traits.effective {
                canonical.push_str(&format!("{:?},", selection.recipe));
            }
        } else {
            canonical.push_str("<missing>");
        }
        canonical.push(';');
    }
    stable_hash(canonical.as_bytes())
}

/// Hashes configured-trait recipes in canonical Node-then-token order.
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
        for recipe in &token.configured_traits {
            canonical.push_str(&format!("{recipe:?},"));
        }
        canonical.push(';');
    }
    stable_hash(canonical.as_bytes())
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
