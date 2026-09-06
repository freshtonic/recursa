//! `RCA0301` at Pratt sites that exclude extenders or inherit a led tail's continuation.
//!
//! Issue #126: an excluded led variant is never selected at its site, so its starters are
//! not a caller-FOLLOW overlap there; and the extenders that follow a led variant's tail
//! are reported where the generated parser consults them, so an exact `#[greedy(...)]`
//! is expressible for every kind that could raise `RCA4102` at run time (#109).

use recursa_codegen::{
    AtomDescription, AtomExpression, ConflictRemedy, ContainerShape, DeclId, Declaration,
    DiagnosticCode, GrammarDescription, GreedyAcceptance, KindDescription, KindId, Location,
    ParseSite, PrattPosition, PrattVariant, Severity, SiteId, SourceId, TextEquivalence,
    TypeDescription, TypeId, TypeShape, analyze,
};

struct Builder {
    grammar: GrammarDescription,
    next_byte: u32,
    atoms: Vec<recursa_codegen::AtomId>,
}

impl Builder {
    fn new(name: &str, kinds: &[&str]) -> Self {
        let mut builder = Self {
            grammar: GrammarDescription::new(name, kinds.len(), 2, TextEquivalence::Sensitive),
            next_byte: 0,
            atoms: Vec::new(),
        };
        for &name in kinds {
            let declaration = builder.declaration(name);
            let kind = builder
                .grammar
                .push_kind(KindDescription::new(name, declaration).with_authored(name));
            builder
                .atoms
                .push(builder.grammar.push_atom(AtomDescription::new(
                    name,
                    declaration,
                    AtomExpression::Kind(kind),
                )));
        }
        builder
    }

    fn declaration(&mut self, name: &str) -> DeclId {
        let start = self.next_byte;
        self.next_byte += 2;
        let location = self
            .grammar
            .push_location(Location::new(SourceId::new(0), start..start + 1));
        self.grammar
            .push_declaration(Declaration::new(name, location))
    }

    fn token(&mut self, name: &str, kind: usize) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom: self.atoms[kind],
                text_predicate: None,
            },
        ))
    }

    fn site(&mut self, owner: TypeId, target: TypeId, name: &str) -> SiteId {
        let declaration = self.declaration(name);
        self.grammar.push_site(ParseSite::new(
            owner,
            target,
            declaration,
            ContainerShape::Direct,
        ))
    }

    fn operand(&mut self, owner: TypeId, name: &str, min_bp: u8) -> SiteId {
        let declaration = self.declaration(name);
        self.grammar.push_site(
            ParseSite::new(
                owner,
                owner,
                declaration,
                ContainerShape::boxed(ContainerShape::Direct),
            )
            .with_min_bp(min_bp),
        )
    }

    fn accept(&mut self, site: SiteId, acceptance: GreedyAcceptance) {
        let declaration = self.grammar.sites[site.index()].declaration;
        let current = self.grammar.declarations[declaration.index()].clone();
        self.grammar.declarations[declaration.index()] = current.with_greedy(acceptance);
    }

    fn exclude(&mut self, site: SiteId, variants: &[DeclId]) {
        self.grammar.sites[site.index()].pratt_excluded = variants.to_vec();
    }
}

fn kinds(set: &recursa_codegen::KindSet) -> Vec<KindId> {
    set.iter().collect()
}

