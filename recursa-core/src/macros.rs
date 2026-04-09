// Doc comments for these macros live on the re-exports in the `recursa` crate,
// where doc tests can compile because both recursa-core and recursa-derive are available.

#[macro_export]
macro_rules! keywords {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, Debug, Clone)]
            #[scan(pattern = $pattern)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, Debug, Clone)]
        pub enum Keyword {
            $($name($name)),*
        }
    };
}

#[macro_export]
macro_rules! punctuation {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, Debug, Clone)]
            #[scan(pattern = $pattern)]
            pub struct $name;
        )*

        #[derive(::recursa_derive::Scan, Debug, Clone)]
        pub enum Punctuation {
            $($name($name)),*
        }
    };
}

#[macro_export]
macro_rules! literals {
    ($($name:ident => $pattern:literal),* $(,)?) => {
        $(
            #[derive(::recursa_derive::Scan, Debug, Clone)]
            #[scan(pattern = $pattern)]
            pub struct $name<'input>(pub &'input str);
        )*

        #[derive(::recursa_derive::Scan, Debug, Clone)]
        pub enum Literal<'input> {
            $($name($name<'input>)),*
        }
    };
}
