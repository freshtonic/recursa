//! `#[greedy(...)]` acceptances over caller-FOLLOW overlap findings.

use recursa_codegen::{
    AtomDescription, AtomExpression, ContainerShape, DeclId, Declaration, DiagnosticCode,
    GrammarDescription, GreedyAcceptance, KindDescription, KindId, Location, ParseSite,
    PrattPosition, PrattVariant, Severity, SiteId, SourceId, TextPredicate, TypeDescription,
    TypeId, TypeShape, analyze,
};

struct Builder {
    grammar: GrammarDescription,
    next_byte: u32,
    atoms: Vec<recursa_codegen::AtomId>,
}

impl Builder {
    fn new(name: &str, kinds: &[&str]) -> Self {
        let mut builder = Self {
            grammar: GrammarDescription::new(
                name,
                kinds.len(),
                1,
                recursa_codegen::TextEquivalence::AsciiInsensitive,
            ),
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
        self.token_with_predicate(name, kind, None)
    }

    fn token_with_predicate(
        &mut self,
        name: &str,
        kind: usize,
        predicate: Option<recursa_codegen::TextPredicateId>,
    ) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom: self.atoms[kind],
                text_predicate: predicate,
            },
        ))
    }

    fn site(
        &mut self,
        owner: TypeId,
        target: TypeId,
        name: &str,
        container: ContainerShape,
    ) -> SiteId {
        let declaration = self.declaration(name);
        self.grammar
            .push_site(ParseSite::new(owner, target, declaration, container))
    }

    /// Attaches one acceptance to the declaration of an existing site.
    fn accept(&mut self, site: SiteId, id: u32, kinds: &[usize]) {
        let declaration = self.grammar.sites[site.index()].declaration;
        let kinds = kinds
            .iter()
            .map(|kind| KindId::new(*kind as u16))
            .collect::<Vec<_>>();
        let current = self.grammar.declarations[declaration.index()].clone();
        self.grammar.declarations[declaration.index()] =
            current.with_greedy(GreedyAcceptance::new(id, kinds));
    }
}

/// `Root = (Inner ("SELECT" Word)?)*` where `Inner = ("SELECT" Word)?`: the optional
/// inner element and the repetition separator share the `Word` kind.
fn nullable_follow_overlap() -> (Builder, SiteId) {
    let mut builder = Builder::new("nested-follow", &["Word"]);
    let select = builder
        .grammar
        .push_text_predicate(TextPredicate::Exact("SELECT".into()));
    let maybe = builder.token_with_predicate("Maybe", 0, Some(select));
    let after = builder.token_with_predicate("After", 0, Some(select));

    let inner = builder.grammar.next_type_id();
    let inner_decl = builder.declaration("Inner");
    let maybe_site = builder.site(
        inner,
        maybe,
        "Inner::maybe",
        ContainerShape::optional(ContainerShape::Direct),
    );
    builder.grammar.push_type(TypeDescription::new(
        "Inner",
        inner_decl,
        TypeShape::Sequence {
            sites: vec![maybe_site],
        },
    ));

    let root = builder.grammar.next_type_id();
    let root_decl = builder.declaration("Root");
    let inner_site = builder.site(
        root,
        inner,
        "Root::inner",
        ContainerShape::zero_or_more(ContainerShape::Direct, Some(after)),
    );
    builder.grammar.push_type(TypeDescription::new(
        "Root",
        root_decl,
        TypeShape::Sequence {
            sites: vec![inner_site],
        },
    ));
    builder.grammar.entry_types.push(root);
    (builder, maybe_site)
}

