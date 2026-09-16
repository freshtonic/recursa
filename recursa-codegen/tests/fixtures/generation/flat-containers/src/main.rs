//! Compile-checks the generated Flat AST surface and prints one size line per flat type.
//!
//! Step two of the tiered flat AST milestone generates the types, the owner, and the layout
//! assertions; flat reduce actions and `parse_flat` arrive in step three, so there is no parse to
//! drive here. Every line this binary prints is a compile-time fact about the generated layout.

use recursa_generated_flat_containers::grammar;

/// Prints one flat type's size, and its name, so the sizes are diffable.
macro_rules! sizes {
    ($($ty:ty),* $(,)?) => {
        $(println!("lr size {} = {}", stringify!($ty), std::mem::size_of::<$ty>());)*
    };
}

fn main() {
    // Every declaration of the grammar has a flat type in the same module.
    sizes!(
        grammar::FlatStatement,
        grammar::FlatLet,
        grammar::FlatAlias,
        grammar::FlatPrint,
        grammar::FlatGuard,
        grammar::FlatCall,
        grammar::FlatArgs,
        grammar::FlatFunctionTail,
        grammar::FlatTrailingVariadic,
        grammar::FlatDefine,
        grammar::FlatParams,
        grammar::FlatParam,
        grammar::FlatMode,
        grammar::FlatWith,
        grammar::FlatBlock,
        grammar::FlatModifiers,
        grammar::FlatErased,
        grammar::FlatOptions,
        grammar::FlatKwAs,
        grammar::FlatKwIn,
        grammar::FlatKwOut,
        grammar::FlatKwOnly,
        grammar::FlatKwTemp,
        grammar::FlatTrail,
        grammar::FlatBoxes,
        grammar::FlatMarks,
        grammar::FlatFilter,
        grammar::FlatValue,
        grammar::FlatList,
        grammar::FlatTuple,
        grammar::FlatBang,
        grammar::FlatName,
        grammar::FlatCallee,
        grammar::FlatNumber,
    );
    sizes!(grammar::FlatAst<'static, grammar::FlatStatement>);

    // A flat Node is `Copy`, so a handle-shaped child never moves out of its parent.
    let copied = <grammar::FlatBang as Clone>::clone(&grammar::FlatBang);
    let _ = copied;
    assert_eq!(grammar::FlatBang, copied);

    // Every flat Node carries a grammar-unique tag, which the shared pool checks on each read.
    println!(
        "lr tags statement={} value={} name={}",
        <grammar::FlatStatement as recursa::FlatNode>::TAG,
        <grammar::FlatValue as recursa::FlatNode>::TAG,
        <grammar::FlatName as recursa::FlatNode>::TAG,
    );

    // The owner is `Send + Sync` for every root Node type, proved without instantiating one.
    grammar::__recursa_assert_flat_ast_send_sync::<grammar::FlatStatement>();

    // The design's four- and eight-byte handle promises.
    println!(
        "lr optional handle={} optional list={}",
        std::mem::size_of::<Option<recursa::Handle<grammar::FlatValue>>>(),
        std::mem::size_of::<Option<recursa::List<recursa::Handle<grammar::FlatValue>>>>(),
    );

    println!("flat-containers-ok");
}
