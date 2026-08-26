pub mod arith;
pub mod json;

use recursa_codegen::{
    AtomDescription, AtomExpression, ContainerShape, DeclId, Declaration, GrammarDescription,
    KindDescription, Location, ParseSite, SourceId, TypeId,
};

struct FixtureBuilder {
    grammar: GrammarDescription,
    location: recursa_codegen::LocationId,
    atoms: Vec<recursa_codegen::AtomId>,
}

impl FixtureBuilder {
    fn new(name: &str, kind_names: &[&str]) -> Self {
        let mut grammar = GrammarDescription::new(
            name,
            kind_names.len(),
            1,
            recursa_codegen::TextEquivalence::AsciiInsensitive,
        );
        let location = grammar.push_location(Location::new(SourceId::new(0), 0..1));
        let mut atoms = Vec::with_capacity(kind_names.len());
        for &name in kind_names {
            let declaration = grammar.push_declaration(Declaration::new(name, location));
            let kind = grammar.push_kind(KindDescription::new(name, declaration));
            atoms.push(grammar.push_atom(AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            )));
        }
        Self {
            grammar,
            location,
            atoms,
        }
    }

    fn declaration(&mut self, name: &str) -> DeclId {
        self.grammar
            .push_declaration(Declaration::new(name, self.location))
    }

    fn token(&mut self, name: &str, kind_index: usize) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar
            .push_type(recursa_codegen::TypeDescription::new(
                name,
                declaration,
                recursa_codegen::TypeShape::Token {
                    atom: self.atoms[kind_index],
                    text_predicate: None,
                },
            ))
    }

    fn site(
        &mut self,
        owner: TypeId,
        target: TypeId,
        declaration: DeclId,
        container: ContainerShape,
    ) -> recursa_codegen::SiteId {
        self.grammar
            .push_site(ParseSite::new(owner, target, declaration, container))
    }
}
