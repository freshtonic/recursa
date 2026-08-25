use recursa_codegen::{
    GrammarDescription, PrattPosition, PrattVariant, SiteContainer, TypeDescription, TypeId,
    TypeShape,
};

use super::FixtureBuilder;

pub const PLUS: u16 = 0;
pub const MINUS: u16 = 1;
pub const STAR: u16 = 2;
pub const SLASH: u16 = 3;
pub const LPAREN: u16 = 4;
pub const RPAREN: u16 = 5;
pub const INT_LIT: u16 = 6;
pub const IDENT: u16 = 7;

pub const PLUS_TOK: TypeId = TypeId::new(0);
pub const MINUS_TOK: TypeId = TypeId::new(1);
pub const STAR_TOK: TypeId = TypeId::new(2);
pub const SLASH_TOK: TypeId = TypeId::new(3);
pub const LPAREN_TOK: TypeId = TypeId::new(4);
pub const RPAREN_TOK: TypeId = TypeId::new(5);
pub const INT_LIT_TOK: TypeId = TypeId::new(6);
pub const IDENT_TOK: TypeId = TypeId::new(7);
pub const EXPR: TypeId = TypeId::new(8);

pub fn grammar(reverse_variants: bool) -> GrammarDescription {
    let mut builder = FixtureBuilder::new(
        "arith",
        &[
            "Plus", "Minus", "Star", "Slash", "LParen", "RParen", "IntLit", "Ident",
        ],
    );
    let tokens = [
        ("PlusTok", PLUS as usize),
        ("MinusTok", MINUS as usize),
        ("StarTok", STAR as usize),
        ("SlashTok", SLASH as usize),
        ("LParenTok", LPAREN as usize),
        ("RParenTok", RPAREN as usize),
        ("IntLitTok", INT_LIT as usize),
        ("IdentTok", IDENT as usize),
    ];
    let token_ids = tokens.map(|(name, kind)| builder.token(name, kind));
    assert_eq!(token_ids[7], IDENT_TOK);

    let expr_declaration = builder.declaration("Expr");
    let variants = [
        ("IntLit", INT_LIT_TOK, PrattPosition::Atom),
        ("Ident", IDENT_TOK, PrattPosition::Atom),
        ("Paren", LPAREN_TOK, PrattPosition::Atom),
        ("Neg", MINUS_TOK, PrattPosition::Prefix { bp: 30 }),
        ("Add", PLUS_TOK, PrattPosition::Infix { lbp: 10, rbp: 11 }),
        ("Sub", MINUS_TOK, PrattPosition::Infix { lbp: 10, rbp: 11 }),
        ("Mul", STAR_TOK, PrattPosition::Infix { lbp: 20, rbp: 21 }),
        ("Div", SLASH_TOK, PrattPosition::Infix { lbp: 20, rbp: 21 }),
    ];
    let mut pratt_variants = Vec::new();
    for (name, target, position) in variants {
        let declaration = builder.declaration(name);
        let site = builder.site(EXPR, target, declaration, SiteContainer::Direct);
        let mut sites = vec![site];
        if matches!(
            position,
            PrattPosition::Prefix { .. } | PrattPosition::Infix { .. }
        ) {
            sites.push(builder.site(EXPR, EXPR, declaration, SiteContainer::Boxed));
        }
        if name == "Paren" {
            sites.push(builder.site(EXPR, EXPR, declaration, SiteContainer::Boxed));
            sites.push(builder.site(EXPR, RPAREN_TOK, declaration, SiteContainer::Direct));
        }
        pratt_variants.push(PrattVariant {
            declaration,
            position,
            sites,
        });
    }
    if reverse_variants {
        pratt_variants.reverse();
    }
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Expr",
            expr_declaration,
            TypeShape::Pratt {
                variants: pratt_variants,
            },
        )),
        EXPR,
    );
    builder.grammar.entry_types.push(EXPR);
    builder.grammar
}