/// `Expr = Atom | Expr NOT | Expr PLUS Expr`, used as `Stmt = Expr NOT`, so `NOT` both
/// extends the expression and follows the `Stmt::expr` site.
fn excluded_extender_grammar() -> (Builder, SiteId, DeclId) {
    let mut builder = Builder::new("excluded-extender", &["Atom", "NOT", "PLUS"]);
    let atom = builder.token("Atom", 0);
    let not = builder.token("NOT", 1);
    let plus = builder.token("PLUS", 2);
    let expr = builder.grammar.next_type_id();
    let expr_decl = builder.declaration("Expr");
    let atom_decl = builder.declaration("Expr::Atom");
    let not_decl = builder.declaration("Expr::NotLike");
    let add_decl = builder.declaration("Expr::Add");
    let atom_site = builder.site(expr, atom, "Expr::Atom::0");
    let not_site = builder.site(expr, not, "Expr::NotLike::1");
    let plus_site = builder.site(expr, plus, "Expr::Add::1");
    let add_rhs = builder.operand(expr, "Expr::Add::2", 11);
    builder.grammar.push_type(TypeDescription::new(
        "Expr",
        expr_decl,
        TypeShape::Pratt {
            variants: vec![
                PrattVariant {
                    declaration: atom_decl,
                    position: PrattPosition::Atom,
                    sites: vec![atom_site],
                },
                PrattVariant {
                    declaration: not_decl,
                    position: PrattPosition::Postfix { bp: 5 },
                    sites: vec![not_site],
                },
                PrattVariant {
                    declaration: add_decl,
                    position: PrattPosition::Infix { lbp: 10, rbp: 11 },
                    sites: vec![plus_site, add_rhs],
                },
            ],
        },
    ));
    let stmt = builder.grammar.next_type_id();
    let stmt_decl = builder.declaration("Stmt");
    let expr_site = builder.site(stmt, expr, "Stmt::expr");
    let tail = builder.site(stmt, not, "Stmt::tail");
    builder.grammar.push_type(TypeDescription::new(
        "Stmt",
        stmt_decl,
        TypeShape::Sequence {
            sites: vec![expr_site, tail],
        },
    ));
    builder.grammar.entry_types.push(stmt);
    (builder, expr_site, not_decl)
}

fn pratt_overlaps(report: &recursa_codegen::AnalysisReport) -> Vec<(SiteId, Vec<KindId>)> {
    report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::PrattFollowOverlap)
        .map(|diagnostic| {
            let Some(ConflictRemedy::ReviewPrattFollow { site, .. }) = diagnostic.conflict_remedy
            else {
                panic!("RCA0301 carries its Pratt remedy");
            };
            (
                site,
                kinds(&diagnostic.conflict.as_ref().expect("evidence").overlap),
            )
        })
        .collect()
}

#[test]
fn an_excluded_extender_is_not_a_caller_follow_overlap() {
    let (builder, expr_site, not_decl) = excluded_extender_grammar();
    let baseline = analyze(&builder.grammar);
    assert_eq!(
        pratt_overlaps(&baseline),
        [(expr_site, vec![KindId::new(1)])],
        "without the exclusion `NOT` overlaps at `Stmt::expr`:\n{}",
        baseline.canonical_diagnostics(),
    );

    let mut excluded = builder;
    excluded.exclude(expr_site, &[not_decl]);
    let report = analyze(&excluded.grammar);
    assert!(report.output.is_some());
    assert!(
        pratt_overlaps(&report).is_empty(),
        "the excluded extender is never selected at the site:\n{}",
        report.canonical_diagnostics(),
    );
    assert!(
        report.diagnostics.is_empty(),
        "{}",
        report.canonical_diagnostics()
    );
}

#[test]
fn a_greedy_acceptance_for_an_excluded_extender_is_stale() {
    let (mut builder, expr_site, not_decl) = excluded_extender_grammar();
    builder.exclude(expr_site, &[not_decl]);
    builder.accept(expr_site, GreedyAcceptance::new(0, vec![KindId::new(1)]));
    let report = analyze(&builder.grammar);
    let stale = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::GreedyStale)
        .unwrap_or_else(|| {
            panic!(
                "the exclusion removed the overlap, so the acceptance is stale:\n{}",
                report.canonical_diagnostics()
            )
        });
    assert_eq!(stale.severity, Severity::Error);
    assert!(
        report.canonical_diagnostics().contains("error[RCA0302]"),
        "{}",
        report.canonical_diagnostics()
    );
}

