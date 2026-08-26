use recursa_codegen::{
    Alternative, ContainerShape, GrammarDescription, TypeDescription, TypeId, TypeShape,
};

use super::FixtureBuilder;

pub const VALUE: TypeId = TypeId::new(11);
pub const STRING_LITERAL: TypeId = TypeId::new(12);
pub const NUMBER_LITERAL: TypeId = TypeId::new(13);
pub const BOOL_LITERAL: TypeId = TypeId::new(14);
pub const OBJECT: TypeId = TypeId::new(15);
pub const MEMBER: TypeId = TypeId::new(16);
pub const ARRAY: TypeId = TypeId::new(17);

pub fn grammar() -> GrammarDescription {
    let mut builder = FixtureBuilder::new(
        "json",
        &[
            "LBrace", "RBrace", "LBracket", "RBracket", "Comma", "Colon", "String", "Number",
            "True", "False", "Null",
        ],
    );
    for (index, name) in [
        "LBraceTok",
        "RBraceTok",
        "LBracketTok",
        "RBracketTok",
        "CommaTok",
        "ColonTok",
        "StringTok",
        "NumberTok",
        "TrueTok",
        "FalseTok",
        "NullTok",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(builder.token(name, index), TypeId::new(index as u32));
    }

    let value_declaration = builder.declaration("Value");
    let mut value_alternatives = Vec::new();
    for (name, target) in [
        ("Object", OBJECT),
        ("Array", ARRAY),
        ("String", STRING_LITERAL),
        ("Number", NUMBER_LITERAL),
        ("Bool", BOOL_LITERAL),
        ("Null", TypeId::new(10)),
    ] {
        let declaration = builder.declaration(name);
        let site = builder.site(VALUE, target, declaration, ContainerShape::Direct);
        value_alternatives.push(Alternative {
            declaration,
            sites: vec![site],
        });
    }
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Value",
            value_declaration,
            TypeShape::Enum {
                alternatives: value_alternatives,
            },
        )),
        VALUE,
    );

    for (name, owner, target) in [
        ("StringLiteral", STRING_LITERAL, TypeId::new(6)),
        ("NumberLiteral", NUMBER_LITERAL, TypeId::new(7)),
    ] {
        let declaration = builder.declaration(name);
        let site = builder.site(owner, target, declaration, ContainerShape::Direct);
        assert_eq!(
            builder.grammar.push_type(TypeDescription::new(
                name,
                declaration,
                TypeShape::Alias { site },
            )),
            owner,
        );
    }

    let bool_declaration = builder.declaration("BoolLiteral");
    let mut bool_alternatives = Vec::new();
    for (name, target) in [("True", TypeId::new(8)), ("False", TypeId::new(9))] {
        let declaration = builder.declaration(name);
        let site = builder.site(BOOL_LITERAL, target, declaration, ContainerShape::Direct);
        bool_alternatives.push(Alternative {
            declaration,
            sites: vec![site],
        });
    }
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "BoolLiteral",
            bool_declaration,
            TypeShape::Enum {
                alternatives: bool_alternatives,
            },
        )),
        BOOL_LITERAL,
    );

    push_sequence(&mut builder, "Object", OBJECT, &[TypeId::new(0)]);
    push_sequence(
        &mut builder,
        "Member",
        MEMBER,
        &[TypeId::new(6), TypeId::new(5), VALUE],
    );
    push_sequence(&mut builder, "Array", ARRAY, &[TypeId::new(2)]);
    builder.grammar.entry_types.push(VALUE);
    builder.grammar
}

fn push_sequence(builder: &mut FixtureBuilder, name: &str, owner: TypeId, targets: &[TypeId]) {
    let declaration = builder.declaration(name);
    let sites = targets
        .iter()
        .map(|target| builder.site(owner, *target, declaration, ContainerShape::Direct))
        .collect();
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Sequence { sites },
        )),
        owner,
    );
}
