//! Grammar-erased traversal of a Flat AST: the children contract a generated owner implements,
//! and the depth-first walk built on it.
//!
//! [`crate::flat`] holds the stores and [`crate::flat_owner`] reads one Node out of them. This
//! module adds the layer above them. A generated owner reports the direct children of one Node
//! through [`crate::flat_visit::FlatChildren`]. [`crate::flat_visit::walk`] then drives a
//! [`crate::flat_visit::FlatVisitor`] over the tree those children make. Neither trait names a
//! grammar type, so one walk serves every grammar and an adapter can reuse it.
//!
//! The traversal value is the grammar's own `NodeRef`, which Recursa generates. It is an enum with
//! one variant per Node application, and each variant holds that application's Node handle. A text
//! handle and a presence flag are data, not children, so neither one appears in it.
//!
//! A grammar root cannot name a Node whose declaration is not public, so it cannot give that Node
//! a variant of its own. Such a Node reaches a traversal as an [`crate::flat_visit::OpaqueRef`],
//! which carries the Node's [`crate::flat_visit::OpaqueTable`]. The table reports the Node's name
//! and the Node's children, so the walk continues through the Node.
//!
//! `docs/flat-ast-design.md` settles the traversal API and the post-order promise that
//! [`crate::flat_visit::PostOrderViolation`] reports.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::num::NonZeroU32;
use core::ptr;

use crate::flat::{ListStore, SharedPool};

/// The traversal behavior of one Node type a grammar root cannot name.
///
/// A grammar's `NodeRef` holds one handle per Node application. It cannot hold the handle of a
/// Node whose declaration is not public, because a public enum cannot name a private type. Such a
/// Node reaches a traversal as an [`OpaqueRef`], which carries this table.
///
/// Recursa emits one table per such application, as a `static` in the declaration's own module.
/// That module can name the Node, so it can also read it. The table therefore makes the Node's
/// name and the Node's children available to a grammar root that cannot name the Node at all.
///
/// `N` is the grammar's own `NodeRef`.
pub struct OpaqueTable<N> {
    /// The name of the Node application, which [`OpaqueRef::name`] returns.
    name: &'static str,
    /// Reports the direct children of one Node of this application.
    ///
    /// The Node is always in the shared pool, because a pooled declaration is always publicly
    /// reachable. The function therefore needs the shared pool, the list store, and the raw
    /// handle value, and it needs nothing that names the owner's root Node type.
    children: fn(&SharedPool, &ListStore, NonZeroU32, &mut dyn FnMut(N)),
}

impl<N> OpaqueTable<N> {
    /// Records the name and the children function of one Node application.
    ///
    /// Generated code calls this in a `static` initializer beside the declaration.
    pub const fn new(
        name: &'static str,
        children: fn(&SharedPool, &ListStore, NonZeroU32, &mut dyn FnMut(N)),
    ) -> Self {
        Self { name, children }
    }

    /// Returns the name of the Node application.
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

impl<N> fmt::Debug for OpaqueTable<N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueTable")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// One Node of a Flat AST that the grammar root cannot name, as a traversal value.
///
/// The reference carries the raw handle value and the Node's [`OpaqueTable`]. The table supplies
/// the Node's name and its children, so a traversal continues through the Node instead of
/// stopping at it. A grammar's `NodeRef` holds this value in its `Opaque` variant.
///
/// The value is four bytes of handle plus one reference. It is `Copy`, like every other traversal
/// value.
pub struct OpaqueRef<N: 'static> {
    /// The raw handle value of the Node in the shared pool.
    raw: NonZeroU32,
    /// The traversal behavior of the Node's own application.
    table: &'static OpaqueTable<N>,
}

impl<N> OpaqueRef<N> {
    /// Names the Node at `raw` through `table`.
    ///
    /// Generated code calls this in the parent's own module. The parent can name its child's
    /// table, even when the grammar root cannot name the child's type.
    pub const fn new(raw: NonZeroU32, table: &'static OpaqueTable<N>) -> Self {
        Self { raw, table }
    }

