use recursa_diagram::railroad;

pub struct Ident;
pub struct Comma;

#[railroad]
pub struct ArgList {
    pub name: Ident,
    pub comma: Comma,
    pub value: Ident,
}

fn main() {}
