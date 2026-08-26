use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ConflictRemedy, ContainerShape, Declaration,
    DiagnosticCode, GrammarDescription, KindDescription, Location, ParseSite, SourceId,
    TypeDescription, TypeId, TypeShape, analyze,
};

struct Builder {
    grammar: GrammarDescription,
    next_byte: u32,
    atoms: Vec<recursa_codegen::AtomId>,
}

impl Builder {
    fn new(max_lookahead: usize, kinds: &[&str]) -> Self {
        let mut builder = Self {
            grammar: GrammarDescription::new(
                "first-k",
                kinds.len(),
                max_lookahead,
                recursa_codegen::TextEquivalence::AsciiInsensitive,
            ),
            next_byte: 0,
            atoms: Vec::new(),
        };
        for &name in kinds {
            let declaration = builder.declaration(name);
            let kind = builder
                .grammar
                .push_kind(KindDescription::new(name, declaration));
            let atom = builder.grammar.push_atom(AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            builder.atoms.push(atom);
        }
        builder
    }

    fn declaration(&mut self, name: &str) -> recursa_codegen::DeclId {
        let start = self.next_byte;
        self.next_byte += 2;
        let location = self
            .grammar
            .push_location(Location::new(SourceId::new(0), start..start + 1));
        self.grammar
            .push_declaration(Declaration::new(name, location))
    }

    fn token(&mut self, name: &str, kind: usize) -> TypeId {
        self.token_from_atom(name, self.atoms[kind], None)
    }

    fn token_from_atom(
        &mut self,
        name: &str,
        atom: recursa_codegen::AtomId,
        text_predicate: Option<recursa_codegen::TextPredicateId>,
    ) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom,
                text_predicate,
            },
        ))
    }

    fn atom(&mut self, name: &str, expression: AtomExpression) -> recursa_codegen::AtomId {
        let declaration = self.declaration(name);
        self.grammar
            .push_atom(AtomDescription::new(name, declaration, expression))
    }

    fn sequence(&mut self, name: &str, targets: &[TypeId]) -> TypeId {
        self.sequence_with(
            name,
            &targets
                .iter()
                .map(|target| (*target, ContainerShape::Direct))
                .collect::<Vec<_>>(),
        )
    }

    fn sequence_with(&mut self, name: &str, targets: &[(TypeId, ContainerShape)]) -> TypeId {
        let owner = self.grammar.next_type_id();
        let declaration = self.declaration(name);
        let sites = targets
            .iter()
            .enumerate()
            .map(|(index, (target, container))| {
                let site_declaration = self.declaration(&format!("{name}::{index}"));
                self.grammar.push_site(ParseSite::new(
                    owner,
                    *target,
                    site_declaration,
                    container.clone(),
                ))
            })
            .collect();
        assert_eq!(
            self.grammar.push_type(TypeDescription::new(
                name,
                declaration,
                TypeShape::Sequence { sites },
            )),
            owner,
        );
        owner
    }

    fn choice(&mut self, name: &str, targets: &[TypeId]) -> TypeId {
        let owner = self.grammar.next_type_id();
        let declaration = self.declaration(name);
        let alternatives = targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let alternative = self.declaration(&format!("{name}::{index}"));
                let site = self.grammar.push_site(ParseSite::new(
                    owner,
                    *target,
                    alternative,
                    ContainerShape::Direct,
                ));
                Alternative {
                    declaration: alternative,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            self.grammar.push_type(TypeDescription::new(
                name,
                declaration,
                TypeShape::Enum { alternatives },
            )),
            owner,
        );
        owner
    }
}

fn sequence_kinds(facts: &recursa_codegen::FirstFacts) -> Vec<Vec<Vec<u16>>> {
    facts
        .sequences
        .iter()
        .map(|sequence| {
            sequence
                .positions
                .iter()
                .map(|position| position.kinds.iter().map(|kind| kind.get()).collect())
                .collect()
        })
        .collect()
}