/// `Expr = Atom | Expr LIKE Expr [ESCAPE Expr] | Expr PLUS Expr`: the escape operand
/// inherits the continuation of the `Like` tail, so every extender of `Expr` can follow it.
fn escape_continuation_grammar() -> (Builder, EscapeSites) {
    let mut builder = Builder::new("escape-continuation", &["Atom", "LIKE", "ESCAPE", "PLUS"]);
    let atom = builder.token("Atom", 0);
    let like = builder.token("LIKE", 1);
    let escape_token = builder.token("ESCAPE", 2);
    let plus = builder.token("PLUS", 3);

    let escape = builder.grammar.next_type_id();
    let expr = TypeId::new(escape.index() as u32 + 1);
    let escape_decl = builder.declaration("EscapeClause");
    let escape_keyword = builder.site(escape, escape_token, "EscapeClause::0");
    let escape_char = builder.site(escape, expr, "EscapeClause::char");
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "EscapeClause",
            escape_decl,
            TypeShape::Sequence {
                sites: vec![escape_keyword, escape_char],
            },
        )),
        escape,
    );

    let expr_decl = builder.declaration("Expr");
    let atom_decl = builder.declaration("Expr::Atom");
    let like_decl = builder.declaration("Expr::Like");
    let add_decl = builder.declaration("Expr::Add");
    let atom_site = builder.site(expr, atom, "Expr::Atom::0");
    let like_site = builder.site(expr, like, "Expr::Like::1");
    let like_rhs = builder.operand(expr, "Expr::Like::2", 6);
    let like_escape_decl = builder.declaration("Expr::Like::3");
    let like_escape = builder.grammar.push_site(ParseSite::new(
        expr,
        escape,
        like_escape_decl,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    let plus_site = builder.site(expr, plus, "Expr::Add::1");
    let add_rhs = builder.operand(expr, "Expr::Add::2", 11);
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Expr",
            expr_decl,
            TypeShape::Pratt {
                variants: vec![
                    PrattVariant {
                        declaration: atom_decl,
                        position: PrattPosition::Atom,
                        sites: vec![atom_site],
                    },
                    PrattVariant {
                        declaration: like_decl,
                        position: PrattPosition::Postfix { bp: 5 },
                        sites: vec![like_site, like_rhs, like_escape],
                    },
                    PrattVariant {
                        declaration: add_decl,
                        position: PrattPosition::Infix { lbp: 10, rbp: 11 },
                        sites: vec![plus_site, add_rhs],
                    },
                ],
            },
        )),
        expr,
    );
    builder.grammar.entry_types.push(expr);
    (
        builder,
        EscapeSites {
            escape_char,
            like_rhs,
            add_rhs,
            like_escape,
            like_decl,
        },
    )
}

struct EscapeSites {
    escape_char: SiteId,
    like_rhs: SiteId,
    add_rhs: SiteId,
    like_escape: SiteId,
    like_decl: DeclId,
}

#[test]
fn a_led_tail_continuation_is_reported_at_the_operand_that_inherits_it() {
    let (builder, sites) = escape_continuation_grammar();
    let report = analyze(&builder.grammar);
    assert!(
        report.output.is_some(),
        "{}",
        report.canonical_diagnostics()
    );
    assert_eq!(
        pratt_overlaps(&report),
        [(sites.escape_char, vec![KindId::new(1), KindId::new(3)])],
        "every extender of `Expr` follows the escape operand, and nothing is reported at \
         the recursive operands `{}` and `{}`:\n{}",
        sites.like_rhs.index(),
        sites.add_rhs.index(),
        report.canonical_diagnostics(),
    );
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(
        diagnostic
            .message
            .contains("`#[greedy(LIKE, PLUS)]` on `char`"),
        "the exact acceptance is suggested:\n{}",
        diagnostic.message
    );
    let like_location = builder.grammar.declarations[sites.like_decl.index()].location;
    assert!(
        diagnostic
            .secondary
            .iter()
            .any(|label| label.location == Some(like_location)),
        "the continuation is attributed to the led variant whose tail it follows:\n{}",
        report.canonical_diagnostics(),
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != DiagnosticCode::NullableFollowOverlap),
        "`ESCAPE` is not an extender, so the optional clause itself does not overlap:\n{}",
        report.canonical_diagnostics(),
    );
    let _ = sites.like_escape;
}

#[test]
fn an_exact_acceptance_names_every_inherited_extender() {
    let (mut builder, sites) = escape_continuation_grammar();
    builder.accept(
        sites.escape_char,
        GreedyAcceptance::new(0, vec![KindId::new(1), KindId::new(3)]),
    );
    let report = analyze(&builder.grammar);
    assert!(
        report.diagnostics.is_empty(),
        "{}",
        report.canonical_diagnostics()
    );
    let output = report
        .output
        .expect("an exact acceptance publishes decisions");
    assert_eq!(
        kinds(&output.decisions().greedy_pratt_boundary_by_site()[sites.escape_char.index()]),
        [KindId::new(1), KindId::new(3)],
        "the accepted kinds are marked greedy where the loop consults them",
    );

    let (mut builder, sites) = escape_continuation_grammar();
    builder.accept(sites.escape_char, GreedyAcceptance::all(0));
    let report = analyze(&builder.grammar);
    assert!(
        report.diagnostics.is_empty(),
        "{}",
        report.canonical_diagnostics()
    );
    assert!(report.output.is_some());
}