    /// Returns the name of the Node application this reference names.
    pub const fn name(self) -> &'static str {
        self.table.name
    }

    /// Returns the raw handle value of the Node.
    ///
    /// The value identifies the Node inside the shared pool. It is not a handle a consumer can
    /// use, because only the Node's own module can name its type.
    pub const fn raw(self) -> NonZeroU32 {
        self.raw
    }

    /// Calls `f` once per direct child of the Node, in source order.
    ///
    /// The generated `children` function of the grammar calls this for an opaque reference, and
    /// passes the stores of the result the reference came from.
    pub fn children(self, shared: &SharedPool, lists: &ListStore, f: &mut dyn FnMut(N)) {
        (self.table.children)(shared, lists, self.raw, f);
    }
}

impl<N> Clone for OpaqueRef<N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<N> Copy for OpaqueRef<N> {}

impl<N> PartialEq for OpaqueRef<N> {
    /// Compares the raw handle value and the address of the table.
    ///
    /// Two references are equal when they name the same Node of the same application. The table
    /// is compared by address, because one application has exactly one table.
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw && ptr::eq(self.table, other.table)
    }
}

impl<N> Eq for OpaqueRef<N> {}

impl<N> Hash for OpaqueRef<N> {
    /// Hashes the raw handle value and the address of the table, which is what `eq` compares.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
        ptr::from_ref(self.table).addr().hash(state);
    }
}

impl<N> fmt::Debug for OpaqueRef<N> {
    /// Prints the Node's name and the raw handle value, for example `Hidden#7`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", self.table.name, self.raw)
    }
}

/// Reports the direct children of one Node of a Flat AST.
///
/// Recursa implements this trait on the grammar's own `FlatAst` owner. The implementation
/// forwards to the generated `children` function, which reads one Node out of its pool and hands
/// every child handle to `f` in source order.
///
/// A parent link is not stored, so the trait walks only downward. A consumer that needs parents
/// builds a side table from one walk.
pub trait FlatChildren {
    /// The traversal value that names one Node of this Flat AST.
    ///
    /// For a generated owner this is the grammar's `NodeRef`.
    type NodeRef: Copy;

    /// Calls `f` once per direct child of `node`, in source order.
    ///
    /// The sink is a trait object, so the whole traversal of one grammar compiles once however
    /// many callers walk it. An implementation never panics for a handle the owner produced.
    fn children(&self, node: Self::NodeRef, f: &mut dyn FnMut(Self::NodeRef));
}

/// Observes the Nodes of a depth-first walk over a Flat AST.
///
/// Both methods have a default, so a visitor states only the half it needs. [`walk`] calls
/// `enter` before a Node's children and `leave` after them.
pub trait FlatVisitor<A: FlatChildren + ?Sized> {
    /// Observes `node` before its children.
    ///
    /// Return `false` to skip the children of `node`. [`Self::leave`] still runs for `node`, so
    /// the two calls always pair.
    fn enter(&mut self, ast: &A, node: A::NodeRef) -> bool {
        let _ = (ast, node);
        true
    }

    /// Observes `node` after its children.
    fn leave(&mut self, ast: &A, node: A::NodeRef) {
        let _ = (ast, node);
    }
}

/// One pending step of a [`walk`], held on the walk's own stack.
///
/// The two steps make the walk iterative. A Node is pushed back as `Leave` before its children are
/// pushed as `Enter`. The children therefore run first, and the parent still gets its second
/// call.
enum Step<N> {
    /// Call `enter` for this Node, then queue its children.
    Enter(N),
    /// Call `leave` for this Node, whose children are done.
    Leave(N),
}

