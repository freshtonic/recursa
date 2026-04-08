//! Core traits and types for the recursa parser framework.

mod rules;

pub use rules::{NoRules, ParseRules};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rules_ignore_is_empty() {
        assert_eq!(<NoRules as ParseRules>::IGNORE, "");
    }
}
