use recursa::Requires as _;
use recursa_generated_requirements::grammar;
use recursa_generated_requirements::grammar::Configuration;

/// Renders one requirement as `name@start..end/code/site`, or `-` when there is none.
fn describe(requirement: Option<recursa::Requirement>) -> String {
    let Some(requirement) = requirement else {
        return "-".to_owned();
    };
    let note = requirement.note();
    let span = requirement.span().map_or_else(
        || "?".to_owned(),
        |span| format!("{}..{}", span.start(), span.end()),
    );
    format!(
        "{}@{span}/{}/{}",
        note.configuration_name(),
        note.code().unwrap_or("-"),
        note.site(),
    )
}

/// Parses one source and reports its minimum, then the first requirement above `floor`.
///
/// No wrapper type is named here, so this function is the same for an ordinary parse and for
/// an arena parse: both wrappers answer the two questions with the same two methods, and both
/// keep the span, because both scan from their own root occurrence cursor.
#[cfg(not(feature = "flat-ast"))]
fn report(source: &str, floor: Configuration) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    let parsed = grammar::Query::parse_spanned(&mut input).expect("fixture source parses");
    assert!(input.is_eof(), "fixture source is complete: {source}");
    format!(
        "{} | {}",
        describe(parsed.minimum_requirement()),
        describe(parsed.requirement_above(floor.id())),
    )
}

/// Reports the same two answers from a Flat AST, which holds no nested value at all.
///
/// The owner answers with the same two methods. The walk reads the pools, and each extent is the
/// one the parse retained for the Node, the text, or the run.
#[cfg(feature = "flat-ast")]
fn report(source: &str, floor: Configuration) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    let parsed = grammar::Query::parse_flat(&mut input).expect("fixture source parses");
    assert!(input.is_eof(), "fixture source is complete: {source}");
    assert!(parsed.has_spans(), "a flat parse of the fixture keeps spans");
    format!(
        "{} | {}",
        describe(parsed.minimum_requirement()),
        describe(parsed.requirement_above(floor.id())),
    )
}

/// Reports the minimum of the detached value, which answers the same question with no span.
///
/// This is the one place the AST storages differ: an ordinary parse gives up its AST with
/// [`recursa::Parsed::into_ast`], while an arena parse lends it with
/// [`recursa::ArenaParsed::ast`]. A `flat` grammar still parses into a nested value, so the flat
/// build reports this line from such a parse.
fn detached(source: &str) -> String {
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    let parsed = grammar::Query::parse_spanned(&mut input).expect("fixture source parses");
    #[cfg(not(feature = "arena-ast"))]
    let minimum = parsed.into_ast().minimum_requirement();
    #[cfg(feature = "arena-ast")]
    let minimum = parsed.ast().minimum_requirement();
    describe(minimum)
}

const MISSING_ALIAS: &str = "select * from (select * from t)";
const NEARBY: &str = "select * from (select * from t) where nearby x";
const NEARBY_SECOND: &str = "select * from t where x, nearby y";

fn main() {
    println!("build={}", Configuration::BUILD.name());
    println!(
        "scale={}",
        Configuration::ALL
            .iter()
            .map(|configuration| configuration.name())
            .collect::<Vec<_>>()
            .join(",")
    );

    // Nothing in this statement needs more than the baseline.
    println!("plain={}", report("select * from t", Configuration::V1));
    println!(
        "with_alias={}",
        report("select * from (select * from t) as u", Configuration::V1)
    );

    // From v2 the alias is optional, so its absence is what needs v2.
    if Configuration::BUILD >= Configuration::V2 {
        println!("missing_alias={}", report(MISSING_ALIAS, Configuration::V1));
        println!("detached={}", detached(MISSING_ALIAS));
    }

    // A v3 variant outranks the v2 absence, but the absence is still the first construct
    // above the baseline in source order.
    if Configuration::BUILD >= Configuration::V3 {
        println!("nearby={}", report(NEARBY, Configuration::V1));
        println!("nearby_above_v2={}", report(NEARBY, Configuration::V2));
        // The gated variant is the second item of the filter repetition, so its span comes
        // from the item ordinal, not from the repetition itself.
        println!(
            "nearby_second={}",
            report(NEARBY_SECOND, Configuration::V1)
        );
        // A repetition field records once, at the element, when it holds a value.
        println!(
            "columns={}",
            report(
                "select * from (select * from t) as u, a, b",
                Configuration::V1
            )
        );
    }
}
