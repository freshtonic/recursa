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

/// Lexes and parses one complete query through the generated public API.
fn parse(source: &str) -> recursa::Parsed<'_, grammar::Query<'_>> {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    let parsed = grammar::Query::parse_spanned(&mut input).expect("fixture source parses");
    assert!(input.is_eof(), "fixture source is complete: {source}");
    parsed
}

/// Parses one source and reports its minimum, then the first requirement above `floor`.
fn report(source: &str, floor: Configuration) -> String {
    let parsed = parse(source);
    format!(
        "{} | {}",
        describe(parsed.minimum_requirement()),
        describe(parsed.requirement_above(floor.id())),
    )
}

const MISSING_ALIAS: &str = "select * from (select * from t)";
const NEARBY: &str = "select * from (select * from t) where nearby x";

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
        let detached = parse(MISSING_ALIAS).into_ast();
        println!("detached={}", describe(detached.minimum_requirement()));
    }

    // A v3 variant outranks the v2 absence, but the absence is still the first construct
    // above the baseline in source order.
    if Configuration::BUILD >= Configuration::V3 {
        println!("nearby={}", report(NEARBY, Configuration::V1));
        println!("nearby_above_v2={}", report(NEARBY, Configuration::V2));
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
