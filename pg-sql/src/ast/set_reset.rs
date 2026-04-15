/// SET/RESET statement AST.
use recursa::seq::Seq;
use recursa::{FormatTokens, Parse, Visit};

use crate::rules::SqlRules;
use crate::tokens::{literal, punct};
use crate::tokens::keyword::*;
use recursa_diagram::railroad;

/// Scope of a SET statement: `SESSION` or `LOCAL`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetScope {
    Session(SESSION),
    Local(LOCAL),
}

/// The value in a SET statement: literal, keyword, or identifier.
///
/// Variant ordering: NumericLit before IntegerLit so `77.7` is consumed as a
/// numeric literal (longest-match-wins).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetValue<'input> {
    On(ON),
    Off(OFF),
    False(FALSE),
    True(TRUE),
    Default(DEFAULT),
    StringLit(literal::StringLit<'input>),
    SignedNumeric(SignedNumericLit<'input>),
    NumericLit(literal::NumericLit<'input>),
    IntegerLit(literal::IntegerLit<'input>),
    Ident(literal::Ident<'input>),
}

/// A numeric literal with an optional leading sign: `-1`, `+1.5`, `2`.
///
/// Used in positions like `SET extra_float_digits = -1` where a full `Expr`
/// is overkill and would admit keywords that shouldn't be legal values.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SignedNumericLit<'input> {
    pub sign: NumericSign,
    pub value: UnsignedNumericLit<'input>,
}

/// Leading `-` or `+` sign of a signed numeric literal.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum NumericSign {
    Neg(punct::Minus),
    Pos(punct::Plus),
}

/// Either a numeric (with decimal point / exponent) or an integer literal.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum UnsignedNumericLit<'input> {
    Numeric(literal::NumericLit<'input>),
    Integer(literal::IntegerLit<'input>),
}

/// The separator between param and value: TO or =.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetSep {
    To(TO),
    Eq(punct::Eq),
}

/// Plain SET statement: `SET [SESSION|LOCAL] param TO|= value [, value ...]`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetStmt<'input> {
    pub _set: SET,
    pub scope: Option<SetScope>,
    pub param: literal::AliasName<'input>,
    pub sep: SetSep,
    pub values: Seq<SetValue<'input>, punct::Comma>,
}

/// Role target in `SET ROLE`: role name, `NONE`, or `DEFAULT`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetRoleTarget<'input> {
    None(NONE),
    Default(DEFAULT),
    Role(literal::AliasName<'input>),
    String(literal::StringLit<'input>),
}

/// `SET [SESSION|LOCAL] ROLE { rolename | NONE | DEFAULT }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetRoleStmt<'input> {
    pub _set: SET,
    pub scope: Option<SetScope>,
    pub _role: ROLE,
    pub target: SetRoleTarget<'input>,
}

/// Role target in `SET SESSION AUTHORIZATION`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetSessionAuthTarget<'input> {
    Default(DEFAULT),
    String(literal::StringLit<'input>),
    Role(literal::AliasName<'input>),
}

/// `SET [SESSION|LOCAL] SESSION AUTHORIZATION { rolename | DEFAULT }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetSessionAuthStmt<'input> {
    pub _set: SET,
    // Only `LOCAL` is allowed here — the `SESSION` scope keyword would
    // conflict with the `SESSION AUTHORIZATION` literal that follows.
    pub local: Option<LOCAL>,
    pub _session: SESSION,
    pub _authorization: AUTHORIZATION,
    pub target: SetSessionAuthTarget<'input>,
}

/// A signed numeric literal: `[-]numeric | [-]integer`.
///
/// Variant ordering: Numeric before Integer (longest-match-wins for `7.5`).
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SignedNumber<'input> {
    Numeric(SignedNumeric<'input>),
    Integer(SignedInteger<'input>),
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SignedNumeric<'input> {
    pub minus: Option<punct::Minus>,
    pub value: literal::NumericLit<'input>,
}

#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SignedInteger<'input> {
    pub minus: Option<punct::Minus>,
    pub value: literal::IntegerLit<'input>,
}

/// Target of `SET TIME ZONE`.
///
/// Variant ordering: `LOCAL` and `DEFAULT` (keywords) before `Number` and
/// `String`. INTERVAL form is deliberately skipped.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum SetTimeZoneTarget<'input> {
    Local(LOCAL),
    Default(DEFAULT),
    Number(SignedNumber<'input>),
    String(literal::StringLit<'input>),
}

/// `SET [SESSION|LOCAL] TIME ZONE { signed_number | string | LOCAL | DEFAULT }`
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct SetTimeZoneStmt<'input> {
    pub _set: SET,
    pub scope: Option<SetScope>,
    pub _time: TIME,
    pub _zone: ZONE,
    pub target: SetTimeZoneTarget<'input>,
}

/// Target of a RESET statement.
///
/// Variant ordering: multi-token variants before single-token variants.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ResetTarget<'input> {
    SessionAuth((SESSION, AUTHORIZATION)),
    TimeZone((TIME, ZONE)),
    Role(ROLE),
    All(ALL),
    Ident(literal::AliasName<'input>),
}

/// RESET statement: `RESET { param | ALL | ROLE | SESSION AUTHORIZATION | TIME ZONE }`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ResetStmt<'input> {
    pub _reset: RESET,
    pub target: ResetTarget<'input>,
}

