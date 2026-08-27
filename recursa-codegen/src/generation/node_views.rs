//! Generated Node-view metadata, span elements, and typed accessors.
//!
//! View validation and rendering are co-located because accessor names, provenance
//! positions, repetition shapes, and authored visibility must evolve together.

use super::*;

/// Rejects semantic and fixed-span accessor names that collide within one Node shape.
pub(super) fn validate_accessor_names(
    node: &ResolvedNode,
    body: &NodeBodyPlan,
) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut check = |element: &ElementParsePlan| -> Result<(), String> {
        let name = match &element.kind {
            ElementParseKind::Fixed { accessor, .. } => accessor.unraw().to_string(),
            ElementParseKind::Semantic { field, .. } => {
                let Some(accessor) = &field.accessor else {
                    return Ok(());
                };
                accessor.unraw().to_string()
            }
        };
        if !names.insert(name.clone()) {
            return Err(format!(
                "generated accessor `{name}` collides within Node `{}`",
                node.ident
            ));
        }
        Ok(())
    };
    match body {
        NodeBodyPlan::Struct { elements, .. } => {
            for element in elements {
                check(element)?;
            }
        }
        NodeBodyPlan::Enum { variants, .. } => {
            for variant in variants {
                for element in &variant.elements {
                    check(element)?;
                }
            }
        }
        NodeBodyPlan::Pratt(pratt) => {
            for branch in &pratt.nud {
                for element in &branch.variant.elements {
                    check(element)?;
                }
            }
            for branch in &pratt.led {
                check(&branch.lhs)?;
                for element in &branch.variant.elements {
                    check(element)?;
                }
            }
        }
    }
    Ok(())
}

