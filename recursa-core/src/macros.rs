/// Declare keyword token types and a combined `Keyword` enum.
///
/// Each entry generates a unit struct with `#[derive(Parse)]` and the
/// specified pattern. Keywords are case-insensitive and invisible to visitors.
#[macro_export]
macro_rules! keywords {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[parse(pattern = $pattern, case_insensitive)]
            #[visit(ignore)]
            pub struct $name;

            impl $crate::fmt::TokenText for $name {
                const TEXT: &'static str = $crate::fmt::strip_word_boundary($pattern);
            }

            impl $crate::fmt::FormatTokens for $name {
                fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                    tokens.push($crate::fmt::Token::String(
                        <Self as $crate::fmt::TokenText>::TEXT.to_string()
                    ));
                }
            }
        )*

        #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[visit(ignore)]
        pub enum Keyword {
            $($name($name)),*
        }

        impl $crate::fmt::FormatTokens for Keyword {
            fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                match self {
                    $(Keyword::$name(inner) => inner.format_tokens(tokens),)*
                }
            }
        }
    };
}

/// Declare punctuation token types and a combined `Punctuation` enum.
#[macro_export]
macro_rules! punctuation {
    ($($name:ident => $pattern:literal, $display:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[parse(pattern = $pattern)]
            #[visit(ignore)]
            pub struct $name;

            impl $crate::fmt::TokenText for $name {
                const TEXT: &'static str = $display;
            }

            impl $crate::fmt::FormatTokens for $name {
                fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                    tokens.push($crate::fmt::Token::String(
                        <Self as $crate::fmt::TokenText>::TEXT.to_string()
                    ));
                }
            }
        )*

        #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[visit(ignore)]
        pub enum Punctuation {
            $($name($name)),*
        }

        impl $crate::fmt::FormatTokens for Punctuation {
            fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                match self {
                    $(Punctuation::$name(inner) => inner.format_tokens(tokens),)*
                }
            }
        }
    };
}

/// Declare literal/capturing token types and a combined `Literal` enum.
#[macro_export]
macro_rules! literals {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[parse(pattern = $pattern)]
            #[visit(terminal)]
            pub struct $name(pub String);

            impl $crate::fmt::FormatTokens for $name {
                fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                    tokens.push($crate::fmt::Token::String(self.0.clone()));
                }
            }
        )*

        #[derive(::recursa_derive::Parse, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[visit(terminal)]
        pub enum Literal {
            $($name($name)),*
        }

        impl $crate::fmt::FormatTokens for Literal {
            fn format_tokens(&self, tokens: &mut Vec<$crate::fmt::Token>) {
                match self {
                    $(Literal::$name(inner) => inner.format_tokens(tokens),)*
                }
            }
        }
    };
}