#[test]
fn an_acceptance_that_omits_an_inherited_extender_is_undeclared() {
    let (mut builder, sites) = escape_continuation_grammar();
    builder.accept(
        sites.escape_char,
        GreedyAcceptance::new(0, vec![KindId::new(3)]),
    );
    let report = analyze(&builder.grammar);
    let undeclared = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::GreedyUndeclared)
        .unwrap_or_else(|| panic!("{}", report.canonical_diagnostics()));
    assert_eq!(undeclared.severity, Severity::Error);
    assert!(
        undeclared.message.contains("LIKE"),
        "the omitted kind is named:\n{}",
        undeclared.message
    );
    assert!(
        report.canonical_diagnostics().contains("error[RCA0303]"),
        "{}",
        report.canonical_diagnostics()
    );
}

#[test]
fn an_exclusion_narrows_the_inherited_continuation() {
    let (mut builder, sites) = escape_continuation_grammar();
    builder.exclude(sites.escape_char, &[sites.like_decl]);
    let report = analyze(&builder.grammar);
    assert_eq!(
        pratt_overlaps(&report),
        [(sites.escape_char, vec![KindId::new(3)])],
        "`LIKE` cannot extend the escape operand once excluded, `PLUS` still can:\n{}",
        report.canonical_diagnostics(),
    );
}

/// `Expr = Atom | Expr FOO [PLUS] | Expr PLUS Expr`: the optional `PLUS` at the end of the
/// `Foo` tail may also begin the `Add` extender that follows the tail.
#[test]
fn a_nullable_tail_element_that_begins_with_an_extender_is_reported() {
    let mut builder = Builder::new("nullable-continuation", &["Atom", "FOO", "PLUS"]);
    let atom = builder.token("Atom", 0);
    let foo = builder.token("FOO", 1);
    let plus = builder.token("PLUS", 2);
    let expr = builder.grammar.next_type_id();
    let expr_decl = builder.declaration("Expr");
    let atom_decl = builder.declaration("Expr::Atom");
    let foo_decl = builder.declaration("Expr::Foo");
    let add_decl = builder.declaration("Expr::Add");
    let atom_site = builder.site(expr, atom, "Expr::Atom::0");
    let foo_site = builder.site(expr, foo, "Expr::Foo::1");
    let foo_plus_decl = builder.declaration("Expr::Foo::2");
    let foo_plus = builder.grammar.push_site(ParseSite::new(
        expr,
        plus,
        foo_plus_decl,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    let plus_site = builder.site(expr, plus, "Expr::Add::1");
    let add_rhs = builder.operand(expr, "Expr::Add::2", 11);
    builder.grammar.push_type(TypeDescription::new(
        "Expr",
        expr_decl,
        TypeShape::Pratt {
            variants: vec![
                PrattVariant {
                    declaration: atom_decl,
                    position: PrattPosition::Atom,
                    sites: vec![atom_site],
                },
                PrattVariant {
                    declaration: foo_decl,
                    position: PrattPosition::Postfix { bp: 20 },
                    sites: vec![foo_site, foo_plus],
                },
                PrattVariant {
                    declaration: add_decl,
                    position: PrattPosition::Infix { lbp: 10, rbp: 11 },
                    sites: vec![plus_site, add_rhs],
                },
            ],
        },
    ));
    builder.grammar.entry_types.push(expr);
    let report = analyze(&builder.grammar);
    let nullable = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::NullableFollowOverlap)
        .map(|diagnostic| {
            let Some(ConflictRemedy::ReviewNullableFollow { site }) = diagnostic.conflict_remedy
            else {
                panic!("RCA0300 carries its nullable remedy");
            };
            (
                site,
                kinds(&diagnostic.conflict.as_ref().expect("evidence").overlap),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        nullable,
        [(foo_plus, vec![KindId::new(2)])],
        "{}",
        report.canonical_diagnostics()
    );
    assert!(report.output.is_some());
}
