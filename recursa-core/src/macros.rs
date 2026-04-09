/// Declare keyword token types and a combined `Keyword` enum.
///
/// Each entry generates a unit struct with `#[derive(Scan)]` and the
/// specified pattern. A combined `Keyword` enum is also generated
/// with all variants.
///
/// # Example
///
/// ```text
/// recursa::keywords! {
///     Let   => "let",
///     While => "while",
///     If    => "if",
/// }
/// ```
///
/// Expands to unit structs `Let`, `While`, `If` (each implementing `Scan`)
/// plus an enum `Keyword` with variants `Keyword::Let(Let)`, etc.
#[macro_export]
macro_rules! keywords {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[scan(pattern = $pattern, case_insensitive)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Keyword {
            $($name($name)),*
        }
    };
}

/// Declare punctuation token types and a combined `Punctuation` enum.
///
/// Each entry generates a unit struct with `#[derive(Scan)]` and the
/// specified pattern. A combined `Punctuation` enum is also generated
/// with all variants.
///
/// Patterns must be valid regex. For literal punctuation characters that
/// are regex metacharacters, provide already-escaped patterns
/// (e.g., `r"\+"` not `"+"`).
///
/// # Example
///
/// ```text
/// recursa::punctuation! {
///     Plus   => r"\+",
///     LParen => r"\(",
/// }
/// ```
#[macro_export]
macro_rules! punctuation {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[scan(pattern = $pattern)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Punctuation {
            $($name($name)),*
        }
    };
}

/// Declare literal/capturing token types and a combined `Literal` enum.
///
/// Each entry generates a tuple struct wrapping `String` with
/// `#[derive(Scan)]` and the specified pattern. A combined `Literal`
/// enum is also generated with all variants.
///
/// # Example
///
/// ```text
/// recursa::literals! {
///     IntLiteral => r"[0-9]+",
///     Ident      => r"[a-zA-Z_][a-zA-Z0-9_]*",
/// }
/// ```
#[macro_export]
macro_rules! literals {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[scan(pattern = $pattern)]
            pub struct $name(pub String);
        )*

        #[derive(::recursa_derive::Scan, ::recursa_derive::Visit, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Literal {
            $($name($name)),*
        }
    };
}
