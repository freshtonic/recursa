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

fn expanded_sequence_kinds(
    facts: &recursa_codegen::FirstFacts,
) -> std::collections::BTreeSet<Vec<u16>> {
    let mut expanded = std::collections::BTreeSet::new();
    for sequence in &facts.sequences {
        let mut prefixes = vec![Vec::new()];
        for position in &sequence.positions {
            prefixes = prefixes
                .into_iter()
                .flat_map(|prefix| {
                    position.kinds.iter().map(move |kind| {
                        let mut expanded = prefix.clone();
                        expanded.push(kind.get());
                        expanded
                    })
                })
                .collect();
        }
        expanded.extend(prefixes);
    }
    expanded
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

    let report = analyze(&builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report.output.expect("depth two separates the alternatives");
    assert_eq!(
        sequence_kinds(output.first(statement).expect("Statement FIRST facts")),
        vec![vec![vec![0], vec![1, 2]]],
    );
}

#[test]
fn independent_choices_remain_canonical_at_depth_five() {
    const NAMES: [&str; 8] = ["A", "B", "C", "D", "E", "F", "G", "H"];
    for branch_count in [2, 4, 8] {
        let mut builder = Builder::new(5, &NAMES[..branch_count]);
        let tokens = (0..branch_count)
            .map(|index| builder.token(&format!("Token{index}"), index))
            .collect::<Vec<_>>();
        let choice = builder.choice("Choice", &tokens);
        let root = builder.sequence("Root", &[choice, choice, choice, choice, choice]);

        let report = analyze(&builder.grammar);
        assert!(report.diagnostics.is_empty());
        let output = report
            .output
            .expect("independent choices are conflict-free");
        let sequences = &output.first(root).expect("Root FIRST facts").sequences;

        assert_eq!(
            sequences.len(),
            1,
            "{branch_count}^5 independent choices should stay one canonical rectangular prefix",
        );
        assert_eq!(
            sequences[0]
                .positions
                .iter()
                .map(|position| position.kinds.iter().count())
                .collect::<Vec<_>>(),
            vec![branch_count; 5],
        );
    }
}

#[test]
fn rectangle_canonicalization_preserves_non_rectangular_language() {
    let mut builder = Builder::new(2, &["A", "B", "C", "D"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let c = builder.token("C", 2);
    let d = builder.token("D", 3);
    let ac = builder.sequence("Ac", &[a, c]);
    let ad = builder.sequence("Ad", &[a, d]);
    let bc = builder.sequence("Bc", &[b, c]);
    let root = builder.choice("Root", &[ac, ad, bc]);

    let report = analyze(&builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report.output.expect("depth two separates the alternatives");
    assert_eq!(
        expanded_sequence_kinds(output.first(root).expect("Root FIRST facts")),
        std::collections::BTreeSet::from([vec![0, 2], vec![0, 3], vec![1, 2]]),
    );
}

#[test]
fn rectangle_canonicalization_preserves_dispatch_conflicts() {
    let mut builder = Builder::new(2, &["A", "B", "C", "D"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let c = builder.token("C", 2);
    let d = builder.token("D", 3);
    let left = builder.choice("Left", &[a, b]);
    let right = builder.choice("Right", &[c, d]);
    let rectangle = builder.sequence("Rectangle", &[left, right]);
    let ac = builder.sequence("Ac", &[a, c]);
    builder.choice("Root", &[rectangle, ac]);

    let report = analyze(&builder.grammar);
    assert!(report.output.is_none());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::EnumFirstOverlap),
    );
}

fn overlapping_rectangle_grammar(reverse: bool) -> (GrammarDescription, TypeId) {
    let mut builder = Builder::new(2, &["A", "B", "C", "D"]);
    let left_lead = builder.atom(
        "LeftLead",
        AtomExpression::Category(vec![builder.atoms[0], builder.atoms[1]]),
    );
    let right_lead = builder.atom(
        "RightLead",
        AtomExpression::Category(vec![builder.atoms[1], builder.atoms[2]]),
    );
    let left_lead = builder.token_from_atom("LeftLeadToken", left_lead, None);
    let right_lead = builder.token_from_atom("RightLeadToken", right_lead, None);
    let c = builder.token("C", 2);
    let d = builder.token("D", 3);
    let left = builder.sequence("Left", &[left_lead, c]);
    let right = builder.sequence("Right", &[right_lead, d]);
    let root = builder.choice("Root", &[left, right]);
    if reverse {
        let TypeShape::Enum { alternatives } = &mut builder.grammar.types[root.index()].shape
        else {
            unreachable!("Root is an enum")
        };
        alternatives.reverse();
    }
    (builder.grammar, root)
}

#[test]
fn overlapping_prefix_rectangles_freeze_disjoint_subset_edges() {
    let (grammar, root) = overlapping_rectangle_grammar(false);
    let output = analyze(&grammar)
        .output
        .expect("full languages are disjoint");
    let dispatch = output
        .decisions()
        .dispatches()
        .iter()
        .find(|dispatch| dispatch.owner() == root)
        .expect("Root dispatch");
    let root_edges = dispatch.trie().edges();
    assert_eq!(root_edges.len(), 3, "A, B, and C are distinct cells");
    for (index, left) in root_edges.iter().enumerate() {
        for right in &root_edges[index + 1..] {
            assert!(
                left.position()
                    .kinds()
                    .intersection(right.position().kinds())
                    .unwrap()
                    .is_empty(),
                "frozen root edges must be pairwise disjoint",
            );
        }
    }
    let shared = root_edges
        .iter()
        .find(|edge| {
            edge.position()
                .kinds()
                .contains(recursa_codegen::KindId::new(1))
        })
        .expect("B subset cell");
    assert_eq!(
        shared.next().edges().len(),
        2,
        "B retains both C and D residual languages"
    );

    let (reversed, reversed_root) = overlapping_rectangle_grammar(true);
    let reversed = analyze(&reversed)
        .output
        .expect("reversed grammar succeeds");
    let reversed = reversed
        .decisions()
        .dispatches()
        .iter()
        .find(|dispatch| dispatch.owner() == reversed_root)
        .expect("reversed Root dispatch");
    assert_eq!(dispatch.trie(), reversed.trie());
}

#[test]
fn overlapping_text_predicates_freeze_exact_and_other_cells() {
    let mut builder = Builder::new(2, &["WORD", "C", "D"]);
    let one_of = builder
        .grammar
        .push_text_predicate(recursa_codegen::TextPredicate::OneOf(vec![
            "a".into(),
            "b".into(),
        ]));
    let not_a = builder
        .grammar
        .push_text_predicate(recursa_codegen::TextPredicate::NotIn(vec!["a".into()]));
    let left_lead = builder.token_from_atom("LeftText", builder.atoms[0], Some(one_of));
    let right_lead = builder.token_from_atom("RightText", builder.atoms[0], Some(not_a));
    let c = builder.token("C", 1);
    let d = builder.token("D", 2);
    let left = builder.sequence("LeftTextSequence", &[left_lead, c]);
    let right = builder.sequence("RightTextSequence", &[right_lead, d]);
    let root = builder.choice("TextRoot", &[left, right]);

    let output = analyze(&builder.grammar)
        .output
        .expect("suffixes disambiguate overlapping text predicates");
    let dispatch = output
        .decisions()
        .dispatches()
        .iter()
        .find(|dispatch| dispatch.owner() == root)
        .expect("TextRoot dispatch");
    let predicates = dispatch
        .trie()
        .edges()
        .iter()
        .map(|edge| edge.position().text_predicate().cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        predicates,
        vec![
            Some(recursa_codegen::TextPredicate::Exact("a".into())),
            Some(recursa_codegen::TextPredicate::Exact("b".into())),
            Some(recursa_codegen::TextPredicate::NotIn(vec![
                "a".into(),
                "b".into()
            ])),
        ],
    );
    let shared = dispatch
        .trie()
        .edges()
        .iter()
        .find(|edge| {
            edge.position().text_predicate()
                == Some(&recursa_codegen::TextPredicate::Exact("b".into()))
        })
        .expect("shared b cell");
    assert_eq!(shared.next().edges().len(), 2);
}

#[test]
fn shorter_language_freezes_caller_follow_before_a_longer_suffix() {
    let mut builder = Builder::new(2, &["A", "B"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let long = builder.sequence("Long", &[a, b]);
    let root = builder.choice("Root", &[a, long]);
    let output = analyze(&builder.grammar)
        .output
        .expect("end-of-language distinguishes the branches");
    let dispatch = output
        .decisions()
        .dispatches()
        .iter()
        .find(|dispatch| dispatch.owner() == root)
        .expect("Root dispatch");
    let after_a = dispatch.trie().edges()[0].next();
    assert!(
        after_a
            .terminal()
            .is_some_and(|terminal| terminal.on_caller_follow()),
        "the shorter A language consults caller FOLLOW"
    );
    assert_eq!(after_a.edges().len(), 1, "B continues the longer language");
}

#[test]
fn pg_like_wide_choice_at_depth_five_stays_symbolic() {
    let names = (0..256)
        .map(|index| format!("K{index}"))
        .collect::<Vec<_>>();
    let borrowed = names.iter().map(String::as_str).collect::<Vec<_>>();
    let mut builder = Builder::new(5, &borrowed);
    let tokens = (0..names.len())
        .map(|index| builder.token(&format!("Token{index}"), index))
        .collect::<Vec<_>>();
    let choice = builder.choice("WideChoice", &tokens);
    let root = builder.sequence("Root", &[choice, choice, choice, choice, choice]);

    let output = analyze(&builder.grammar)
        .output
        .expect("wide five-position grammar remains analyzable");
    let sequences = &output.first(root).expect("Root FIRST facts").sequences;
    assert_eq!(sequences.len(), 1, "256^5 prefixes stay one rectangle");
    assert_eq!(
        sequences[0]
            .positions
            .iter()
            .map(|position| position.kinds.iter().count())
            .collect::<Vec<_>>(),
        vec![256; 5],
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
    let report = analyze(&depth_three_grammar(2));
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

    let report = analyze(&depth_three_grammar(3));
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

    let report = analyze(&builder.grammar);
    assert!(report.diagnostics.is_empty());
    let output = report
        .output
        .expect("nullable branch separates at depth two");
    assert_eq!(
        sequence_kinds(output.first(statement).expect("Statement FIRST facts")),
        vec![vec![vec![0], vec![1, 2, 3]], vec![vec![1, 2, 3]],],
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

    let report = analyze(&builder.grammar);
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

    let report = analyze(&builder.grammar);
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
    let report = analyze(&builder.grammar);
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

    let report = analyze(&builder.grammar);
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
    let report = analyze(&grammar);
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

    let report = analyze(&builder.grammar);
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
    let forward_success = analyze(&shuffled_depth_three(false, 3));
    let reverse_success = analyze(&shuffled_depth_three(true, 3));
    let forward_output = forward_success.output.unwrap();
    let reverse_output = reverse_success.output.unwrap();
    assert_eq!(
        forward_output.first(TypeId::new(6)).unwrap().sequences,
        reverse_output.first(TypeId::new(6)).unwrap().sequences,
    );

    let forward_error = analyze(&shuffled_depth_three(false, 2));
    let reverse_error = analyze(&shuffled_depth_three(true, 2));
    assert_eq!(forward_error.diagnostics, reverse_error.diagnostics);
    assert_eq!(
        forward_error.canonical_diagnostics(),
        reverse_error.canonical_diagnostics(),
    );
}

#[test]
fn lookahead_can_disambiguate_a_nullable_use_site_from_its_tail() {
    let report = analyze(&nullable_tail_prefix(2));
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
    let report = analyze(&nullable_tail_prefix(1));
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
    let report = analyze(&long_finite_prefix(2, 40));
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

    let report = analyze(&long_finite_prefix(41, 40));
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
    let report = analyze(&consuming_optional_prefix(2));
    assert_eq!(
        report.diagnostics[0]
            .lookahead
            .as_ref()
            .unwrap()
            .sufficient_depth,
        Some(3),
    );

    let report = analyze(&consuming_optional_prefix(3));
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
    let report = analyze(&empty_alternatives());
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
    let report = analyze(&right_recursive_shared_prefix());
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
    let report = analyze(&nullable_prefix_before_recursive_alternatives(1));
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("`Choice`"))
        .expect("depth-one shared-prefix diagnostic");
    assert_eq!(
        diagnostic.lookahead.as_ref().unwrap().sufficient_depth,
        Some(2),
    );

    let report = analyze(&nullable_prefix_before_recursive_alternatives(2));
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

    let report = analyze(&builder.grammar);
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
