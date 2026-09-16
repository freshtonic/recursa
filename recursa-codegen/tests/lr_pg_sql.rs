//! Manual: the LALR(1) construction over the pg-sql grammar.
//!
//! This lives outside the library's unit tests because the library test build runs the
//! materializer's cycle-cut oracle, which is exponential on a grammar of pg-sql's size; the
//! production path is the fixed point the build script uses.

use std::path::{Path, PathBuf};

use recursa_codegen::lr::{
    AutomatonOptions, ConflictKind, LrAnalysis, NonterminalOrigin, PACT_NINF,
};

/// Where the pg-sql checkout lives: `PG_SQL_MANIFEST_DIR`, else the sibling `pg-sql` directory.
fn pg_sql_manifest_dir() -> PathBuf {
    std::env::var_os("PG_SQL_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("recursa-codegen is in the workspace root")
                .parent()
                .expect("the workspace has a parent")
                .join("pg-sql")
        })
}

/// Lowers the pg-sql grammar, builds its automaton and prints the sizes and the complete
/// conflict list; the full dump and conflict report are written under `target/lr-pg-sql`.
///
/// ```sh
/// cargo test -p recursa-codegen --release --test lr_pg_sql --locked \
///   pg_sql_grammar_constructs_and_reports_its_conflicts -- --ignored --exact --nocapture
/// ```
#[test]
#[ignore = "manual representative-scale run over the pg-sql grammar; see docs/agents/testing.md"]
fn pg_sql_grammar_constructs_and_reports_its_conflicts() {
    let worker = std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(|| {
            let started = std::time::Instant::now();
            let analysis = LrAnalysis::analyze_source(
                &pg_sql_manifest_dir(),
                Path::new("src/lib.rs"),
                AutomatonOptions::default(),
            )
            .unwrap_or_else(|error| panic!("pg-sql did not analyze:\n{error}"));
            let grammar = &analysis.grammar;
            let automaton = &analysis.automaton;
            let tables = &analysis.tables;
            println!(
                "lr grammar: {} terminals, {} nonterminals, {} rules, {} start rules",
                grammar.terminal_count(),
                grammar.nonterminal_count(),
                grammar.rules().len(),
                grammar.starts().len()
            );
            println!(
                "automaton: {} states; merge check {:?}; total {:.1?}",
                automaton.states().len(),
                automaton.merge_check(),
                started.elapsed()
            );
            let helper_copies = grammar
                .nonterminals()
                .iter()
                .filter(|nonterminal| matches!(nonterminal.origin, NonterminalOrigin::HelperCopy { .. }))
                .count();
            println!(
                "helper copies: {helper_copies}; reduce/reduce conflicts on $end resolved by declaration order: {}",
                automaton.end_marker_resolutions().len()
            );
            let mut nonterminals_by_origin = [0usize; 6];
            for nonterminal in grammar.nonterminals() {
                let bucket = match nonterminal.origin {
                    NonterminalOrigin::Type(_) => 0,
                    NonterminalOrigin::Start(_) => 1,
                    NonterminalOrigin::Helper => 2,
                    NonterminalOrigin::HelperCopy { .. } => 3,
                    NonterminalOrigin::Restriction { .. } => 4,
                    NonterminalOrigin::DeclaredRestriction { .. } => 5,
                };
                nonterminals_by_origin[bucket] += 1;
            }
            let mut rules_by_origin = [0usize; 6];
            for rule in grammar.rules() {
                let bucket = match grammar.nonterminal(rule.lhs).origin {
                    NonterminalOrigin::Type(_) => 0,
                    NonterminalOrigin::Start(_) => 1,
                    NonterminalOrigin::Helper => 2,
                    NonterminalOrigin::HelperCopy { .. } => 3,
                    NonterminalOrigin::Restriction { .. } => 4,
                    NonterminalOrigin::DeclaredRestriction { .. } => 5,
                };
                rules_by_origin[bucket] += 1;
            }
            println!(
                "nonterminals by origin: type {} start {} helper {} helper-copy {} restriction {} declared-restriction {}; rules: type {} start {} helper {} helper-copy {} restriction {} declared-restriction {}",
                nonterminals_by_origin[0],
                nonterminals_by_origin[1],
                nonterminals_by_origin[2],
                nonterminals_by_origin[3],
                nonterminals_by_origin[4],
                nonterminals_by_origin[5],
                rules_by_origin[0],
                rules_by_origin[1],
                rules_by_origin[2],
                rules_by_origin[3],
                rules_by_origin[4],
                rules_by_origin[5],
            );
            let encoded = tables.encode();
            println!(
                "tables: pact {} table {} check {} defact {} pgoto {} defgoto {} r1 {} r2 {} value_pops {} start_states {} = {} cells, {} encoded bytes",
                tables.pact.len(),
                tables.table.len(),
                tables.check.len(),
                tables.defact.len(),
                tables.pgoto.len(),
                tables.defgoto.len(),
                tables.r1.len(),
                tables.r2.len(),
                tables.value_pops.len(),
                tables.start_states.len(),
                tables.cell_count(),
                [
                    &encoded.pact,
                    &encoded.table,
                    &encoded.check,
                    &encoded.defact,
                    &encoded.pgoto,
                    &encoded.defgoto,
                    &encoded.r1,
                    &encoded.r2,
                    &encoded.value_pops,
                    &encoded.start_states,
                ]
                .iter()
                .map(|table| table.text.len())
                .sum::<usize>()
            );
            let default_only = tables.pact.iter().filter(|base| **base == PACT_NINF).count();
            println!(
                "states acting without a lookahead: {default_only} of {} ({:.1}%)",
                tables.pact.len(),
                100.0 * default_only as f64 / tables.pact.len() as f64
            );
            let conflicts = automaton.conflicts();
            let shift_reduce = conflicts
                .iter()
                .filter(|conflict| matches!(conflict.kind, ConflictKind::ShiftReduce { .. }))
                .count();
            let reduce_reduce = conflicts.len() - shift_reduce;
            let artefacts = conflicts
                .iter()
                .filter(|conflict| conflict.merge_artefact)
                .count();
            let states_with_conflicts = conflicts
                .iter()
                .map(|conflict| conflict.state)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            println!(
                "conflicts: {} total ({shift_reduce} shift/reduce, {reduce_reduce} reduce/reduce, {artefacts} merge artefacts) in {states_with_conflicts} states",
                conflicts.len()
            );

            let out_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("recursa-codegen is in the workspace root")
                .join("target")
                .join("lr-pg-sql");
            std::fs::create_dir_all(&out_dir).expect("create the report directory");
            std::fs::write(out_dir.join("automaton.txt"), analysis.dump())
                .expect("write the automaton dump");
            std::fs::write(out_dir.join("statistics.txt"), analysis.statistics.render())
                .expect("write the structural statistics");
            let mut report = String::new();
            for diagnostic in &analysis.diagnostics {
                report.push_str(&diagnostic.render());
                report.push('\n');
            }
            std::fs::write(out_dir.join("conflicts.txt"), &report).expect("write the conflicts");
            println!("wrote {}", out_dir.display());
        })
        .expect("spawn the construction thread");
    worker.join().expect("the construction thread completes");
}