#[test]
fn shared_create_prefix_disambiguates_at_depth_two() {
    let mut builder = Builder::new(2, &["CREATE", "TABLE", "VIEW"]);
    let create = builder.token("Create", 0);
    let table = builder.token("Table", 1);
    let view = builder.token("View", 2);
    let create_table = builder.sequence("CreateTable", &[create, table]);
    let create_view = builder.sequence("CreateView", &[create, view]);
    let statement = builder.choice("Statement", &[create_table, create_view]);

    let report = analyze(builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report.output.expect("depth two separates the alternatives");
    assert_eq!(
        sequence_kinds(output.first(statement).expect("Statement FIRST facts")),
        vec![vec![vec![0], vec![1]], vec![vec![0], vec![2]]],
    );
}

fn depth_three_grammar(max_lookahead: usize) -> GrammarDescription {
    let mut builder = Builder::new(max_lookahead, &["A", "B", "C", "D"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let c = builder.token("C", 2);
    let d = builder.token("D", 3);
    let abc = builder.sequence("Abc", &[a, b, c]);
    let abd = builder.sequence("Abd", &[a, b, d]);
    builder.choice("Choice", &[abc, abd]);
    builder.grammar
}

#[test]
fn exhausted_depth_reports_the_computable_sufficient_depth() {
    let report = analyze(depth_three_grammar(2));
    assert!(report.output.is_none());
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::EnumFirstOverlap);
    let lookahead = diagnostic
        .lookahead
        .as_ref()
        .expect("shared-prefix evidence");
    assert_eq!(lookahead.configured_depth, 2);
    assert_eq!(lookahead.sufficient_depth, Some(3));
    assert_eq!(
        lookahead
            .witness
            .iter()
            .map(|position| position.kind.get())
            .collect::<Vec<_>>(),
        vec![0, 1],
    );
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RaiseMaxLookahead {
            configured: 2,
            sufficient: 3,
        })
    ));
    assert!(
        report
            .canonical_diagnostics()
            .contains("remedy: raise max_lookahead from 2 to 3")
    );

    let report = analyze(depth_three_grammar(3));
    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