/// A Pratt expression with one postfix extender that also follows the expression use site.
fn pratt_follow_overlap() -> (Builder, SiteId) {
    let mut builder = Builder::new("pratt-follow", &["Atom", "Post"]);
    let atom = builder.token("Atom", 0);
    let post = builder.token("Post", 1);
    let expr = builder.grammar.next_type_id();
    let expr_decl = builder.declaration("Expr");
    let atom_decl = builder.declaration("Expr::Atom");
    let post_decl = builder.declaration("Expr::Post");
    let atom_site = builder.site(expr, atom, "Expr::Atom::0", ContainerShape::Direct);
    let post_site = builder.site(expr, post, "Expr::Post::0", ContainerShape::Direct);
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
                    declaration: post_decl,
                    position: PrattPosition::Postfix { bp: 10 },
                    sites: vec![post_site],
                },
            ],
        },
    ));
    let stmt = builder.grammar.next_type_id();
    let stmt_decl = builder.declaration("Stmt");
    let expr_site = builder.site(stmt, expr, "Stmt::expr", ContainerShape::Direct);
    let tail = builder.site(stmt, post, "Stmt::tail", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "Stmt",
        stmt_decl,
        TypeShape::Sequence {
            sites: vec![expr_site, tail],
        },
    ));
    builder.grammar.entry_types.push(stmt);
    (builder, expr_site)
}

fn codes(report: &recursa_codegen::AnalysisReport) -> Vec<&'static str> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

#[test]
fn greedy_acceptance_retires_a_matching_nullable_follow_overlap() {
    let (mut builder, maybe_site) = nullable_follow_overlap();
    let baseline = analyze(&builder.grammar);
    assert!(
        baseline
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::NullableFollowOverlap),
        "the fixture must produce RCA0300 without an acceptance:\n{}",
        baseline.canonical_diagnostics(),
    );

    builder.accept(maybe_site, 0, &[0]);
    let report = analyze(&builder.grammar);

    assert!(report.output.is_some());
    assert!(
        report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != DiagnosticCode::NullableFollowOverlap
                || matches!(
                    diagnostic.conflict_remedy,
                    Some(recursa_codegen::ConflictRemedy::ReviewNullableFollow { site })
                        if site != maybe_site
                )
        }),
        "the accepted site keeps no RCA0300:\n{}",
        report.canonical_diagnostics(),
    );
    let accepted = report
        .accepted
        .iter()
        .find(|overlap| overlap.site == maybe_site)
        .expect("the acceptance records what it retired");
    assert_eq!(accepted.code, DiagnosticCode::NullableFollowOverlap);
    assert_eq!(accepted.kinds.iter().collect::<Vec<_>>(), [KindId::new(0)]);
}

#[test]
fn stale_greedy_acceptance_is_a_blocking_rca0302_error() {
    let (mut builder, expr_site) = pratt_follow_overlap();
    let tail = SiteId::new(expr_site.index() as u32 + 1);
    assert_eq!(
        builder.grammar.declarations[builder.grammar.sites[tail.index()].declaration.index()].name,
        "Stmt::tail"
    );
    builder.accept(tail, 0, &[1]);
    let report = analyze(&builder.grammar);

    let stale = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::GreedyStale)
        .expect("a site without an overlap finding cannot carry an acceptance");
    assert_eq!(stale.severity, Severity::Error);
    assert!(
        report.canonical_diagnostics().contains("error[RCA0302]"),
        "{}",
        report.canonical_diagnostics()
    );
    assert!(
        stale.message.contains("#[greedy(Post)]") && stale.message.contains("Stmt::tail"),
        "{}",
        stale.message
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::PrattFollowOverlap),
        "the unrelated Pratt overlap at `Stmt::expr` still warns"
    );
}