/// Walks `root` and every Node below it, depth first, and reports each one to `visitor`.
///
/// The walk is iterative. It keeps its own stack on the heap, so the depth of the Flat AST does
/// not bound it: a chain of a hundred thousand Nodes walks without a deep call stack. One buffer
/// collects the children of each Node and is reused for every Node of the walk.
///
/// `visitor.enter` runs before the children of a Node and returns `false` to skip them.
/// `visitor.leave` runs after them, and also after a skipped Node.
pub fn walk<A: FlatChildren + ?Sized>(
    ast: &A,
    root: A::NodeRef,
    visitor: &mut impl FlatVisitor<A>,
) {
    let mut steps = vec![Step::Enter(root)];
    let mut children = Vec::new();
    while let Some(step) = steps.pop() {
        match step {
            Step::Leave(node) => visitor.leave(ast, node),
            Step::Enter(node) => {
                steps.push(Step::Leave(node));
                if !visitor.enter(ast, node) {
                    continue;
                }
                ast.children(node, &mut |child| children.push(child));
                steps.extend(children.drain(..).rev().map(Step::Enter));
            }
        }
    }
}

/// One pooled Node whose child of the same pool does not precede it.
///
/// Invariant I1 of `docs/flat-ast-design.md` promises that a fresh parse leaves every typed pool
/// in post-order. The grammar's generated `check_post_order` walks the children of every pooled
/// Node and returns the first pair that breaks the promise as this value.
///
/// The pool name is the flat Node type of the pool. Both handle values are raw, because a
/// violation is a report about storage rather than a handle a consumer can use.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PostOrderViolation {
    /// The flat Node type of the typed pool that holds both Nodes.
    pool: &'static str,
    /// The raw handle value of the parent.
    parent: u32,
    /// The raw handle value of the child that does not precede the parent.
    child: u32,
}

impl PostOrderViolation {
    /// Records one violation of the post-order promise.
    ///
    /// Generated code calls this after its own check finds the pair.
    pub const fn new(pool: &'static str, parent: u32, child: u32) -> Self {
        Self {
            pool,
            parent,
            child,
        }
    }

    /// Returns the flat Node type of the pool that holds both Nodes.
    pub const fn pool(self) -> &'static str {
        self.pool
    }

    /// Returns the raw handle value of the parent.
    pub const fn parent(self) -> u32 {
        self.parent
    }

    /// Returns the raw handle value of the child.
    pub const fn child(self) -> u32 {
        self.child
    }
}

/// Checks one pooled parent against one child of the same pool.
///
/// The promise is one comparison: a child that a parse built before its parent has a smaller
/// handle. This function is that comparison. The generated `check_post_order` of every grammar
/// therefore decides a pair the same way, and a test can hand the decision a pair of its own.
///
/// `pool` names the flat Node type both Nodes belong to, and both values are raw handle values.
pub const fn check_post_order_pair(
    pool: &'static str,
    parent: u32,
    child: u32,
) -> Result<(), PostOrderViolation> {
    if child >= parent {
        return Err(PostOrderViolation::new(pool, parent, child));
    }
    Ok(())
}

