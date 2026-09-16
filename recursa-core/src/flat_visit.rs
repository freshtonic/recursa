//! Grammar-erased traversal of a Flat AST: the children contract a generated owner implements,
//! and the depth-first walk built on it.
//!
//! [`crate::flat`] holds the stores and [`crate::flat_owner`] reads one Node out of them. This
//! module adds the layer above them. A generated owner reports the direct children of one Node
//! through [`crate::flat_visit::FlatChildren`], and [`crate::flat_visit::walk`] drives a
//! [`crate::flat_visit::FlatVisitor`] over the tree those children make. Neither trait names a
//! grammar type, so one walk serves every grammar and an adapter can reuse it.
//!
//! The traversal value is the grammar's own `NodeRef`, which Recursa generates: an enum with one
//! variant per Node application, each holding that application's Node handle. A text handle and a
//! presence flag are data, not children, so neither appears in it.
//!
//! `docs/flat-ast-design.md` settles the traversal API and the post-order promise that
//! [`crate::flat_visit::PostOrderViolation`] reports.

use core::fmt;

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
/// The two steps are what make the walk iterative: a Node is pushed back as `Leave` before its
/// children are pushed as `Enter`, so the children run first and the parent still gets its second
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
/// handle. This function is that comparison, so the generated `check_post_order` of every grammar
/// decides a pair the same way and a test can hand the decision a pair of its own.
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
    use super::{FlatChildren, FlatVisitor, PostOrderViolation, check_post_order_pair, walk};

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
}