#[test]
fn stale_kinds_and_undeclared_kinds_are_reported_separately() {
    let (mut builder, maybe_site) = nullable_follow_overlap();
    builder.grammar.kind_count = 2;
    let extra_declaration = builder.declaration("Other");
    builder
        .grammar
        .push_kind(KindDescription::new("Other", extra_declaration).with_authored("Other"));
    builder.accept(maybe_site, 0, &[1]);
    let report = analyze(&builder.grammar);

    let found = codes(&report);
    assert!(
        found.contains(&"RCA0302") && found.contains(&"RCA0303"),
        "{}",
        report.canonical_diagnostics()
    );
    assert_eq!(
        found.iter().filter(|code| **code == "RCA0300").count(),
        2,
        "a mismatched acceptance keeps the original warnings visible:\n{}",
        report.canonical_diagnostics()
    );
    let rendered = report.canonical_diagnostics();
    assert!(rendered.contains("error[RCA0302]"), "{rendered}");
    assert!(rendered.contains("error[RCA0303]"), "{rendered}");
    assert!(
        rendered.contains("does not declare Word"),
        "the undeclared kind is named:\n{rendered}"
    );
    assert!(
        rendered.contains("accepts Other that no caller FOLLOW overlap"),
        "the stale kind is named:\n{rendered}"
    );
    assert!(report.accepted.is_empty());
}

#[test]
fn greedy_acceptance_retires_a_matching_pratt_follow_overlap() {
    let (mut builder, expr_site) = pratt_follow_overlap();
    let baseline = analyze(&builder.grammar);
    assert!(
        baseline
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::PrattFollowOverlap),
        "the fixture must produce RCA0301 without an acceptance:\n{}",
        baseline.canonical_diagnostics(),
    );

    builder.accept(expr_site, 0, &[1]);
    let report = analyze(&builder.grammar);

    assert!(report.output.is_some());
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != DiagnosticCode::PrattFollowOverlap),
        "{}",
        report.canonical_diagnostics()
    );
    assert_eq!(report.accepted.len(), 1);
    assert_eq!(report.accepted[0].site, expr_site);
    assert_eq!(report.accepted[0].code, DiagnosticCode::PrattFollowOverlap);
}

#[test]
fn greedy_all_accepts_any_overlap_but_not_the_absence_of_one() {
    let (mut builder, maybe_site) = nullable_follow_overlap();
    let declaration = builder.grammar.sites[maybe_site.index()].declaration;
    let current = builder.grammar.declarations[declaration.index()].clone();
    builder.grammar.declarations[declaration.index()] =
        current.with_greedy(GreedyAcceptance::all(0));
    let report = analyze(&builder.grammar);
    assert!(
        report.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != DiagnosticCode::NullableFollowOverlap
                || !matches!(
                    diagnostic.conflict_remedy,
                    Some(recursa_codegen::ConflictRemedy::ReviewNullableFollow { site })
                        if site == maybe_site
                )
        }),
        "{}",
        report.canonical_diagnostics()
    );
    assert_eq!(report.accepted.len(), 1);
    assert_eq!(
        report.accepted[0].kinds.iter().collect::<Vec<_>>(),
        [KindId::new(0)]
    );

    let (mut builder, expr_site) = pratt_follow_overlap();
    let tail = SiteId::new(expr_site.index() as u32 + 1);
    let declaration = builder.grammar.sites[tail.index()].declaration;
    let current = builder.grammar.declarations[declaration.index()].clone();
    builder.grammar.declarations[declaration.index()] =
        current.with_greedy(GreedyAcceptance::all(0));
    let report = analyze(&builder.grammar);
    let rendered = report.canonical_diagnostics();
    assert!(rendered.contains("error[RCA0302]"), "{rendered}");
    assert!(
        rendered.contains("`#[greedy(all)]` at `Stmt::tail`"),
        "{rendered}"
    );
}

#[test]
fn one_acceptance_identity_over_several_sites_matches_the_union_of_their_overlaps() {
    let (mut builder, maybe_site) = nullable_follow_overlap();
    let root_inner = SiteId::new(maybe_site.index() as u32 + 1);
    assert_eq!(
        builder.grammar.declarations[builder.grammar.sites[root_inner.index()]
            .declaration
            .index()]
        .name,
        "Root::inner"
    );
    builder.accept(maybe_site, 7, &[0]);
    builder.accept(root_inner, 7, &[0]);
    let report = analyze(&builder.grammar);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != DiagnosticCode::NullableFollowOverlap),
        "both sites of one acceptance are retired together:\n{}",
        report.canonical_diagnostics()
    );
    assert_eq!(report.accepted.len(), 2);
}