// --- SHOW ---

/// Target of a SHOW statement.
///
/// Variant ordering: multi-token targets before single-token `Param`
/// fallback so the specific forms are matched first.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub enum ShowTarget<'input> {
    TransactionIsolationLevel((TRANSACTION, ISOLATION, LEVEL)),
    SessionAuthorization((SESSION, AUTHORIZATION)),
    TimeZone((TIME, ZONE)),
    All(ALL),
    Param(literal::AliasName<'input>),
}

/// SHOW statement: `SHOW { name | ALL | TIME ZONE | SESSION AUTHORIZATION | TRANSACTION ISOLATION LEVEL }`.
#[railroad]
#[derive(Debug, Clone, FormatTokens, Parse, Visit)]
#[parse(rules = SqlRules)]
pub struct ShowStmt<'input> {
    pub _show: SHOW,
    pub target: ShowTarget<'input>,
}

#[cfg(test)]
mod tests {
    use recursa::{Input, Parse};

    use crate::ast::set_reset::{
        ResetStmt, SetRoleStmt, SetSessionAuthStmt, SetStmt, SetTimeZoneStmt, ShowStmt,
    };
    use crate::rules::SqlRules;

    #[test]
    fn parse_set_to() {
        let mut input = Input::new("SET enable_seqscan TO off");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.param.text(), "enable_seqscan");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_eq() {
        let mut input = Input::new("SET enable_sort = false");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.param.text(), "enable_sort");
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_integer_value() {
        let mut input = Input::new("SET work_mem = 4096");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.values.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_numeric_value() {
        let mut input = Input::new("SET seq_page_cost = 1.5");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.values.len(), 1);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_multi_value() {
        let mut input = Input::new("SET search_path TO public, pg_catalog");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert_eq!(stmt.values.len(), 2);
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_session_scope() {
        let mut input = Input::new("SET SESSION enable_seqscan TO off");
        let stmt = SetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(stmt.scope.is_some());
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reset() {
        let mut input = Input::new("RESET enable_seqscan");
        let stmt = ResetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
        let _ = stmt;
    }

    #[test]
    fn parse_reset_all() {
        let mut input = Input::new("RESET ALL");
        let _stmt = ResetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reset_role() {
        let mut input = Input::new("RESET ROLE");
        let _stmt = ResetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reset_session_authorization() {
        let mut input = Input::new("RESET SESSION AUTHORIZATION");
        let _stmt = ResetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_reset_time_zone() {
        let mut input = Input::new("RESET TIME ZONE");
        let _stmt = ResetStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_role_default() {
        let mut input = Input::new("SET ROLE DEFAULT");
        let _stmt = SetRoleStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_role_none() {
        let mut input = Input::new("SET ROLE NONE");
        let _stmt = SetRoleStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_role_name() {
        let mut input = Input::new("SET ROLE alice");
        let _stmt = SetRoleStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_local_role() {
        let mut input = Input::new("SET LOCAL ROLE alice");
        let _stmt = SetRoleStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_session_authorization_default() {
        let mut input = Input::new("SET SESSION AUTHORIZATION DEFAULT");
        let _stmt = SetSessionAuthStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_session_authorization_string() {
        let mut input = Input::new("SET SESSION AUTHORIZATION 'alice'");
        let _stmt = SetSessionAuthStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_time_zone_string() {
        let mut input = Input::new("SET TIME ZONE 'UTC'");
        let _stmt = SetTimeZoneStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_time_zone_negative() {
        let mut input = Input::new("SET TIME ZONE -8");
        let _stmt = SetTimeZoneStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_time_zone_default() {
        let mut input = Input::new("SET TIME ZONE DEFAULT");
        let _stmt = SetTimeZoneStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_show_param() {
        let mut input = Input::new("SHOW TimeZone");
        let _stmt = ShowStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_show_ident() {
        let mut input = Input::new("SHOW transaction_read_only");
        let _stmt = ShowStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_show_all() {
        let mut input = Input::new("SHOW ALL");
        let _stmt = ShowStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_show_time_zone() {
        let mut input = Input::new("SHOW TIME ZONE");
        let _stmt = ShowStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_show_transaction_isolation_level() {
        let mut input = Input::new("SHOW TRANSACTION ISOLATION LEVEL");
        let _stmt = ShowStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn parse_set_time_zone_local() {
        let mut input = Input::new("SET TIME ZONE LOCAL");
        let _stmt = SetTimeZoneStmt::parse::<SqlRules>(&mut input).unwrap();
        assert!(input.is_empty());
    }
}
