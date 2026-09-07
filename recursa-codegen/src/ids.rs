//! Compact typed identities for normalized grammar arenas.

#![deny(missing_docs, clippy::missing_docs_in_private_items)]

/// Defines a distinct checked-by-type `u32` arena identity.
macro_rules! u32_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[doc = concat!("Typed arena identity for `", stringify!($name), "` values.")]
        pub struct $name(u32);

        impl $name {
            /// Creates an identity from its canonical arena offset.
            pub const fn new(value: u32) -> Self {
                Self(value)
            }

            /// Returns the stored numeric identity.
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Returns the identity as a collection index.
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Grammar-local token-kind identity.
pub struct KindId(u16);

impl KindId {
    /// Creates a token-kind identity from its canonical numeric value.
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the stored numeric identity.
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Returns the identity as a collection index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

u32_id!(AtomId);
u32_id!(DeclId);
u32_id!(LocationId);
u32_id!(PrecedenceLevelId);
u32_id!(SiteId);
u32_id!(SourceId);
u32_id!(TextPredicateId);
u32_id!(TypeId);
