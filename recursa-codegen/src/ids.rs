macro_rules! u32_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            pub const fn new(value: u32) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u32 {
                self.0
            }

            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KindId(u16);

impl KindId {
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

u32_id!(AtomId);
u32_id!(DeclId);
u32_id!(LocationId);
u32_id!(ProofId);
u32_id!(SiteId);
u32_id!(SourceId);
u32_id!(TextPredicateId);
u32_id!(TypeId);