#[test]
fn nullable_leading_tokens_produce_all_symbolic_first_two_sequences() {
    let mut builder = Builder::new(2, &["DEBUG", "PRINT", "READ", "SET"]);
    let debug = builder.token("Debug", 0);
    let print = builder.token("Print", 1);
    let read = builder.token("Read", 2);
    let set = builder.token("Set", 3);
    let print_statement = builder.sequence_with(
        "PrintStatement",
        &[
            (debug, ContainerShape::optional(ContainerShape::Direct)),
            (print, ContainerShape::Direct),
        ],
    );
    let read_statement = builder.sequence_with(
        "ReadStatement",
        &[
            (debug, ContainerShape::optional(ContainerShape::Direct)),
            (read, ContainerShape::Direct),
        ],
    );
    let set_statement = builder.sequence_with(
        "SetStatement",
        &[
            (debug, ContainerShape::optional(ContainerShape::Direct)),
            (set, ContainerShape::Direct),
        ],
    );
    let statement = builder.choice(
        "Statement",
        &[print_statement, read_statement, set_statement],
    );

    let report = analyze(builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report
        .output
        .expect("nullable branch separates at depth two");
    assert_eq!(
        sequence_kinds(output.first(statement).expect("Statement FIRST facts")),
        vec![
            vec![vec![0], vec![1]],
            vec![vec![0], vec![2]],
            vec![vec![0], vec![3]],
            vec![vec![1]],
            vec![vec![2]],
            vec![vec![3]],
        ],
    );
}

#[test]
fn internal_text_predicates_separate_a_shared_kind_at_depth_two() {
    let mut builder = Builder::new(2, &["COMMAND", "WORD"]);
    let table = builder
        .grammar
        .push_text_predicate(recursa_codegen::TextPredicate::Exact("TABLE".to_owned()));
    let view = builder
        .grammar
        .push_text_predicate(recursa_codegen::TextPredicate::Exact("VIEW".to_owned()));
    let command = builder.token("Command", 0);
    let table = builder.token_from_atom("Table", builder.atoms[1], Some(table));
    let view = builder.token_from_atom("View", builder.atoms[1], Some(view));
    let table_statement = builder.sequence("TableStatement", &[command, table]);
    let view_statement = builder.sequence("ViewStatement", &[command, view]);
    builder.choice("Statement", &[table_statement, view_statement]);

    let report = analyze(builder.grammar);
    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

#[test]
fn first_k_positions_keep_partial_kind_sets_symbolic() {
    let mut builder = Builder::new(2, &["A", "B", "C", "X", "Y"]);
    let left = builder.atom(
        "LeftKinds",
        AtomExpression::Category(vec![builder.atoms[0], builder.atoms[1]]),
    );
    let right = builder.atom(
        "RightKinds",
        AtomExpression::Category(vec![builder.atoms[1], builder.atoms[2]]),
    );
    let left = builder.token_from_atom("Left", left, None);
    let right = builder.token_from_atom("Right", right, None);
    let x = builder.token("X", 3);
    let y = builder.token("Y", 4);
    let left = builder.sequence("LeftSequence", &[left, x]);
    let right = builder.sequence("RightSequence", &[right, y]);
    let choice = builder.choice("Choice", &[left, right]);

    let report = analyze(builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report.output.expect("second position separates overlap");
    assert_eq!(
        sequence_kinds(output.first(choice).expect("Choice FIRST facts")),
        vec![vec![vec![0, 1], vec![3]], vec![vec![1, 2], vec![4]],],
    );
}

#[test]
fn an_unequal_length_prefix_is_separated_by_end_of_alternative() {
    let mut builder = Builder::new(2, &["A", "B"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let ab = builder.sequence("Ab", &[a, b]);
    builder.choice("Choice", &[a, ab]);

    let predicates = builder.grammar.text_predicates.clone();
    let report = analyze(builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report.output.unwrap();
    assert_eq!(
        recursa_codegen::disambiguation_depth(
            &output.first(a).unwrap().sequences,
            &output.first(ab).unwrap().sequences,
            &predicates,
            recursa_codegen::TextEquivalence::AsciiInsensitive,
            2,
        ),
        Some(2),
    );
}

#[test]
fn a_repeated_shared_prefix_recommends_restructuring() {
    let mut builder = Builder::new(3, &["X", "A", "B"]);
    let x = builder.token("X", 0);
    let a = builder.token("A", 1);
    let b = builder.token("B", 2);
    let repeated = ContainerShape::boxed(ContainerShape::one_or_more(
        ContainerShape::boxed(ContainerShape::Direct),
        None,
    ));
    let left = builder.sequence_with(
        "Left",
        &[(x, repeated.clone()), (a, ContainerShape::Direct)],
    );
    let right = builder.sequence_with("Right", &[(x, repeated), (b, ContainerShape::Direct)]);
    builder.choice("Choice", &[left, right]);

    let report = analyze(builder.grammar);
    let diagnostic = &report.diagnostics[0];
    let lookahead = diagnostic.lookahead.as_ref().expect("lookahead evidence");
    assert_eq!(lookahead.configured_depth, 3);
    assert_eq!(lookahead.sufficient_depth, None);
    assert_eq!(lookahead.witness.len(), 3);
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RestructureSharedPrefix { .. })
    ));
    assert!(
        report
            .canonical_diagnostics()
            .contains("no finite depth separates this shared prefix")
    );
}

#[test]
fn disambiguation_depth_returns_the_first_separating_position() {
    let grammar = depth_three_grammar(3);
    let predicates = grammar.text_predicates.clone();
    let report = analyze(grammar);
    let output = report.output.expect("depth three succeeds");

    assert_eq!(
        recursa_codegen::disambiguation_depth(
            &output.first(TypeId::new(4)).unwrap().sequences,
            &output.first(TypeId::new(5)).unwrap().sequences,
            &predicates,
            recursa_codegen::TextEquivalence::AsciiInsensitive,
            3,
        ),
        Some(3),
    );
}

#[test]
fn every_pair_of_three_shared_prefix_alternatives_is_diagnosed() {
    let mut builder = Builder::new(2, &["A", "B", "C", "D", "E"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let c = builder.token("C", 2);
    let d = builder.token("D", 3);
    let e = builder.token("E", 4);
    let abc = builder.sequence("Abc", &[a, b, c]);
    let abd = builder.sequence("Abd", &[a, b, d]);
    let abe = builder.sequence("Abe", &[a, b, e]);
    builder.choice("Choice", &[abc, abd, abe]);

    let report = analyze(builder.grammar);
    assert_eq!(report.diagnostics.len(), 3);
    assert!(report.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .lookahead
            .as_ref()
            .is_some_and(|evidence| evidence.sufficient_depth == Some(3))
    }));
}

fn shuffled_depth_three(reverse: bool, max_lookahead: usize) -> GrammarDescription {
    let mut grammar = depth_three_grammar(max_lookahead);
    if reverse {
        let TypeShape::Enum { alternatives } = &mut grammar.types.last_mut().unwrap().shape else {
            unreachable!("last type is Choice")
        };
        alternatives.reverse();
    }
    grammar
}

#[test]
fn first_k_sequences_witnesses_and_rendering_ignore_construction_order() {
    let forward_success = analyze(shuffled_depth_three(false, 3));
    let reverse_success = analyze(shuffled_depth_three(true, 3));
    let forward_output = forward_success.output.unwrap();
    let reverse_output = reverse_success.output.unwrap();
    assert_eq!(
        forward_output.first(TypeId::new(6)).unwrap().sequences,
        reverse_output.first(TypeId::new(6)).unwrap().sequences,
    );

    let forward_error = analyze(shuffled_depth_three(false, 2));
    let reverse_error = analyze(shuffled_depth_three(true, 2));
    assert_eq!(forward_error.diagnostics, reverse_error.diagnostics);
    assert_eq!(
        forward_error.canonical_diagnostics(),
        reverse_error.canonical_diagnostics(),
    );
}

#[test]
fn lookahead_can_disambiguate_a_nullable_use_site_from_its_tail() {
    let report = analyze(nullable_tail_prefix(2));
    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

fn nullable_tail_prefix(max_lookahead: usize) -> GrammarDescription {
    let mut builder = Builder::new(max_lookahead, &["A", "X", "Y"]);
    let a = builder.token("A", 0);
    let x = builder.token("X", 1);
    let y = builder.token("Y", 2);
    let optional = builder.sequence("OptionalHead", &[a, x]);
    let required = builder.sequence("RequiredTail", &[a, y]);
    builder.sequence_with(
        "Container",
        &[
            (optional, ContainerShape::optional(ContainerShape::Direct)),
            (required, ContainerShape::Direct),
        ],
    );

    builder.grammar
}

#[test]
fn nullable_use_site_reports_a_sufficient_higher_depth() {
    let report = analyze(nullable_tail_prefix(1));
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::StaticUseSiteOverlap);
    assert_eq!(
        diagnostic.lookahead.as_ref().unwrap().sufficient_depth,
        Some(2),
    );
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RaiseMaxLookahead {
            configured: 1,
            sufficient: 2,
        })
    ));
}

