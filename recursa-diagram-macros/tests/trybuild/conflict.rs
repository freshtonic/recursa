use recursa_diagram::railroad;

#[railroad]
pub struct S {
    #[railroad(label = "X", skip)]
    f: Foo,
}

pub struct Foo;

fn main() {}
