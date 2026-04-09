use std::sync::OnceLock;

use recursa::{Input, ParseRules};
use regex::Regex;

pub struct SqlRules;

impl ParseRules for SqlRules {
    const IGNORE: &'static str = "";

    fn ignore_cache() -> &'static OnceLock<Regex> {
        static CACHE: OnceLock<Regex> = OnceLock::new();
        &CACHE
    }

    fn consume_ignored(input: &mut Input) {
        loop {
            let before = input.cursor();
            skip_whitespace(input);
            skip_line_comment(input);
            skip_block_comment(input);
            if input.cursor() == before {
                break;
            }
        }
    }
}

fn skip_whitespace(input: &mut Input) {
    let remaining = input.remaining();
    let count = remaining.len() - remaining.trim_start().len();
    if count > 0 {
        input.advance(count);
    }
}

fn skip_line_comment(input: &mut Input) {
    if input.remaining().starts_with("--") {
        match input.remaining().find('\n') {
            Some(newline) => input.advance(newline + 1),
            None => input.advance(input.remaining().len()),
        }
    }
}

fn skip_block_comment(input: &mut Input) {
    if !input.remaining().starts_with("/*") {
        return;
    }
    let bytes = input.remaining().as_bytes();
    let mut depth: u32 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                input.advance(i);
                return;
            }
        } else {
            i += 1;
        }
    }
    // Unclosed block comment -- advance to end
    input.advance(bytes.len());
}

#[cfg(test)]
mod tests {
    use recursa::{Input, ParseRules};

    use super::SqlRules;

    #[test]
    fn skip_whitespace() {
        let mut input = Input::new("   SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_line_comment() {
        let mut input = Input::new("-- this is a comment\nSELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_block_comment() {
        let mut input = Input::new("/* comment */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_nested_block_comment() {
        let mut input = Input::new("/* outer /* inner */ still outer */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_deeply_nested_block_comment() {
        let mut input = Input::new("/* a /* b /* c */ d */ e */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn skip_mixed_whitespace_and_comments() {
        let mut input = Input::new("  -- line comment\n  /* block */  SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }

    #[test]
    fn no_skip_when_no_ignored() {
        let mut input = Input::new("SELECT 1");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT 1");
    }

    #[test]
    fn skip_line_comment_at_end_of_input() {
        let mut input = Input::new("-- comment without newline");
        SqlRules::consume_ignored(&mut input);
        assert!(input.is_empty());
    }

    #[test]
    fn skip_unclosed_block_comment() {
        let mut input = Input::new("/* unclosed comment");
        SqlRules::consume_ignored(&mut input);
        assert!(input.is_empty());
    }

    #[test]
    fn skip_whitespace_between_comments() {
        let mut input = Input::new("/* a */  /* b */SELECT");
        SqlRules::consume_ignored(&mut input);
        assert_eq!(input.remaining(), "SELECT");
    }
}