fn long_finite_prefix(max_lookahead: usize, shared: usize) -> GrammarDescription {
    let mut builder = Builder::new(max_lookahead, &["A", "X", "Y"]);
    let a = builder.token("A", 0);
    let x = builder.token("X", 1);
    let y = builder.token("Y", 2);
    let mut left = vec![a; shared];
    left.push(x);
    let mut right = vec![a; shared];
    right.push(y);
    let left = builder.sequence("Left", &left);
    let right = builder.sequence("Right", &right);
    builder.choice("Choice", &[left, right]);
    builder.grammar
}

#[test]
fn a_long_finite_prefix_reports_its_exact_sufficient_depth() {
    let report = analyze(long_finite_prefix(2, 40));
    let diagnostic = &report.diagnostics[0];
    let lookahead = diagnostic.lookahead.as_ref().unwrap();
    assert_eq!(lookahead.sufficient_depth, Some(41));
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RaiseMaxLookahead {
            configured: 2,
            sufficient: 41,
        })
    ));

    let report = analyze(long_finite_prefix(41, 40));
    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

fn consuming_optional_prefix(max_lookahead: usize) -> GrammarDescription {
    let mut builder = Builder::new(max_lookahead, &["A", "B"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    builder.sequence_with(
        "Container",
        &[
            (a, ContainerShape::optional(ContainerShape::Direct)),
            (a, ContainerShape::Direct),
            (a, ContainerShape::Direct),
            (b, ContainerShape::Direct),
        ],
    );
    builder.grammar
}

#[test]
fn nullable_decision_compares_consuming_plus_tail_against_skipping_tail() {
    let report = analyze(consuming_optional_prefix(2));
    assert_eq!(
        report.diagnostics[0]
            .lookahead
            .as_ref()
            .unwrap()
            .sufficient_depth,
        Some(3),
    );

    let report = analyze(consuming_optional_prefix(3));
    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

fn empty_alternatives() -> GrammarDescription {
    let mut builder = Builder::new(2, &["A"]);
    let empty_left = builder.sequence("EmptyLeft", &[]);
    let empty_right = builder.sequence("EmptyRight", &[]);
    builder.choice("Choice", &[empty_left, empty_right]);
    builder.grammar
}

#[test]
fn empty_derivations_have_structured_end_of_alternative_evidence() {
    let report = analyze(empty_alternatives());
    assert!(report.output.is_none());
    let diagnostic = &report.diagnostics[0];
    let lookahead = diagnostic.lookahead.as_ref().unwrap();
    assert!(lookahead.end_of_alternative);
    assert!(
        report
            .canonical_diagnostics()
            .contains("shared prefix: <end>")
    );
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RestructureSharedPrefix { .. })
    ));
}