/// Renders source-order element metadata for every active shape of a Node.
pub(super) fn render_metadata(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let name = metadata_static(node_index, None);
            let entries = elements
                .iter()
                .map(|element| render_metadata_entry(facade, element));
            quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
        }
        NodeBodyPlan::Enum { variants, .. } => {
            let definitions = variants.iter().enumerate().map(|(index, variant)| {
                let name = metadata_static(node_index, Some(index));
                let entries = variant
                    .elements
                    .iter()
                    .map(|element| render_metadata_entry(facade, element));
                quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
            });
            quote!(#(#definitions)*)
        }
        NodeBodyPlan::Pratt(pratt) => {
            let nud = pratt.nud.iter().enumerate().map(|(index, branch)| {
                let name = metadata_static(node_index, Some(index));
                let entries = branch
                    .variant
                    .elements
                    .iter()
                    .map(|element| render_metadata_entry(facade, element));
                quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
            });
            let led_offset = pratt.nud.len();
            let led = pratt.led.iter().enumerate().map(|(index, branch)| {
                let name = metadata_static(node_index, Some(led_offset + index));
                let entries = std::iter::once(&branch.lhs)
                    .chain(branch.variant.elements.iter())
                    .map(|element| render_metadata_entry(facade, element));
                quote!(static #name: &[#facade::ElementMetadata] = &[#(#entries),*];)
            });
            quote!(#(#nud)* #(#led)*)
        }
    }
}

/// Renders one element's stable name and fixed-token or semantic-field role.
pub(super) fn render_metadata_entry(
    facade: &Ident,
    element: &ElementParsePlan,
) -> proc_macro2::TokenStream {
    let name = &element.metadata_name;
    let role = match element.kind {
        ElementParseKind::Fixed { .. } => quote!(#facade::ElementRole::FixedToken),
        ElementParseKind::Semantic { .. } => quote!(#facade::ElementRole::SemanticField),
    };
    quote!(#facade::__private::element_metadata(Some(#name), #role))
}

/// Selects the provenance-element slice for the Node's active struct or enum shape.
pub(super) fn render_span_elements(
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { .. } => {
            let metadata = metadata_static(node_index, None);
            quote!(self.cursor.elements(#metadata))
        }
        NodeBodyPlan::Enum { variants, .. } => {
            let node_ident = &node.ident;
            let arms = variants.iter().enumerate().map(|(index, variant)| {
                let variant_ident = &variant.ident;
                let pattern = variant_pattern(node_ident, variant_ident, variant.style);
                let metadata = metadata_static(node_index, Some(index));
                quote!(#pattern => self.cursor.elements(#metadata),)
            });
            quote!(match self.value { #(#arms)* })
        }
        NodeBodyPlan::Pratt(pratt) => {
            let node_ident = &node.ident;
            let nud = pratt.nud.iter().enumerate().map(|(index, branch)| {
                let variant = &branch.variant;
                let pattern = variant_pattern(node_ident, &variant.ident, variant.style);
                let metadata = metadata_static(node_index, Some(index));
                quote!(#pattern => self.cursor.elements(#metadata),)
            });
            let led_offset = pratt.nud.len();
            let led = pratt.led.iter().enumerate().map(|(index, branch)| {
                let variant = &branch.variant;
                let pattern = variant_pattern(node_ident, &variant.ident, variant.style);
                let metadata = metadata_static(node_index, Some(led_offset + index));
                quote!(#pattern => self.cursor.elements(#metadata),)
            });
            quote!(match self.value { #(#nud)* #(#led)* })
        }
    }
}

/// Renders visibility-matched occurrence-view accessors for one Node.
pub(super) fn render_accessors(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let (_, type_generics, _) = node.generics.split_for_impl();
    let self_type: Type = syn::parse_quote!(#node_ident #type_generics);
    let self_type = &self_type;
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let methods = elements.iter().enumerate().filter_map(|(index, element)| {
                render_accessor(facade, self_type, element, index, None)
            });
            quote!(#(#methods)*)
        }
        NodeBodyPlan::Enum { variants, .. } => {
            let methods = variants.iter().flat_map(|variant| {
                variant
                    .elements
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, element)| {
                        render_accessor(
                            facade,
                            self_type,
                            element,
                            index,
                            Some((node_ident, &variant.ident, variant.style)),
                        )
                    })
            });
            quote!(#(#methods)*)
        }
        NodeBodyPlan::Pratt(pratt) => {
            let nud = pratt.nud.iter().flat_map(|branch| {
                let variant = &branch.variant;
                variant
                    .elements
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, element)| {
                        render_accessor(
                            facade,
                            self_type,
                            element,
                            index,
                            Some((node_ident, &variant.ident, variant.style)),
                        )
                    })
            });
            let led = pratt.led.iter().flat_map(|branch| {
                let variant = &branch.variant;
                std::iter::once(&branch.lhs)
                    .chain(variant.elements.iter())
                    .enumerate()
                    .filter_map(move |(index, element)| {
                        render_accessor(
                            facade,
                            self_type,
                            element,
                            index,
                            Some((node_ident, &variant.ident, variant.style)),
                        )
                    })
            });
            quote!(#(#nud)* #(#led)*)
        }
    }
}

/// Renders semantic-value and fixed-token span accessors for one planned element.
///
/// # Long-function contract
///
/// Rationale: renders authored and structural accessors from the same occurrence layout.
/// Invariants: accessor names are prevalidated and every span cursor matches its source-order grammar element.
/// Phases: binds the active shape, emits semantic accessors, then emits fixed-token spans.
pub(super) fn render_accessor(
    facade: &Ident,
    self_type: &Type,
    element: &ElementParsePlan,
    element_index: usize,
    variant: Option<(&Ident, &Ident, ResolvedFieldStyle)>,
) -> Option<proc_macro2::TokenStream> {
    match &element.kind {
        ElementParseKind::Fixed { accessor, .. } => {
            let method = accessor;
            let active = variant.map(|(node, variant, style)| {
                let pattern = variant_pattern(node, variant, style);
                quote! {
                    if !matches!(self.value, #pattern) { return None; }
                }
            });
            Some(quote! {
                pub fn #method(&self) -> Option<#facade::Span> {
                    #active
                    self.cursor.child(#element_index).span()
                }
            })
        }
        ElementParseKind::Semantic {
            field,
            terminal,
            wrappers,
            attachment,
            ..
        } => {
            let terminal =
                if terminal_path_ident(terminal).is_some_and(|ident| ident.unraw() == "Self") {
                    self_type
                } else {
                    terminal
                };
            let accessor = field.accessor.as_ref()?;
            if let Some(repeat_index) = wrappers.iter().position(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            }) {
                return Some(render_repeated_accessor(
                    facade,
                    accessor,
                    field,
                    terminal,
                    wrappers,
                    repeat_index,
                    element_index,
                    variant,
                    attachment.as_deref(),
                ));
            }
            let is_optional = wrappers
                .iter()
                .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));
            let semantic_child = attachment
                .as_ref()
                .map(|attachment| attachment.before.len());
            let semantic_cursor = |cursor: proc_macro2::TokenStream| {
                semantic_child.map_or(cursor.clone(), |child| quote!((#cursor).child(#child)))
            };
            if let Some((node, variant_ident, style)) = variant {
                let binding = format_ident!("__recursa_field");
                let active_pattern = field_binding_pattern(
                    node,
                    variant_ident,
                    style,
                    field.ident.as_ref(),
                    field.index,
                    &binding,
                );
                let value = if is_optional {
                    let optional_reference =
                        render_optional_view_reference(quote!(#binding), wrappers, true);
                    let cursor =
                        semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                    quote! {
                        #optional_reference.map(|value| {
                            #facade::__private::NodeViewFactory::view(
                                value,
                                #cursor,
                            )
                        })
                    }
                } else {
                    let reference = render_terminal_reference(quote!(#binding), wrappers, true);
                    let cursor =
                        semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                    quote! {
                        Some(#facade::__private::NodeViewFactory::view(
                            #reference,
                            #cursor,
                        ))
                    }
                };
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> Option<impl #facade::NodeView<Value = #terminal> + 'view>
                    where
                        #terminal: #facade::__private::NodeViewFactory,
                    {
                        match self.value {
                            #active_pattern => #value,
                            _ => None,
                        }
                    }
                })
            } else if is_optional {
                let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
                let optional_reference = render_optional_view_reference(reference, wrappers, false);
                let cursor = semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> Option<impl #facade::NodeView<Value = #terminal> + 'view>
                    where
                        #terminal: #facade::__private::NodeViewFactory,
                    {
                        #optional_reference.map(|value| {
                            #facade::__private::NodeViewFactory::view(
                                value,
                                #cursor,
                            )
                        })
                    }
                })
            } else {
                let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
                let reference = render_terminal_reference(reference, wrappers, false);
                let cursor = semantic_cursor(quote!(self.cursor.child(#element_index).child(0)));
                Some(quote! {
                    pub fn #accessor(
                        &self,
                    ) -> impl #facade::NodeView<Value = #terminal> + 'view
                    where
                        #terminal: #facade::__private::NodeViewFactory,
                    {
                        #facade::__private::NodeViewFactory::view(
                            #reference,
                            #cursor,
                        )
                    }
                })
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Renders a repeated semantic accessor while preserving container and occurrence order.
///
/// # Long-function contract
///
/// Rationale: handles repeated occurrences centrally because wrappers and provenance advance together.
/// Invariants: iteration order and optional/non-empty container semantics match the parse plan.
/// Phases: unwraps containers, maps semantic values to occurrence cursors, then restores the declared wrapper.
pub(super) fn render_repeated_accessor(
    facade: &Ident,
    accessor: &Ident,
    field: &FieldPlan,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    repeat_index: usize,
    element_index: usize,
    variant: Option<(&Ident, &Ident, ResolvedFieldStyle)>,
    attachment: Option<&FixedAttachmentParsePlan>,
) -> proc_macro2::TokenStream {
    let outer = &wrappers[..repeat_index];
    let inner = &wrappers[repeat_index + 1..];
    let item_type = render_wrapped_type(facade, terminal, inner);
    let project = render_terminal_reference(quote!(value), inner, true);
    let cursor = quote!(self.cursor.child(#element_index));
    let repeated = |collection: proc_macro2::TokenStream| {
        attachment.map_or_else(
            || {
                quote! {
                    #facade::Repeated::new(
                        (#collection).as_slice(),
                        #cursor,
                        |value: &#item_type| -> &#terminal { #project },
                    )
                }
            },
            |attachment| {
                let semantic_child = attachment.before.len();
                quote! {
                    #facade::Repeated::new_attached(
                        (#collection).as_slice(),
                        #cursor,
                        |value: &#item_type| -> &#terminal { #project },
                        #semantic_child,
                    )
                }
            },
        )
    };
    let outer_optional = outer
        .iter()
        .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));

    if let Some((node, variant_ident, style)) = variant {
        let binding = format_ident!("__recursa_field");
        let active_pattern = field_binding_pattern(
            node,
            variant_ident,
            style,
            field.ident.as_ref(),
            field.index,
            &binding,
        );
        let active_value = if outer_optional {
            let collection = render_optional_view_reference(quote!(#binding), outer, true);
            let repeated = repeated(quote!(collection));
            quote!(#collection.map(|collection| #repeated))
        } else {
            let collection = render_terminal_reference(quote!(#binding), outer, true);
            let repeated = repeated(collection);
            quote!(Some(#repeated))
        };
        quote! {
            pub fn #accessor(
                &self,
            ) -> Option<#facade::Repeated<'view, #terminal, #item_type>>
            where
                #terminal: #facade::__private::NodeViewFactory,
            {
                match self.value {
                    #active_pattern => #active_value,
                    _ => None,
                }
            }
        }
    } else {
        let reference = field_access(quote!(self.value), field.ident.as_ref(), field.index);
        if outer_optional {
            let collection = render_optional_view_reference(reference, outer, false);
            let repeated = repeated(quote!(collection));
            quote! {
                pub fn #accessor(
                    &self,
                ) -> Option<#facade::Repeated<'view, #terminal, #item_type>>
                where
                    #terminal: #facade::__private::NodeViewFactory,
                {
                    #collection.map(|collection| #repeated)
                }
            }
        } else {
            let collection = render_terminal_reference(reference, outer, false);
            let repeated = repeated(collection);
            quote! {
                pub fn #accessor(
                    &self,
                ) -> #facade::Repeated<'view, #terminal, #item_type>
                where
                    #terminal: #facade::__private::NodeViewFactory,
                {
                    #repeated
                }
            }
        }
    }
}

/// Derives a valid generated span-accessor identifier from an element metadata name.
pub(super) fn fixed_accessor_ident(metadata_name: &str) -> Result<Ident, String> {
    let direct = format!("{metadata_name}_span");
    emitted_ident(&direct).or_else(|_| emitted_ident(&format!("element_{direct}")))
}
