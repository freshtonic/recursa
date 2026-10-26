use recursa_generated_rejections::grammar;

/// Renders one parse failure as `kind/code/message/cite/start..end`, or its kind alone.
///
/// A declared rejection carries the rule's whole message data, so the line proves the code, the
/// message, the citation and the position of the item that completed the forbidden pair.
fn describe(error: &recursa::ParseError) -> String {
    match error.rejection() {
        Some(note) => {
            assert_eq!(
                error.code(),
                "RCA4105",
                "a declared rejection reports the runtime code of its own failure class",
            );
            format!(
                "{}/{}/{}/{}/{}..{}",
                error.code(),
                note.code().unwrap_or("-"),
                note.message(),
                note.citation(),
                error.span().start(),
                error.span().end(),
            )
        }
        None => format!("{}/{error}", error.code()),
    }
}

/// Parses one `UNIQUE` constraint and reports `ok` or the failure.
///
/// No wrapper type is named, so one function serves an ordinary parse and an arena parse: both
/// run the same generated list action over the same rule table.
#[cfg(not(feature = "flat-ast"))]
fn unique(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Unique::parse_spanned(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `UNIQUE` constraint into a Flat AST and reports `ok` or the failure.
#[cfg(feature = "flat-ast")]
fn unique(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Unique::parse_flat(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `CHECK` constraint, whose attribute list is a one-or-more repetition.
#[cfg(not(feature = "flat-ast"))]
fn check(source: &str) -> String {
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    match grammar::Check::parse_spanned(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `CHECK` constraint into a Flat AST.
#[cfg(feature = "flat-ast")]
fn check(source: &str) -> String {
    let lexed = grammar::lex(source);
    let mut input = lexed.input();
    match grammar::Check::parse_flat(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `COLUMN` with its option list and reports `ok` or the failure.
#[cfg(not(feature = "flat-ast"))]
fn column(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Column::parse_spanned(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `COLUMN` with its option list into a Flat AST.
#[cfg(feature = "flat-ast")]
fn column(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Column::parse_flat(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `SIZED` Node, whose optional width is a struct with text rules.
#[cfg(not(feature = "flat-ast"))]
fn sized(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Sizing::parse_spanned(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `SIZED` Node into a Flat AST.
#[cfg(feature = "flat-ast")]
fn sized(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Sizing::parse_flat(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `FRAME` Node and reports `ok` or the failure.
#[cfg(not(feature = "flat-ast"))]
fn frame(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Frame::parse_spanned(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `FRAME` Node into a Flat AST.
#[cfg(feature = "flat-ast")]
fn frame(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Frame::parse_flat(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `PAIR` Node and reports `ok` or the failure.
#[cfg(not(feature = "flat-ast"))]
fn pair(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Pair::parse_spanned(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Parses one `PAIR` Node into a Flat AST.
#[cfg(feature = "flat-ast")]
fn pair(source: &str) -> String {
    let lexed = grammar::lex(source);
    assert_eq!(lexed.errors().count(), 0, "fixture source lexes: {source}");
    let mut input = lexed.input();
    match grammar::Pair::parse_flat(&mut input) {
        Ok(_) => "ok".to_owned(),
        Err(error) => describe(&error),
    }
}

/// Samples `FRAME` Nodes and reports whether any of them holds the refused variant.
#[cfg(not(any(feature = "arena-ast", feature = "flat-ast")))]
fn sampled_frames() -> String {
    use recursa::Arbitrary as _;

    let mut sampled = 0_usize;
    for seed in 0_u8..64 {
        let bytes = [seed, seed.wrapping_mul(11), seed ^ 0x2b, seed.wrapping_add(7), seed];
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        let Ok(value) = grammar::Frame::arbitrary(&mut input) else {
            continue;
        };
        if matches!(value.bound, grammar::Bound::Last) {
            return "a sample holds the refused variant".to_owned();
        }
        sampled += 1;
    }
    format!("{} samples, none refused", sampled.min(1))
}

/// A grammar whose AST holds no sampler of its own reports that it sampled nothing.
#[cfg(any(feature = "arena-ast", feature = "flat-ast"))]
fn sampled_frames() -> String {
    "not sampled".to_owned()
}

/// Samples `SIZING` Nodes and reports whether every one of them parses back.
///
/// A sampled width that a rule refuses is sampled again, and an optional width whose retries run
/// out is absent, so every seed gives a value and every value parses back. The digits token
/// samples its one canonical spelling, `0`, which the first rule refuses, so no sample here holds
/// a width: the fixture proves that the refusal neither escapes nor starves the holder.
#[cfg(not(any(feature = "arena-ast", feature = "flat-ast")))]
fn sampled_sized_round_trip() -> String {
    use recursa::Arbitrary as _;
    use recursa::Pretty as _;

    let mut rendered = 0_usize;
    for seed in 0_u8..64 {
        let bytes = [seed, seed.wrapping_mul(29), seed ^ 0x71, seed.wrapping_add(5), seed, 1];
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        let Ok(value) = grammar::Sizing::arbitrary(&mut input) else {
            continue;
        };
        let text = recursa::Parsed::synthetic(value).render();
        let lexed = grammar::lex(text.as_str());
        let mut parse_input = lexed.input();
        match grammar::Sizing::parse_spanned(&mut parse_input) {
            Ok(_) => rendered += 1,
            Err(error) => return format!("{text:?} => {}", describe(&error)),
        }
    }
    format!("{rendered} samples parse back")
}

/// A grammar whose AST holds no sampler of its own reports that it sampled nothing.
#[cfg(any(feature = "arena-ast", feature = "flat-ast"))]
fn sampled_sized_round_trip() -> String {
    "not sampled".to_owned()
}

/// Samples values and reports whether every one of them parses back.
///
/// A sampled repetition drops every item that would complete a forbidden pair, so a rendered
/// sample always parses. The value form is the one that samples the authored type directly; the
/// arena form samples its seed mirror, and a flat grammar has no sampler of its own.
#[cfg(not(any(feature = "arena-ast", feature = "flat-ast")))]
fn sampled_round_trips() -> String {
    use recursa::Arbitrary as _;
    use recursa::Pretty as _;

    let mut rendered = 0_usize;
    for seed in 0_u8..64 {
        let bytes = [seed, seed.wrapping_mul(7), seed ^ 0x5a, seed.wrapping_add(3)];
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        let Ok(value) = grammar::Unique::arbitrary(&mut input) else {
            continue;
        };
        let text = recursa::Parsed::synthetic(value).render();
        let lexed = grammar::lex(text.as_str());
        let mut parse_input = lexed.input();
        match grammar::Unique::parse_spanned(&mut parse_input) {
            Ok(_) => rendered += 1,
            Err(error) => return format!("{text:?} => {}", describe(&error)),
        }
    }
    format!("{rendered} samples parse back")
}

/// Samples columns and reports whether every sample the bytes describe parses back.
///
/// A sampled option list drops every option a rule refuses, and a list whose every option is
/// refused describes no value, so the count is whatever survives; every survivor must parse.
#[cfg(not(any(feature = "arena-ast", feature = "flat-ast")))]
fn sampled_columns_round_trip() -> String {
    use recursa::Arbitrary as _;
    use recursa::Pretty as _;

    let mut rendered = 0_usize;
    for seed in 0_u8..64 {
        let bytes = [seed, seed.wrapping_mul(13), seed ^ 0x3c, seed.wrapping_add(9), seed];
        let mut input = recursa::__private::arbitrary::Unstructured::new(&bytes);
        let Ok(value) = grammar::Column::arbitrary(&mut input) else {
            continue;
        };
        let text = recursa::Parsed::synthetic(value).render();
        let lexed = grammar::lex(text.as_str());
        let mut parse_input = lexed.input();
        match grammar::Column::parse_spanned(&mut parse_input) {
            Ok(_) => rendered += 1,
            Err(error) => return format!("{text:?} => {}", describe(&error)),
        }
    }
    format!("{rendered} samples parse back")
}

/// A grammar whose AST holds no sampler of its own reports that it sampled nothing.
#[cfg(any(feature = "arena-ast", feature = "flat-ast"))]
fn sampled_columns_round_trip() -> String {
    "not sampled".to_owned()
}

/// A grammar whose AST holds no sampler of its own reports that it sampled nothing.
#[cfg(any(feature = "arena-ast", feature = "flat-ast"))]
fn sampled_round_trips() -> String {
    "not sampled".to_owned()
}

fn main() {
    println!("build={}", grammar::Configuration::BUILD.name());

    // A conflict-free list, and a list that repeats one attribute, are both accepted:
    // PostgreSQL admits `DEFERRABLE DEFERRABLE`.
    println!("empty={}", unique("unique (a)"));
    println!("clean={}", unique("unique (a) deferrable initially deferred"));
    println!("repeated={}", unique("unique (a) deferrable deferrable"));

    // The specific rule, in both orders of the same unordered pair. The position is the item
    // that completes the pair, never the list.
    println!(
        "specific={}",
        unique("unique (a) not deferrable initially deferred")
    );
    println!(
        "specific_reversed={}",
        unique("unique (a) initially deferred not deferrable")
    );

    // The general rule, through each of its two pairs.
    println!("general={}", unique("unique (a) deferrable not deferrable"));
    println!(
        "general_second_pair={}",
        unique("unique (a) initially immediate initially deferred")
    );

    // Both rules complete at the third item. The rule written first reports.
    println!(
        "attribute_order={}",
        unique("unique (a) deferrable initially deferred not deferrable")
    );

    // The same rules run over a one-or-more repetition of the same element type.
    println!(
        "one_or_more={}",
        check("check not deferrable deferrable a")
    );
    println!("one_or_more_clean={}", check("check deferrable a"));

    // The gated rule exists only where its variants do.
    println!("gated={}", unique("unique (a) enforced not enforced"));

    // An option list in any order, and the identifier spellings that count as an option.
    println!("column_path={}", column("column a path 'x'"));
    println!("column_quoted_path={}", column("column a \"path\" 'x'"));
    println!(
        "column_any_order={}",
        column("column a not null default 'd' \"path\" 'x'")
    );
    println!("column_legacy_nullability={}", column("column a is_not_null 'x'"));

    // A text rule refuses one option on its own, the first one of a list included.
    println!("column_unknown={}", column("column a foo 'x'"));
    println!("column_unknown_later={}", column("column a null foo 'x'"));

    // `once` refuses a second option of its set, whichever spelling either one has.
    println!(
        "column_two_paths={}",
        column("column a path 'x' \"path\" 'y'")
    );
    println!(
        "column_two_defaults={}",
        column("column a default 'x' \"default\" 'y'")
    );
    println!("column_redundant_null={}", column("column a null null"));
    println!("column_conflicting_null={}", column("column a null not null"));
    println!(
        "column_legacy_conflict={}",
        column("column a is_not_null 'x' null")
    );

    // The gated text rule exists only in the newer build; the older one reports the name as
    // unrecognized.
    println!(
        "reserved={}",
        column("column a \"__pg__is_not_null\" 'x'")
    );

    // A struct's text rules refuse the struct on its own field's folded text, at the extent of
    // that field, as `opt_float` refuses `float(0)` at the `Iconst`.
    println!("sized_bare={}", sized("sized a"));
    println!("sized_width={}", sized("sized a width(8)"));
    println!("sized_leading_zeros={}", sized("sized a width(064)"));
    println!("sized_zero={}", sized("sized a width(0)"));
    println!("sized_wide={}", sized("sized a width(65)"));

    // A refused variant keeps its syntax and is refused when it is reduced, at the extent of the
    // whole variant; the other variants parse.
    println!("frame_first={}", frame("frame first"));
    println!("frame_last={}", frame("frame last"));
    // A validation hook of a struct refuses the value at the field it names; a hook of one
    // variant runs only for that variant.
    println!("pair_two={}", pair("pair (a b)"));
    println!("pair_three={}", pair("pair (a b c)"));
    println!("frame_many_two={}", frame("frame many (a b)"));
    println!("frame_many_three={}", frame("frame many (a b c)"));
    println!("sampled_frames={}", sampled_frames());

    println!("sampled={}", sampled_round_trips());
    println!("sampled_columns={}", sampled_columns_round_trip());
    println!("sampled_sized={}", sampled_sized_round_trip());
}
