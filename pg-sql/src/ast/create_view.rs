/// CREATE VIEW and DROP VIEW statement AST.
///
/// `CREATE [OR REPLACE] [TEMP|TEMPORARY] [RECURSIVE] VIEW name [(cols)] AS query`
/// `DROP VIEW [IF EXISTS] name`
use std::marker::PhantomData;

use recursa::seq::Seq;
use recursa::surrounded::Surrounded;
use recursa::{Input, Parse, ParseError, ParseRules, Visit};

use crate::ast::values::CompoundQuery;
use crate::tokens::{keyword, literal, punct};

/// CREATE VIEW statement.
///
/// Manual Parse impl needed because the many optional keywords (OR REPLACE,
/// TEMP/TEMPORARY, RECURSIVE) create complex disambiguation before the VIEW keyword.
/// To eliminate this, recursa would need multi-keyword optional prefix chains.
/// Manual Visit impl needed because `bool` fields don't implement Visit.
/// To eliminate this, recursa would need `#[visit(skip)]` field attribute support.
#[derive(Debug, Clone)]
pub struct CreateViewStmt {
    pub or_replace: bool,
    pub temp: bool,
    pub recursive: bool,
    pub name: literal::Ident,
    pub columns:
        Option<Surrounded<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>>,
    pub query: CompoundQuery,
}

impl recursa::visitor::AsNodeKey for CreateViewStmt {}

impl Visit for CreateViewStmt {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for CreateViewStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Long pattern to win over CREATE TABLE/FUNCTION/INDEX in enum disambiguation.
        // Matches CREATE [OR REPLACE] [TEMP|TEMPORARY] [RECURSIVE] VIEW.
        static PATTERN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        PATTERN.get_or_init(|| {
            r"(?i:CREATE\b)(?:\s+(?i:OR\b)\s+(?i:REPLACE\b))?(?:\s+(?i:TEMP\b|\bTEMPORARY\b))?(?:\s+(?i:RECURSIVE\b))?\s+(?i:VIEW\b)".to_string()
        })
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            regex::Regex::new(&format!(r"\A(?:{})", Self::first_pattern())).unwrap()
        });
        re.is_match(input.remaining())
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        PhantomData::<keyword::Create>::parse(input, rules)?;
        R::consume_ignored(input);

        // Optional OR REPLACE
        let or_replace = if keyword::Or::peek(input, rules) {
            PhantomData::<keyword::Or>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Replace>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else {
            false
        };

        // Optional TEMP / TEMPORARY
        let temp = if keyword::Temp::peek(input, rules) {
            PhantomData::<keyword::Temp>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else if keyword::Temporary::peek(input, rules) {
            PhantomData::<keyword::Temporary>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else {
            false
        };

        // Optional RECURSIVE
        let recursive = if keyword::Recursive::peek(input, rules) {
            PhantomData::<keyword::Recursive>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else {
            false
        };

        PhantomData::<keyword::View>::parse(input, rules)?;
        R::consume_ignored(input);

        let name = literal::Ident::parse(input, rules)?;
        R::consume_ignored(input);

        // Optional column list
        let columns = if punct::LParen::peek(input, rules) {
            // Check if this is a column list (identifiers) before AS
            let mut fork = input.fork();
            match Surrounded::<punct::LParen, Seq<literal::AliasName, punct::Comma>, punct::RParen>::parse(&mut fork, rules) {
                Ok(cols) => {
                    R::consume_ignored(&mut fork);
                    if keyword::As::peek(&fork, rules) {
                        input.advance(fork.cursor() - input.cursor());
                        R::consume_ignored(input);
                        Some(cols)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        };

        PhantomData::<keyword::As>::parse(input, rules)?;
        R::consume_ignored(input);

        let query = CompoundQuery::parse(input, rules)?;

        Ok(CreateViewStmt {
            or_replace,
            temp,
            recursive,
            name,
            columns,
            query,
        })
    }
}

/// DROP VIEW statement: `DROP VIEW [IF EXISTS] name`
///
/// Manual Parse impl needed because DROP VIEW must be distinguished from
/// DROP TABLE / DROP FUNCTION / DROP INDEX via forward lookahead.
/// To eliminate this, recursa would need multi-keyword compound first patterns.
/// Manual Visit impl needed because `bool` field doesn't implement Visit.
/// To eliminate this, recursa would need `#[visit(skip)]` field attribute support.
#[derive(Debug, Clone)]
pub struct DropViewStmt {
    pub if_exists: bool,
    pub name: literal::Ident,
}

impl recursa::visitor::AsNodeKey for DropViewStmt {}

impl Visit for DropViewStmt {
    fn visit<V: recursa::visitor::TotalVisitor>(
        &self,
        _visitor: &mut V,
    ) -> std::ops::ControlFlow<recursa::visitor::Break<V::Error>> {
        std::ops::ControlFlow::Continue(())
    }
}

impl<'input> Parse<'input> for DropViewStmt {
    const IS_TERMINAL: bool = false;

    fn first_pattern() -> &'static str {
        // Long pattern to win over DROP TABLE/FUNCTION/INDEX in enum disambiguation.
        static PATTERN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        PATTERN.get_or_init(|| r"(?i:DROP\b)\s+(?i:VIEW\b)".to_string())
    }

    fn peek<R: ParseRules>(input: &Input<'input>, _rules: &R) -> bool {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            regex::Regex::new(&format!(r"\A(?:{})", Self::first_pattern())).unwrap()
        });
        re.is_match(input.remaining())
    }

    fn parse<R: ParseRules>(input: &mut Input<'input>, rules: &R) -> Result<Self, ParseError> {
        PhantomData::<keyword::Drop>::parse(input, rules)?;
        R::consume_ignored(input);
        PhantomData::<keyword::View>::parse(input, rules)?;
        R::consume_ignored(input);

        let if_exists = if keyword::If::peek(input, rules) {
            PhantomData::<keyword::If>::parse(input, rules)?;
            R::consume_ignored(input);
            PhantomData::<keyword::Exists>::parse(input, rules)?;
            R::consume_ignored(input);
            true
        } else {
            false
        };

        let name = literal::Ident::parse(input, rules)?;

        Ok(DropViewStmt { if_exists, name })
    }
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::rules::SqlRules;

    use super::*;

    #[test]
    fn parse_create_view() {
        let mut input = Input::new("CREATE VIEW v AS SELECT 1");
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "v");
        assert!(!stmt.or_replace);
        assert!(!stmt.temp);
        assert!(!stmt.recursive);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_temp_view() {
        let mut input = Input::new("CREATE TEMPORARY VIEW v AS SELECT 1");
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.temp);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_recursive_view() {
        let mut input = Input::new(
            "CREATE RECURSIVE VIEW nums (n) AS VALUES (1) UNION ALL SELECT n+1 FROM nums WHERE n < 5",
        );
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.recursive);
        assert!(stmt.columns.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_create_or_replace_recursive_view() {
        let mut input = Input::new(
            "CREATE OR REPLACE RECURSIVE VIEW nums (n) AS VALUES (1) UNION ALL SELECT n+1 FROM nums WHERE n < 6",
        );
        let stmt = CreateViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert!(stmt.or_replace);
        assert!(stmt.recursive);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_drop_view() {
        let mut input = Input::new("DROP VIEW v");
        let stmt = DropViewStmt::parse(&mut input, &SqlRules).unwrap();
        assert_eq!(stmt.name.0, "v");
        assert!(!stmt.if_exists);
        assert!(input.is_empty());
    }
}