fn right_recursive_shared_prefix() -> GrammarDescription {
    let mut builder = Builder::new(2, &["A", "X", "Y", "L", "R"]);
    let a = builder.token("A", 0);
    let x = builder.token("X", 1);
    let y = builder.token("Y", 2);
    let l = builder.token("L", 3);
    let r = builder.token("R", 4);

    let left = builder.grammar.next_type_id();
    let left_declaration = builder.declaration("Left");
    let left_recursive_declaration = builder.declaration("Left::Recursive");
    let left_recursive_a = builder.grammar.push_site(ParseSite::new(
        left,
        a,
        left_recursive_declaration,
        ContainerShape::Direct,
    ));
    let left_recursive_self = builder.grammar.push_site(ParseSite::new(
        left,
        left,
        left_recursive_declaration,
        ContainerShape::Direct,
    ));
    let left_recursive_l = builder.grammar.push_site(ParseSite::new(
        left,
        l,
        left_recursive_declaration,
        ContainerShape::Direct,
    ));
    let left_base_declaration = builder.declaration("Left::Base");
    let left_base = builder.grammar.push_site(ParseSite::new(
        left,
        x,
        left_base_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Left",
            left_declaration,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: left_recursive_declaration,
                        sites: vec![left_recursive_a, left_recursive_self, left_recursive_l],
                    },
                    Alternative {
                        declaration: left_base_declaration,
                        sites: vec![left_base],
                    },
                ],
            },
        )),
        left,
    );

    let right = builder.grammar.next_type_id();
    let right_declaration = builder.declaration("Right");
    let right_recursive_declaration = builder.declaration("Right::Recursive");
    let right_recursive_a = builder.grammar.push_site(ParseSite::new(
        right,
        a,
        right_recursive_declaration,
        ContainerShape::Direct,
    ));
    let right_recursive_self = builder.grammar.push_site(ParseSite::new(
        right,
        right,
        right_recursive_declaration,
        ContainerShape::Direct,
    ));
    let right_recursive_r = builder.grammar.push_site(ParseSite::new(
        right,
        r,
        right_recursive_declaration,
        ContainerShape::Direct,
    ));
    let right_base_declaration = builder.declaration("Right::Base");
    let right_base = builder.grammar.push_site(ParseSite::new(
        right,
        y,
        right_base_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Right",
            right_declaration,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: right_recursive_declaration,
                        sites: vec![right_recursive_a, right_recursive_self, right_recursive_r],
                    },
                    Alternative {
                        declaration: right_base_declaration,
                        sites: vec![right_base],
                    },
                ],
            },
        )),
        right,
    );

    builder.choice("Choice", &[left, right]);
    builder.grammar
}

#[test]
fn unequal_recursive_tails_are_pumped_as_an_unbounded_shared_prefix() {
    let report = analyze(right_recursive_shared_prefix());
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("`Choice`"))
        .expect("root shared-prefix diagnostic");
    let lookahead = diagnostic.lookahead.as_ref().unwrap();
    assert_eq!(lookahead.sufficient_depth, None);
    assert!(!lookahead.end_of_alternative);
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RestructureSharedPrefix { .. })
    ));
}

