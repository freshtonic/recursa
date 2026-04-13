//! SQL pretty-printer using derived FormatTokens.

use recursa::fmt::{FormatStyle, PrintEngine};

/// Format an AST node into pretty-printed SQL.
pub fn format_tokens_sql(root: &impl recursa::FormatTokens, style: FormatStyle) -> String {
    let mut tokens = Vec::new();
    root.format_tokens(&mut tokens);
    let engine = PrintEngine::new(style);
    engine.print(&tokens)
}

/// Format a list of parsed commands into SQL text.
pub fn format_commands(commands: &[crate::ast::PsqlCommand], style: FormatStyle) -> String {
    let mut output = String::new();
    for cmd in commands {
        output.push_str(&format_tokens_sql(cmd, style.clone()));
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use recursa::Input;

    use super::*;
    use crate::ast::parse_sql_file;

    fn format(sql: &str) -> String {
        let mut input = Input::new(sql);
        let commands = parse_sql_file(&mut input).unwrap();
        format_tokens_sql(&commands[0], FormatStyle::default())
    }

    #[test]
    fn format_simple_select() {
        let result = format("select 1 as one;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("AS one"), "got: {result}");
    }

    #[test]
    fn format_select_star_from() {
        let result = format("select * from t;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("FROM"), "got: {result}");
        assert!(result.contains("t"), "got: {result}");
    }

    #[test]
    fn format_select_where() {
        let result = format("select a from t where a = 1;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("FROM"), "got: {result}");
        assert!(result.contains("WHERE"), "got: {result}");
    }

    #[test]
    fn format_uppercase_keywords() {
        let result = format("select a from t;");
        assert!(result.contains("SELECT"), "got: {result}");
        assert!(result.contains("FROM"), "got: {result}");
    }
}