impl fmt::Display for PostOrderViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the `{}` pool is out of post-order: the child at {} does not precede its parent at {}",
            self.pool, self.child, self.parent,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash as _, Hasher as _};
    use std::num::NonZeroU32;

    use super::{
        FlatChildren, FlatVisitor, ListStore, OpaqueRef, OpaqueTable, PostOrderViolation,
        SharedPool, check_post_order_pair, walk,
    };
    use crate::flat::{FlatNode, Handle};

    /// A Flat AST stand-in: one child list per Node, addressed by index.
    struct Tree {
        /// The children of each Node, in source order.
        children: Vec<Vec<usize>>,
    }

    impl FlatChildren for Tree {
        type NodeRef = usize;

        fn children(&self, node: usize, f: &mut dyn FnMut(usize)) {
            for child in &self.children[node] {
                f(*child);
            }
        }
    }

    /// Records one line per call, so a test can pin the order of the whole walk.
    struct Recorder {
        /// One `enter` or `leave` line per call, in call order.
        lines: Vec<String>,
        /// Nodes whose children the walk must skip.
        skip: Vec<usize>,
    }

    impl Recorder {
        /// Creates a recorder that enters every Node.
        fn new() -> Self {
            Self {
                lines: Vec::new(),
                skip: Vec::new(),
            }
        }
    }

    impl FlatVisitor<Tree> for Recorder {
        fn enter(&mut self, _ast: &Tree, node: usize) -> bool {
            self.lines.push(format!("enter {node}"));
            !self.skip.contains(&node)
        }

        fn leave(&mut self, _ast: &Tree, node: usize) {
            self.lines.push(format!("leave {node}"));
        }
    }

    /// Counts Nodes without keeping one line each, for the deep case.
    struct Counter {
        /// Number of `enter` calls.
        entered: usize,
        /// Number of `leave` calls.
        left: usize,
        /// The deepest nesting the walk reached.
        depth: usize,
        /// The current nesting.
        open: usize,
    }

    impl FlatVisitor<Tree> for Counter {
        fn enter(&mut self, _ast: &Tree, _node: usize) -> bool {
            self.entered += 1;
            self.open += 1;
            self.depth = self.depth.max(self.open);
            true
        }

        fn leave(&mut self, _ast: &Tree, _node: usize) {
            self.left += 1;
            self.open -= 1;
        }
    }

    /// Builds the three-level tree the order tests use.
    fn sample() -> Tree {
        Tree {
            children: vec![vec![1, 4], vec![2, 3], Vec::new(), Vec::new(), Vec::new()],
        }
    }

    #[test]
    fn a_walk_enters_a_parent_before_its_children_and_leaves_it_after_them() {
        let tree = sample();
        let mut recorder = Recorder::new();
        walk(&tree, 0, &mut recorder);
        assert_eq!(
            recorder.lines,
            [
                "enter 0", "enter 1", "enter 2", "leave 2", "enter 3", "leave 3", "leave 1",
                "enter 4", "leave 4", "leave 0",
            ]
        );
    }

    #[test]
    fn a_walk_visits_children_in_source_order() {
        let tree = sample();
        let mut recorder = Recorder::new();
        walk(&tree, 1, &mut recorder);
        assert_eq!(
            recorder.lines,
            [
                "enter 1", "enter 2", "leave 2", "enter 3", "leave 3", "leave 1"
            ]
        );
    }

    #[test]
    fn enter_returns_false_to_skip_the_children_and_still_leaves_the_node() {
        let tree = sample();
        let mut recorder = Recorder::new();
        recorder.skip.push(1);
        walk(&tree, 0, &mut recorder);
        assert_eq!(
            recorder.lines,
            [
                "enter 0", "enter 1", "leave 1", "enter 4", "leave 4", "leave 0"
            ]
        );
    }

    #[test]
    fn a_walk_of_a_hundred_thousand_deep_chain_does_not_recurse() {
        const DEPTH: usize = 100_000;
        let mut children = (1..DEPTH).map(|child| vec![child]).collect::<Vec<_>>();
        children.push(Vec::new());
        let tree = Tree { children };
        let mut counter = Counter {
            entered: 0,
            left: 0,
            depth: 0,
            open: 0,
        };
        walk(&tree, 0, &mut counter);
        assert_eq!(counter.entered, DEPTH);
        assert_eq!(counter.left, DEPTH);
        assert_eq!(counter.depth, DEPTH);
        assert_eq!(counter.open, 0);
    }

    #[test]
    fn a_child_that_precedes_its_parent_keeps_the_post_order_promise() {
        assert_eq!(check_post_order_pair("FlatValue", 7, 6), Ok(()));
        assert_eq!(check_post_order_pair("FlatValue", 7, 1), Ok(()));
    }

    #[test]
    fn a_child_that_does_not_precede_its_parent_is_a_violation() {
        assert_eq!(
            check_post_order_pair("FlatValue", 3, 7),
            Err(PostOrderViolation::new("FlatValue", 3, 7)),
        );
        assert_eq!(
            check_post_order_pair("FlatValue", 3, 3),
            Err(PostOrderViolation::new("FlatValue", 3, 3)),
            "a Node is never its own child, so an equal handle is a violation too",
        );
    }

    #[test]
    fn a_violation_reports_the_pool_and_both_raw_handles() {
        let violation = PostOrderViolation::new("FlatValue", 3, 7);
        assert_eq!(violation.pool(), "FlatValue");
        assert_eq!(violation.parent(), 3);
        assert_eq!(violation.child(), 7);
        assert_eq!(
            violation.to_string(),
            "the `FlatValue` pool is out of post-order: the child at 7 does not precede its \
             parent at 3",
        );
    }

    /// A Node the shared pool can hold, which stands in for one generated flat Node.
    ///
    /// The value is the number of children the Node reports, so a test can steer the walk.
    #[derive(Clone, Copy)]
    struct Leaf(u32);

    impl FlatNode for Leaf {
        const TAG: u32 = 0x4c45_4146;
    }

    /// Reports one child per unit of the Node's value.
    ///
    /// This is the shape Recursa generates beside a declaration the grammar root cannot name: it
    /// reads the Node out of the shared pool, and it names no type of the owner.
    fn leaf_children(
        shared: &SharedPool,
        _lists: &ListStore,
        raw: NonZeroU32,
        f: &mut dyn FnMut(usize),
    ) {
        let leaf = *shared.get::<Leaf>(Handle::from_raw(raw));
        for child in 0..leaf.0 as usize {
            f(child);
        }
    }

    /// The table of the Node type the opaque tests read.
    static LEAF: OpaqueTable<usize> = OpaqueTable::new("Leaf", leaf_children);

    /// A second table of the same shape, which proves a reference compares its table too.
    static OTHER: OpaqueTable<usize> = OpaqueTable::new("Other", leaf_children);

    /// Returns the hash of one value, so two values can be compared by hash.
    fn hash_of(reference: OpaqueRef<usize>) -> u64 {
        let mut hasher = DefaultHasher::new();
        reference.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn an_opaque_reference_reports_the_name_its_table_holds() {
        let raw = NonZeroU32::new(9).expect("nine is not zero");
        assert_eq!(OpaqueRef::new(raw, &LEAF).name(), "Leaf");
        assert_eq!(OpaqueRef::new(raw, &OTHER).name(), "Other");
        assert_eq!(LEAF.name(), "Leaf");
        assert_eq!(OpaqueRef::new(raw, &LEAF).raw(), raw);
    }

    #[test]
    fn an_opaque_reference_compares_its_handle_and_its_table() {
        let (first, second) = (
            NonZeroU32::new(1).expect("one is not zero"),
            NonZeroU32::new(2).expect("two is not zero"),
        );
        assert_eq!(OpaqueRef::new(first, &LEAF), OpaqueRef::new(first, &LEAF));
        assert_ne!(
            OpaqueRef::new(first, &LEAF),
            OpaqueRef::new(second, &LEAF),
            "two Nodes of one application are distinct",
        );
        assert_ne!(
            OpaqueRef::new(first, &LEAF),
            OpaqueRef::new(first, &OTHER),
            "one handle value in two applications names two Nodes",
        );
        assert_eq!(
            hash_of(OpaqueRef::new(first, &LEAF)),
            hash_of(OpaqueRef::new(first, &LEAF)),
            "two equal references hash alike",
        );
    }

    #[test]
    fn an_opaque_reference_prints_its_name_and_its_raw_handle() {
        let raw = NonZeroU32::new(7).expect("seven is not zero");
        assert_eq!(format!("{:?}", OpaqueRef::new(raw, &LEAF)), "Leaf#7");
        assert_eq!(
            format!("{LEAF:?}"),
            "OpaqueTable { name: \"Leaf\", .. }",
            "a table prints its name and hides its function pointer",
        );
    }

    #[test]
    fn an_opaque_reference_reads_its_children_out_of_the_shared_pool() {
        let mut shared = SharedPool::new(false);
        let handle = shared.alloc(Leaf(3), None);
        let lists = ListStore::new();
        let reference = OpaqueRef::new(handle.raw(), &LEAF);
        let mut children = Vec::new();
        reference.children(&shared, &lists, &mut |child| children.push(child));
        assert_eq!(
            children,
            [0, 1, 2],
            "the table's function reports the Node's own children",
        );
    }
}