fn nullable_prefix_before_recursive_alternatives(max_lookahead: usize) -> GrammarDescription {
    let mut builder = Builder::new(max_lookahead, &["A", "L", "R", "X", "Y"]);
    let a = builder.token("A", 0);
    let l = builder.token("L", 1);
    let r = builder.token("R", 2);
    let x = builder.token("X", 3);
    let y = builder.token("Y", 4);
    let empty = builder.sequence("Empty", &[]);

    let left = builder.grammar.next_type_id();
    let left_declaration = builder.declaration("Left");
    let left_recursive_declaration = builder.declaration("Left::Recursive");
    let left_recursive_sites = [a, empty, l, left]
        .into_iter()
        .map(|target| {
            builder.grammar.push_site(ParseSite::new(
                left,
                target,
                left_recursive_declaration,
                ContainerShape::Direct,
            ))
        })
        .collect::<Vec<_>>();
    let left_base_declaration = builder.declaration("Left::Base");
    let left_base = builder.grammar.push_site(ParseSite::new(
        left,
        x,
        left_base_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Left",
            left_declaration,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: left_recursive_declaration,
                        sites: left_recursive_sites,
                    },
                    Alternative {
                        declaration: left_base_declaration,
                        sites: vec![left_base],
                    },
                ],
            },
        )),
        left,
    );

    let right = builder.grammar.next_type_id();
    let right_declaration = builder.declaration("Right");
    let right_recursive_declaration = builder.declaration("Right::Recursive");
    let right_recursive_sites = [a, empty, r, right]
        .into_iter()
        .map(|target| {
            builder.grammar.push_site(ParseSite::new(
                right,
                target,
                right_recursive_declaration,
                ContainerShape::Direct,
            ))
        })
        .collect::<Vec<_>>();
    let right_base_declaration = builder.declaration("Right::Base");
    let right_base = builder.grammar.push_site(ParseSite::new(
        right,
        y,
        right_base_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Right",
            right_declaration,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: right_recursive_declaration,
                        sites: right_recursive_sites,
                    },
                    Alternative {
                        declaration: right_base_declaration,
                        sites: vec![right_base],
                    },
                ],
            },
        )),
        right,
    );

    let choice = builder.grammar.next_type_id();
    let choice_declaration = builder.declaration("Choice");
    let left_choice_declaration = builder.declaration("Choice::Left");
    let left_root_sites = [empty, left]
        .into_iter()
        .map(|target| {
            builder.grammar.push_site(ParseSite::new(
                choice,
                target,
                left_choice_declaration,
                ContainerShape::Direct,
            ))
        })
        .collect();
    let right_choice_declaration = builder.declaration("Choice::Right");
    let right_root_sites = [empty, right]
        .into_iter()
        .map(|target| {
            builder.grammar.push_site(ParseSite::new(
                choice,
                target,
                right_choice_declaration,
                ContainerShape::Direct,
            ))
        })
        .collect();
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Choice",
            choice_declaration,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: left_choice_declaration,
                        sites: left_root_sites,
                    },
                    Alternative {
                        declaration: right_choice_declaration,
                        sites: right_root_sites,
                    },
                ],
            },
        )),
        choice,
    );
    builder.grammar
}

#[test]
fn pumping_does_not_cross_a_nullable_protected_continuation() {
    let report = analyze(nullable_prefix_before_recursive_alternatives(1));
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("`Choice`"))
        .expect("depth-one shared-prefix diagnostic");
    assert_eq!(
        diagnostic.lookahead.as_ref().unwrap().sufficient_depth,
        Some(2),
    );

    let report = analyze(nullable_prefix_before_recursive_alternatives(2));
    assert!(report.diagnostics.is_empty());
    assert!(report.output.is_some());
}

#[test]
fn compressed_finite_prefix_ignores_an_unrelated_recursive_branch() {
    let mut builder = Builder::new(2, &["A", "X", "Y", "Z"]);
    let a = builder.token("A", 0);
    let x = builder.token("X", 1);
    let y = builder.token("Y", 2);
    let z = builder.token("Z", 3);

    let recursive = builder.grammar.next_type_id();
    let recursive_declaration = builder.declaration("Recursive");
    let step_declaration = builder.declaration("Recursive::Step");
    let step_a = builder.grammar.push_site(ParseSite::new(
        recursive,
        a,
        step_declaration,
        ContainerShape::Direct,
    ));
    let step_self = builder.grammar.push_site(ParseSite::new(
        recursive,
        recursive,
        step_declaration,
        ContainerShape::Direct,
    ));
    let base_declaration = builder.declaration("Recursive::Base");
    let base = builder.grammar.push_site(ParseSite::new(
        recursive,
        z,
        base_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Recursive",
            recursive_declaration,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: step_declaration,
                        sites: vec![step_a, step_self],
                    },
                    Alternative {
                        declaration: base_declaration,
                        sites: vec![base],
                    },
                ],
            },
        )),
        recursive,
    );

    let chunk = builder.sequence("Chunk", &[a; 10]);
    let mut left = vec![chunk; 10];
    left.push(x);
    let mut right = vec![chunk; 10];
    right.push(y);
    let left = builder.sequence("Left", &left);
    let right = builder.sequence("Right", &right);
    builder.choice("Choice", &[left, right]);

    let report = analyze(builder.grammar);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("`Choice`"))
        .expect("finite shared-prefix diagnostic");
    assert_eq!(
        diagnostic.lookahead.as_ref().unwrap().sufficient_depth,
        Some(101),
    );
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RaiseMaxLookahead {
            configured: 2,
            sufficient: 101,
        })
    ));
}
