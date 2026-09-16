//! Flat AST storage primitives: handles, typed pools, the shared pool, the list store, the text
//! pool, and side tables.
//!
//! One parsed result owns one Flat AST store. The store keeps every Node of a pooled Node type in
//! its own [`TypedPool`], every other Node in one [`SharedPool`] of four-byte words, every list of
//! handles in one [`ListStore`], and every content token's text in one [`TextPool`]. Nodes refer to
//! one another through four-byte typed handles, never through pointers. `docs/flat-ast-design.md`
//! settles the design and `docs/adr/0012-tiered-flat-storage.md` records the decision.
//!
//! Every part is append-only. A handle stays valid for the life of the store, and no slot is
//! removed, reused, or compacted. This module therefore exposes no mutable Node access: there is no
//! `get_mut` and no `IndexMut` on a pool. Mutation is a later milestone. A [`SideTable`] is the
//! exception, because a consumer owns its values and writing them does not change the AST.
//!
//! # Handle encoding
//!
//! Every handle is `#[repr(transparent)]` over [`NonZeroU32`], so an optional handle is also four
//! bytes. A pool interprets the raw value; the handle type does not know which pool holds it.
//!
//! - A typed pool, the text pool, and a side table read the raw value as `index + 1`.
//! - The shared pool and the list store read it as an offset into their word or slot vector, whose
//!   slot zero is reserved and never allocated. The value is therefore never zero.
//!
//! Exhausting the `u32` range is a checked panic with a clear message in every build, as
//! `docs/adr/0009-give-each-flat-ast-its-own-typed-stores.md` requires. A handle never wraps,
//! truncates, or becomes zero.
//!
//! # Safety model
//!
//! The shared pool stores whole four-byte words. Every flat Node has an alignment of at most four,
//! because a handle is a `u32`, a list handle is two `u32`, and a [`Span`] is two `u32`. A `Vec<u32>`
//! buffer is four-byte aligned, so a Node written at a word offset can be read back through an
//! aligned pointer. [`SharedPool::alloc`] asserts `align_of::<T>() <= 4` at compile time and
//! [`SharedPool::get`] performs the only pointer cast, guarded by the typed handle (invariant I3),
//! the bounds check, and, in debug and test builds, a type tag word. The list store reads a run of
//! `u32` slots as a slice of handles, which is sound because it wrote every slot from a handle's raw
//! value (invariant I4) and because [`ListItem`] requires a transparent representation. The value
//! stack in this crate is the precedent for both the confinement and the documentation style. This
//! module's tests run under Miri in CI.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ops::{Index, IndexMut};
use std::ptr;
use std::slice;

use crate::Span;
use crate::parsed::ContentText;

/// Bytes in one shared-pool or list-store slot.
const WORD_BYTES: usize = size_of::<u32>();

/// Alignment of one shared-pool word, and the highest alignment a shared-pool Node may have.
const WORD_ALIGN: usize = align_of::<u32>();

/// Words one captured [`Span`] occupies beside a shared-pool Node.
const SPAN_WORDS: usize = 2;

/// Whether the shared pool writes a type tag word before each Node.
///
/// Debug and test builds write and check the tag, which turns a handle used with the wrong Node
/// type into a clear panic. Release builds omit it, so the layout differs between profiles. That
/// costs nothing, because handles never outlive the parse that made them.
const TAGGED: bool = cfg!(any(debug_assertions, test));

/// Reports that a store cannot address another entry, and never returns.
///
/// This is a representation-capacity guard, not an allocation failure. It fires in every build.
#[cold]
#[inline(never)]
fn capacity_exhausted(store: &'static str) -> ! {
    panic!(
        "Flat AST {store} capacity exhausted: a four-byte handle addresses at most {} entries. \
         Recursa panics here instead of truncating a handle. This limit is not an allocation \
         failure.",
        u32::MAX
    )
}

/// Reports that a store does not hold the entry a handle names, and never returns.
#[cold]
#[inline(never)]
fn out_of_range(store: &'static str, raw: NonZeroU32) -> ! {
    panic!(
        "Flat AST {store} does not hold the entry of handle {raw}: the handle belongs to another \
         store or another parse"
    )
}

/// Converts a store offset into a handle value and guards the handle range.
///
/// `offset` is the first word or slot the entry occupies, counted from a reserved slot zero, and
/// `occupied` is the number of words or slots it uses. The call panics when the entry's last word
/// would fall outside the `u32` handle range.
fn offset_handle(offset: usize, occupied: usize, store: &'static str) -> NonZeroU32 {
    let fits = offset
        .checked_add(occupied)
        .is_some_and(|end| end <= u32::MAX as usize);
    if !fits {
        capacity_exhausted(store);
    }
    match NonZeroU32::new(offset as u32) {
        Some(raw) => raw,
        None => unreachable!("slot zero of a Flat AST store is reserved and never allocated"),
    }
}

/// Returns the two words that record `span` beside a shared-pool Node.
fn span_to_words(span: Span) -> [u32; SPAN_WORDS] {
    [span.start(), span.end()]
}

/// Rebuilds the [`Span`] that [`span_to_words`] recorded.
///
/// # Panics
///
/// Panics when the words did not come from a checked [`Span`].
fn span_from_words(start: u32, end: u32) -> Span {
    if start == u32::MAX && end == u32::MAX {
        return Span::SYNTHETIC;
    }
    Span::new(start as usize, end as usize)
        .expect("a shared-pool span word pair always comes from a checked Span")
}

/// Displays `T`'s type name with every module path removed.
///
/// Handle formatting uses it so that a debug line names the Node rather than its module path:
/// `a::b::Handle<c::Expr>` displays as `Handle<Expr>`.
struct ShortTypeName<T: ?Sized>(
    /// Names the type without storing it.
    PhantomData<fn() -> T>,
);

impl<T: ?Sized> ShortTypeName<T> {
    /// Returns the displayable short name of `T`.
    const fn of() -> Self {
        Self(PhantomData)
    }
}

impl<T: ?Sized> fmt::Display for ShortTypeName<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rest = std::any::type_name::<T>();
        loop {
            // A path runs up to the first character that cannot belong to one.
            let boundary = rest.find(|character: char| {
                !(character.is_alphanumeric() || character == '_' || character == ':')
            });
            let (path, tail) = match boundary {
                Some(at) => rest.split_at(at),
                None => (rest, ""),
            };
            formatter.write_str(path.rsplit_once("::").map_or(path, |(_, last)| last))?;
            let Some(delimiter) = tail.chars().next() else {
                return Ok(());
            };
            let width = delimiter.len_utf8();
            formatter.write_str(&tail[..width])?;
            rest = &tail[width..];
        }
    }
}

/// One Node type the shared pool can hold.
///
/// Recursa's generator implements this trait for every flat Node type. `TAG` is derived from the
/// Node key, so two Node types of one grammar never share a tag. Debug and test builds write the
/// tag before a Node's words and check it on every read, which reports a mismatched handle instead
/// of reinterpreting bytes.
pub trait FlatNode: Copy + 'static {
    /// The type tag the shared pool writes before this Node's words in debug and test builds.
    const TAG: u32;
}

/// The four-byte typed handle of one Node in a Flat AST store.
///
/// The type parameter names the Node. The Node's declaration decides whether the handle resolves
/// into a [`TypedPool`] or the [`SharedPool`], so a field type does not change when a Node moves
/// between pools. No storage tag is stored, and raw construction is private to this crate.
///
/// `Debug` prints the Node's short type name and the handle's zero-based value, for example
/// `Expr#3`. For a typed-pool handle that value is the pool index. For a shared-pool handle it is
/// the word offset minus one, which identifies the Node but is not an index.
#[repr(transparent)]
pub struct Handle<T> {
    /// The raw handle value: `index + 1` in a typed pool, or a word offset in the shared pool.
    raw: NonZeroU32,
    /// Names the Node without storing it, and keeps the handle `Send` and `Sync` for every `T`.
    _node: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Wraps a raw handle value that a store produced.
    const fn from_raw(raw: NonZeroU32) -> Self {
        Self {
            raw,
            _node: PhantomData,
        }
    }

    /// Returns the handle of the entry at zero-based `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index + 1` does not fit a `u32`, which exhausts the handle range.
    fn from_index(index: usize) -> Self {
        let Some(offset) = index.checked_add(1) else {
            capacity_exhausted("typed pool");
        };
        Self::from_raw(offset_handle(offset, 0, "typed pool"))
    }

    /// Returns the zero-based index of the entry, which only an index-addressed store may use.
    const fn index(self) -> usize {
        self.raw.get() as usize - 1
    }

    /// Returns the raw handle value, which the owning store interprets.
    pub const fn raw(self) -> NonZeroU32 {
        self.raw
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", ShortTypeName::<T>::of(), self.index())
    }
}

/// The four-byte typed handle of one content token's text in a result's text pool.
///
/// The type parameter names the content-token type. The pool, not the Node, owns the text, so every
/// flat Node stays `Copy`. A text handle is data rather than a child: it never appears in a
/// traversal value.
///
/// `Debug` prints the token type's short name and the handle's zero-based index, for example
/// `Text<NameText>#0`.
#[repr(transparent)]
pub struct Text<K> {
    /// The raw handle value, which is the text pool index plus one.
    raw: NonZeroU32,
    /// Names the content-token type without storing it, and keeps the handle `Send` and `Sync`.
    _kind: PhantomData<fn() -> K>,
}

impl<K> Text<K> {
    /// Wraps a raw handle value that the text pool produced.
    const fn from_raw(raw: NonZeroU32) -> Self {
        Self {
            raw,
            _kind: PhantomData,
        }
    }

    /// Returns the handle of the text at zero-based `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index + 1` does not fit a `u32`, which exhausts the handle range.
    fn from_index(index: usize) -> Self {
        let Some(offset) = index.checked_add(1) else {
            capacity_exhausted("text pool");
        };
        Self::from_raw(offset_handle(offset, 0, "text pool"))
    }

    /// Returns the zero-based index of the text in its pool.
    const fn index(self) -> usize {
        self.raw.get() as usize - 1
    }

    /// Returns the raw handle value, which the owning text pool interprets.
    pub const fn raw(self) -> NonZeroU32 {
        self.raw
    }
}

impl<K> Clone for Text<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Text<K> {}

impl<K> PartialEq for Text<K> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<K> Eq for Text<K> {}

impl<K> Hash for Text<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<K> fmt::Debug for Text<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Text<{}>#{}",
            ShortTypeName::<K>::of(),
            self.index()
        )
    }
}

/// A handle a [`ListStore`] run can hold.
///
/// # Safety
///
/// An implementor must be `#[repr(transparent)]` over [`NonZeroU32`], and `from_raw` must rebuild
/// the value that `raw` returned. The list store relies on both: it writes each run slot from
/// `raw`, and it reads a run of `u32` slots back as a slice of the implementing type.
pub unsafe trait ListItem: Copy {
    /// Returns the handle's raw value, which the list store writes into one slot.
    fn raw(self) -> NonZeroU32;

    /// Rebuilds the handle from a raw value.
    ///
    /// # Safety
    ///
    /// `raw` must be a value that `raw()` returned for a handle of this type, so that the rebuilt
    /// handle names an entry of the store that produced it.
    unsafe fn from_raw(raw: NonZeroU32) -> Self;
}

// SAFETY: `Handle<T>` is `#[repr(transparent)]` over `NonZeroU32`, and `from_raw` stores the given
// value unchanged, so it rebuilds the handle that `raw` came from.
unsafe impl<T> ListItem for Handle<T> {
    fn raw(self) -> NonZeroU32 {
        self.raw
    }

    unsafe fn from_raw(raw: NonZeroU32) -> Self {
        Self::from_raw(raw)
    }
}

// SAFETY: `Text<K>` is `#[repr(transparent)]` over `NonZeroU32`, and `from_raw` stores the given
// value unchanged, so it rebuilds the handle that `raw` came from.
unsafe impl<K> ListItem for Text<K> {
    fn raw(self) -> NonZeroU32 {
        self.raw
    }

    unsafe fn from_raw(raw: NonZeroU32) -> Self {
        Self::from_raw(raw)
    }
}

/// The eight-byte handle of one contiguous run of handles in a result's list store.
///
/// The run may be empty, which is a value distinct from an absent list. A zero-or-more field maps
/// to this handle type.
///
/// `Debug` prints the item type's short name, the start slot, and the length, for example
/// `List<Handle<Expr>>@1+2`.
pub struct List<H: ListItem> {
    /// First slot of the run in the list store; never zero, because slot zero is reserved.
    start: NonZeroU32,
    /// Number of slots in the run, which may be zero.
    len: u32,
    /// Names the item type without storing it, and keeps the handle `Send` and `Sync`.
    _item: PhantomData<fn() -> H>,
}

impl<H: ListItem> List<H> {
    /// Wraps a run the list store produced.
    const fn new(start: NonZeroU32, len: u32) -> Self {
        Self {
            start,
            len,
            _item: PhantomData,
        }
    }

    /// Returns the number of handles in the run.
    pub const fn len(self) -> usize {
        self.len as usize
    }

    /// Reports whether the run holds no handle.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

impl<H: ListItem> Clone for List<H> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H: ListItem> Copy for List<H> {}

impl<H: ListItem> PartialEq for List<H> {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start && self.len == other.len
    }
}

impl<H: ListItem> Eq for List<H> {}

impl<H: ListItem> Hash for List<H> {
    fn hash<S: Hasher>(&self, state: &mut S) {
        self.start.hash(state);
        self.len.hash(state);
    }
}

impl<H: ListItem> fmt::Debug for List<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "List<{}>@{}+{}",
            ShortTypeName::<H>::of(),
            self.start,
            self.len
        )
    }
}

/// The eight-byte handle of one non-empty contiguous run of handles in a result's list store.
///
/// A one-or-more field maps to this handle type, whose length cannot be zero. Use [`List1::as_list`]
/// to read the run through the zero-or-more API.
///
/// `Debug` prints the item type's short name, the start slot, and the length, for example
/// `List1<Handle<Expr>>@1+2`.
pub struct List1<H: ListItem> {
    /// First slot of the run in the list store; never zero, because slot zero is reserved.
    start: NonZeroU32,
    /// Number of slots in the run, which is at least one.
    len: NonZeroU32,
    /// Names the item type without storing it, and keeps the handle `Send` and `Sync`.
    _item: PhantomData<fn() -> H>,
}

impl<H: ListItem> List1<H> {
    /// Wraps a non-empty run the list store produced.
    const fn new(start: NonZeroU32, len: NonZeroU32) -> Self {
        Self {
            start,
            len,
            _item: PhantomData,
        }
    }

    /// Returns the number of handles in the run, which is at least one.
    pub const fn len(self) -> usize {
        self.len.get() as usize
    }

    /// Returns `false`; provided for handle-compatible APIs.
    pub const fn is_empty(self) -> bool {
        false
    }

    /// Returns the same run as a zero-or-more list handle.
    pub const fn as_list(self) -> List<H> {
        List::new(self.start, self.len.get())
    }
}

impl<H: ListItem> From<List1<H>> for List<H> {
    fn from(list: List1<H>) -> Self {
        list.as_list()
    }
}

impl<H: ListItem> Clone for List1<H> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H: ListItem> Copy for List1<H> {}

impl<H: ListItem> PartialEq for List1<H> {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start && self.len == other.len
    }
}

impl<H: ListItem> Eq for List1<H> {}

impl<H: ListItem> Hash for List1<H> {
    fn hash<S: Hasher>(&self, state: &mut S) {
        self.start.hash(state);
        self.len.hash(state);
    }
}

impl<H: ListItem> fmt::Debug for List1<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "List1<{}>@{}+{}",
            ShortTypeName::<H>::of(),
            self.start,
            self.len
        )
    }
}

// Layout contracts the design fixes: an optional handle costs nothing, and an optional list handle
// stays eight bytes.
const _: () = assert!(size_of::<Handle<()>>() == 4);
const _: () = assert!(size_of::<Option<Handle<()>>>() == 4);
const _: () = assert!(size_of::<Text<()>>() == 4);
const _: () = assert!(size_of::<Option<Text<()>>>() == 4);
const _: () = assert!(size_of::<List<Handle<()>>>() == 8);
const _: () = assert!(size_of::<Option<List<Handle<()>>>>() == 8);
const _: () = assert!(size_of::<List1<Handle<()>>>() == 8);
const _: () = assert!(size_of::<Option<List1<Handle<()>>>>() == 8);

/// A per-result store holding every Node of one pooled Node type contiguously.
///
/// Nodes of a fresh parse appear in the pool in post-order, because an LR reduce builds a parent
/// after its children. Invariant I5 holds because the pool is the only writer of its span array:
/// when the pool captures spans, the two vectors always have the same length.
///
/// The pool is append-only and exposes no mutable Node access.
pub struct TypedPool<T> {
    /// Every Node of the pooled type, in index order.
    nodes: Vec<T>,
    /// One span per Node when the parse requested capture, otherwise absent.
    spans: Option<Vec<Span>>,
}

impl<T> TypedPool<T> {
    /// Creates an empty pool that captures one span per Node only when `capture_spans` is true.
    pub fn new(capture_spans: bool) -> Self {
        Self {
            nodes: Vec::new(),
            spans: capture_spans.then(Vec::new),
        }
    }

    /// Appends `node` and returns its handle.
    ///
    /// A pool that does not capture spans drops `span`. A pool that captures spans stores
    /// [`Span::SYNTHETIC`] when `span` is `None`.
    ///
    /// # Panics
    ///
    /// Panics when the pool already holds every Node a four-byte handle can address.
    pub fn push(&mut self, node: T, span: Option<Span>) -> Handle<T> {
        let handle = Handle::from_index(self.nodes.len());
        self.nodes.push(node);
        if let Some(spans) = self.spans.as_mut() {
            spans.push(span.unwrap_or(Span::SYNTHETIC));
        }
        handle
    }

    /// Returns the number of Nodes in the pool.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Reports whether the pool holds no Node.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Borrows the Node `handle` names, or returns `None` when the pool does not hold it.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.nodes.get(handle.index())
    }

    /// Returns the span of the Node `handle` names.
    ///
    /// The result is `None` when the parse did not capture spans, and also when the pool does not
    /// hold the Node.
    pub fn span(&self, handle: Handle<T>) -> Option<Span> {
        self.spans.as_ref()?.get(handle.index()).copied()
    }

    /// Iterates every handle of the pool in index order.
    ///
    /// For a fresh parse that order is post-order: every same-pool child of a Node has a smaller
    /// index than the Node (invariant I1).
    pub fn handles(&self) -> impl Iterator<Item = Handle<T>> {
        (0..self.nodes.len()).map(Handle::from_index)
    }
}

impl<T> Index<Handle<T>> for TypedPool<T> {
    type Output = T;

    fn index(&self, handle: Handle<T>) -> &T {
        match self.get(handle) {
            Some(node) => node,
            None => out_of_range("typed pool", handle.raw()),
        }
    }
}

/// The per-result store that holds every Node not in a typed pool.
///
/// Each Node occupies whole four-byte words at a typed word offset. Word zero is reserved, so a
/// handle into this pool is never zero. A Node of `n` bytes takes `ceil(n / 4)` words, and a
/// zero-size Node takes one word, so every Node has its own address.
///
/// The words before a Node hold its optional record:
///
/// ```text
/// capture on,  tagged build:  [span.start, span.end][tag][node words...]
/// capture on,  release build: [span.start, span.end][node words...]
/// capture off, tagged build:  [tag][node words...]
/// capture off, release build: [node words...]
/// ```
///
/// A handle always names the Node's first word, so a reader never needs to know which record words
/// precede it.
///
/// The pool is append-only and exposes no mutable Node access.
pub struct SharedPool {
    /// Every Node's words, preceded by its record words. Word zero is reserved.
    words: Vec<u32>,
    /// Whether each Node's words follow a recorded span.
    capture_spans: bool,
}

impl SharedPool {
    /// Creates a pool holding only its reserved word zero, which records one span per Node only
    /// when `capture_spans` is true.
    pub fn new(capture_spans: bool) -> Self {
        Self {
            words: vec![0],
            capture_spans,
        }
    }

    /// Returns the number of words a Node of type `T` occupies, which is at least one.
    fn node_words<T>() -> usize {
        size_of::<T>().div_ceil(WORD_BYTES).max(1)
    }

    /// Returns the number of record words this pool writes before each Node.
    fn record_words(&self) -> usize {
        (if self.capture_spans { SPAN_WORDS } else { 0 }) + usize::from(TAGGED)
    }

    /// Returns the total number of words in the pool, including the reserved word zero.
    pub fn len_words(&self) -> usize {
        self.words.len()
    }

    /// Appends `node` and returns its handle.
    ///
    /// A pool that does not capture spans drops `span`. A pool that captures spans records
    /// [`Span::SYNTHETIC`] when `span` is `None`.
    ///
    /// Node types whose alignment exceeds four bytes are refused at compile time, which is
    /// invariant I6.
    ///
    /// # Panics
    ///
    /// Panics when the Node's last word would fall outside the `u32` handle range.
    pub fn alloc<T: FlatNode>(&mut self, node: T, span: Option<Span>) -> Handle<T> {
        const {
            assert!(
                align_of::<T>() <= WORD_ALIGN,
                "a shared-pool Node must have an alignment of at most four bytes"
            )
        };
        let node_words = Self::node_words::<T>();
        let offset = self.words.len() + self.record_words();
        let raw = offset_handle(offset, node_words, "shared pool");
        if self.capture_spans {
            self.words
                .extend_from_slice(&span_to_words(span.unwrap_or(Span::SYNTHETIC)));
        }
        if TAGGED {
            self.words.push(T::TAG);
        }
        debug_assert_eq!(
            self.words.len(),
            offset,
            "the shared pool wrote a record of the wrong width"
        );
        self.words.resize(offset + node_words, 0);
        // SAFETY: `offset + node_words` lies inside the words just reserved, and `offset` indexes a
        // `Vec<u32>` whose buffer is four-byte aligned, so the destination is aligned for a `T`
        // whose alignment the inline const above bounds at four (invariant I6). The destination is
        // freshly reserved storage that no live reference aliases, and writing it establishes
        // invariant I3 for the returned handle.
        unsafe {
            let destination = self.words.as_mut_ptr().add(offset).cast::<T>();
            ptr::write(destination, node);
        }
        Handle::from_raw(raw)
    }

    /// Borrows the Node `handle` names, or returns `None` when the pool does not hold it.
    ///
    /// # Panics
    ///
    /// In debug and test builds, panics when the tag beside the Node is not `T`'s tag, which means
    /// the handle names a Node of another type.
    pub fn try_get<T: FlatNode>(&self, handle: Handle<T>) -> Option<&T> {
        const {
            assert!(
                align_of::<T>() <= WORD_ALIGN,
                "a shared-pool Node must have an alignment of at most four bytes"
            )
        };
        let offset = handle.raw().get() as usize;
        let end = offset.checked_add(Self::node_words::<T>())?;
        if end > self.words.len() {
            return None;
        }
        if TAGGED {
            let tag = self.words[offset - 1];
            assert_eq!(
                tag,
                T::TAG,
                "shared pool tag mismatch at word {offset}: handle {} names a Node of another type",
                handle.raw()
            );
        }
        // SAFETY: the bounds check above proves the pool holds `node_words` words at `offset`, and
        // invariant I3 holds for a handle this pool produced: `alloc` wrote a `T` at exactly that
        // offset, and the pool is append-only, so those bytes still hold that `T`. The offset
        // indexes a `Vec<u32>` whose buffer is four-byte aligned and the inline const above bounds
        // `align_of::<T>()` at four (invariant I6), so the pointer is aligned. The shared borrow of
        // `self` keeps the buffer alive and unchanged for the returned lifetime.
        Some(unsafe { &*self.words.as_ptr().add(offset).cast::<T>() })
    }

    /// Borrows the Node `handle` names.
    ///
    /// # Panics
    ///
    /// Panics when the pool does not hold the Node. In debug and test builds, also panics when the
    /// tag beside the Node is not `T`'s tag.
    pub fn get<T: FlatNode>(&self, handle: Handle<T>) -> &T {
        match self.try_get(handle) {
            Some(node) => node,
            None => out_of_range("shared pool", handle.raw()),
        }
    }

    /// Returns the span recorded beside the Node `handle` names.
    ///
    /// The result is `None` when the parse did not capture spans, and also when the pool does not
    /// hold the Node.
    pub fn span<T: FlatNode>(&self, handle: Handle<T>) -> Option<Span> {
        if !self.capture_spans {
            return None;
        }
        let offset = handle.raw().get() as usize;
        let end = offset.checked_add(Self::node_words::<T>())?;
        if end > self.words.len() {
            return None;
        }
        let recorded = offset.checked_sub(usize::from(TAGGED) + SPAN_WORDS)?;
        Some(span_from_words(
            self.words[recorded],
            self.words[recorded + 1],
        ))
    }
}

/// The per-result store of every list of handles, each list contiguous.
///
/// Slot zero is reserved, so the start slot of a run is never zero. The store is the only writer of
/// its slots, which is invariant I4: every slot of a run holds the raw value of one handle of the
/// run's item type.
///
/// The store is append-only and exposes no mutable slot access.
pub struct ListStore {
    /// The raw value of every stored handle, run after run. Slot zero is reserved.
    slots: Vec<u32>,
}

impl Default for ListStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ListStore {
    /// Creates a store holding only its reserved slot zero.
    pub fn new() -> Self {
        Self { slots: vec![0] }
    }

    /// Returns the total number of slots in the store, including the reserved slot zero.
    pub fn len_slots(&self) -> usize {
        self.slots.len()
    }

    /// Appends `items` as one run and returns its list handle.
    ///
    /// An empty run is a valid handle. It consumes no slot and names the slot the next run will
    /// start at.
    ///
    /// # Panics
    ///
    /// Panics when the run's last slot would fall outside the `u32` handle range. The store may
    /// already have grown when the guard fires, because the guard reads the finished run's length.
    pub fn push<H: ListItem>(&mut self, items: impl IntoIterator<Item = H>) -> List<H> {
        let start = self.slots.len();
        self.slots
            .extend(items.into_iter().map(|item| item.raw().get()));
        let len = self.slots.len() - start;
        let start = offset_handle(start, len, "list store");
        List::new(start, len as u32)
    }

    /// Appends `first` followed by `rest` as one non-empty run and returns its list handle.
    ///
    /// # Panics
    ///
    /// Panics when the run's last slot would fall outside the `u32` handle range.
    pub fn push1<H: ListItem>(&mut self, first: H, rest: impl IntoIterator<Item = H>) -> List1<H> {
        let list = self.push(std::iter::once(first).chain(rest));
        let len = NonZeroU32::new(list.len).expect("push1 always stores at least its first item");
        List1::new(list.start, len)
    }

    /// Borrows the run `list` names as a slice of handles.
    ///
    /// # Panics
    ///
    /// Panics when the store does not hold the run.
    pub fn get<H: ListItem>(&self, list: List<H>) -> &[H] {
        let start = list.start.get() as usize;
        let len = list.len as usize;
        let holds = start
            .checked_add(len)
            .is_some_and(|end| end <= self.slots.len());
        if !holds {
            out_of_range("list store", list.start);
        }
        // SAFETY: the check above proves the store holds `len` slots at `start`. Invariant I4 holds
        // because `push` is the store's only writer: it filled each of those slots from the `raw`
        // value of one `H`, so every slot is non-zero and is a valid `H`. `ListItem` requires `H` to
        // be `#[repr(transparent)]` over `NonZeroU32`, so `H` has the size and alignment of a `u32`
        // and the run's words are a valid `[H]`. The shared borrow of `self` keeps the slots alive
        // and unchanged for the returned lifetime.
        unsafe { slice::from_raw_parts(self.slots.as_ptr().add(start).cast::<H>(), len) }
    }

    /// Borrows the non-empty run `list` names as a slice of handles.
    ///
    /// # Panics
    ///
    /// Panics when the store does not hold the run.
    pub fn get1<H: ListItem>(&self, list: List1<H>) -> &[H] {
        self.get(list.as_list())
    }
}

impl<H: ListItem> Index<List<H>> for ListStore {
    type Output = [H];

    fn index(&self, list: List<H>) -> &[H] {
        self.get(list)
    }
}

impl<H: ListItem> Index<List1<H>> for ListStore {
    type Output = [H];

    fn index(&self, list: List1<H>) -> &[H] {
        self.get1(list)
    }
}

/// One list a parse is still building on the [`ListScratch`] stack.
///
/// A reduce action keeps this value on the parser's value stack, pushes one item per list step, and
/// closes the list when the parent rule consumes it. It is `Copy`, holds no borrow, and names no
/// store: only the scratch stack it came from can interpret it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OpenList {
    /// First scratch slot of this list. Scratch slots are plain indices, not handles.
    start: u32,
    /// Number of items pushed so far.
    len: u32,
}

/// The parse-time scratch stack every list is built on.
///
/// LR completes an inner list before the outer list receives its next item, so open lists nest:
/// only the most recently opened list can accept an item. [`ListScratch::push`] asserts that in
/// debug builds. Closing a list copies its run into the [`ListStore`] and truncates the stack, so a
/// parse reuses the same buffer for every list.
pub struct ListScratch {
    /// The raw value of every item of every open list, outermost list first.
    slots: Vec<u32>,
}

impl Default for ListScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl ListScratch {
    /// Creates an empty scratch stack.
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Returns the number of items of every open list together.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Reports whether no open list holds an item, which every parse must reach when it ends.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Opens an empty list at the top of the stack.
    ///
    /// # Panics
    ///
    /// Panics when the stack already holds every item a four-byte offset can address.
    pub fn open(&self) -> OpenList {
        let Ok(start) = u32::try_from(self.slots.len()) else {
            capacity_exhausted("list scratch");
        };
        OpenList { start, len: 0 }
    }

    /// Pushes `item` onto `list`.
    ///
    /// # Panics
    ///
    /// Panics when the stack already holds every item a four-byte offset can address. In debug
    /// builds, also panics when another list is open above `list`.
    pub fn push<H: ListItem>(&mut self, list: &mut OpenList, item: H) {
        debug_assert_eq!(
            self.slots.len(),
            list.start as usize + list.len as usize,
            "another list is open above this one: LR reduce order must close the inner list first"
        );
        if self.slots.len() >= u32::MAX as usize {
            capacity_exhausted("list scratch");
        }
        self.slots.push(item.raw().get());
        list.len += 1;
    }

    /// Copies `list` into `store`, truncates the stack back to the list's start, and returns the
    /// stored run.
    ///
    /// # Panics
    ///
    /// Panics when the store cannot address the run. In debug builds, also panics when another list
    /// is open above `list`.
    pub fn close<H: ListItem>(&mut self, list: OpenList, store: &mut ListStore) -> List<H> {
        let start = list.start as usize;
        let end = start + list.len as usize;
        debug_assert_eq!(
            self.slots.len(),
            end,
            "another list is open above this one: LR reduce order must close the inner list first"
        );
        let items = self.slots[start..end].iter().map(|&raw| {
            let raw = NonZeroU32::new(raw).expect("a scratch slot always holds a non-zero handle");
            // SAFETY: `push` wrote every slot of this run from the `raw` value of one `H`, and the
            // debug assertion above plus the nesting contract prove the run is still this list's.
            // `ListItem` requires `from_raw` to rebuild the handle that `raw` came from.
            unsafe { H::from_raw(raw) }
        });
        let stored = store.push(items);
        self.slots.truncate(start);
        stored
    }

    /// Closes `list` as a non-empty run.
    ///
    /// # Panics
    ///
    /// Panics when `list` holds no item. The generator calls this only for a one-or-more rule,
    /// whose reduce always pushes at least one item.
    pub fn close1<H: ListItem>(&mut self, list: OpenList, store: &mut ListStore) -> List1<H> {
        let stored = self.close::<H>(list, store);
        match NonZeroU32::new(stored.len) {
            Some(len) => List1::new(stored.start, len),
            None => panic!(
                "close1 closed an empty list: a one-or-more rule always pushes at least one item"
            ),
        }
    }
}

/// The per-result store of every content token's text and lexical kind.
///
/// Each text still borrows the input or owns its replacement. The pool, not the Node, owns it, so
/// every flat Node stays `Copy`.
///
/// A content token that accepts several kinds carries its kind beside its text in the nested AST,
/// so the pool keeps that kind too. The generator encodes the token's accepted-kind enum as a
/// `u16` and decodes it back. The kind vector always has the same length as the text vector,
/// because the pool is the only writer of both.
///
/// The pool is append-only and exposes no mutable text access.
pub struct TextPool<'input> {
    /// Every stored text, in index order.
    texts: Vec<ContentText<'input>>,
    /// The lexical kind of every stored text, in the same index order.
    kinds: Vec<u16>,
    /// One span per text when the parse requested capture, otherwise absent.
    spans: Option<Vec<Span>>,
}

impl<'input> TextPool<'input> {
    /// Creates an empty pool that captures one span per text only when `capture_spans` is true.
    pub fn new(capture_spans: bool) -> Self {
        Self {
            texts: Vec::new(),
            kinds: Vec::new(),
            spans: capture_spans.then(Vec::new),
        }
    }

    /// Appends `text` with its lexical `kind` and returns its handle, typed by the content-token
    /// type `K`.
    ///
    /// `kind` is the generator's encoding of the token's accepted kind. A token that accepts one
    /// kind only still stores the encoding of that kind.
    ///
    /// A pool that does not capture spans drops `span`. A pool that captures spans stores
    /// [`Span::SYNTHETIC`] when `span` is `None`.
    ///
    /// # Panics
    ///
    /// Panics when the pool already holds every text a four-byte handle can address.
    pub fn push<K>(&mut self, text: ContentText<'input>, kind: u16, span: Option<Span>) -> Text<K> {
        let handle = Text::from_index(self.texts.len());
        self.texts.push(text);
        self.kinds.push(kind);
        if let Some(spans) = self.spans.as_mut() {
            spans.push(span.unwrap_or(Span::SYNTHETIC));
        }
        handle
    }

    /// Returns the number of texts in the pool.
    pub fn len(&self) -> usize {
        self.texts.len()
    }

    /// Reports whether the pool holds no text.
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    /// Borrows the text `handle` names, or returns `None` when the pool does not hold it.
    pub fn get<K>(&self, handle: Text<K>) -> Option<&ContentText<'input>> {
        self.texts.get(handle.index())
    }

    /// Returns the lexical kind of the text `handle` names, or `None` when the pool does not hold
    /// it.
    pub fn kind<K>(&self, handle: Text<K>) -> Option<u16> {
        self.kinds.get(handle.index()).copied()
    }

    /// Returns the span of the text `handle` names.
    ///
    /// The result is `None` when the parse did not capture spans, and also when the pool does not
    /// hold the text.
    pub fn span<K>(&self, handle: Text<K>) -> Option<Span> {
        self.spans.as_ref()?.get(handle.index()).copied()
    }
}

impl<'input, K> Index<Text<K>> for TextPool<'input> {
    type Output = ContentText<'input>;

    fn index(&self, handle: Text<K>) -> &ContentText<'input> {
        match self.get(handle) {
            Some(text) => text,
            None => out_of_range("text pool", handle.raw()),
        }
    }
}

/// A consumer-owned dense array with one value per Node of one typed pool.
///
/// A side table is not part of the Flat AST store. Recursa supplies the type and [`SideTable::grow`];
/// the consumer owns the values. Reading through a handle requires the table to be at least as long
/// as its pool, which is invariant I8. A table covers one typed pool only, which is one reason to
/// pool a Node type.
///
/// Writing a value is not AST mutation, so this type does expose [`IndexMut`].
pub struct SideTable<T, V> {
    /// One value per Node of the pool, in the pool's index order.
    values: Vec<V>,
    /// Names the pooled Node type without storing it, and keeps the table `Send` and `Sync`.
    _node: PhantomData<fn() -> T>,
}

impl<T, V> SideTable<T, V> {
    /// Creates a table of `len` copies of `init`.
    pub fn new(len: usize, init: V) -> Self
    where
        V: Clone,
    {
        Self {
            values: vec![init; len],
            _node: PhantomData,
        }
    }

    /// Creates a table with one copy of `init` per Node currently in `pool`.
    pub fn for_pool(pool: &TypedPool<T>, init: V) -> Self
    where
        V: Clone,
    {
        Self::new(pool.len(), init)
    }

    /// Extends the table with copies of `init` until it covers every Node in `pool`.
    ///
    /// The table never shrinks, so a value written before the pool grew stays where it was.
    pub fn grow(&mut self, pool: &TypedPool<T>, init: V)
    where
        V: Clone,
    {
        if self.values.len() < pool.len() {
            self.values.resize(pool.len(), init);
        }
    }

    /// Returns the number of values in the table.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether the table holds no value.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl<T, V> Index<Handle<T>> for SideTable<T, V> {
    type Output = V;

    fn index(&self, handle: Handle<T>) -> &V {
        match self.values.get(handle.index()) {
            Some(value) => value,
            None => out_of_range("side table", handle.raw()),
        }
    }
}

impl<T, V> IndexMut<Handle<T>> for SideTable<T, V> {
    fn index_mut(&mut self, handle: Handle<T>) -> &mut V {
        let raw = handle.raw();
        match self.values.get_mut(handle.index()) {
            Some(value) => value,
            None => out_of_range("side table", raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashSet;

    /// A two-word Node used for pool, list, and side-table tests.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Pair {
        /// First stored word.
        left: u32,
        /// Second stored word.
        right: u32,
    }

    impl FlatNode for Pair {
        const TAG: u32 = 0x5041_4952;
    }

    /// A Node of the same size as [`Pair`] and a different tag.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Other {
        /// First stored word.
        first: u32,
        /// Second stored word.
        second: u32,
    }

    impl FlatNode for Other {
        const TAG: u32 = 0x4f54_4852;
    }

    /// A Node whose variants differ in width.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Choice {
        /// A leaf carrying one word.
        Leaf(u32),
        /// A branch carrying two handles.
        Branch(Handle<Pair>, Handle<Pair>),
    }

    impl FlatNode for Choice {
        const TAG: u32 = 0x4348_4f49;
    }

    /// A zero-size Node.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Marker;

    impl FlatNode for Marker {
        const TAG: u32 = 0x4d41_524b;
    }

    /// Names a Node type in handle formatting tests.
    struct Expr;

    /// Names a content-token type in text pool tests.
    struct NameText;

    /// The encoded lexical kind of a plain name token.
    const NAME_KIND: u16 = 0;

    /// The encoded lexical kind of a keyword a name token also accepts.
    const KEYWORD_KIND: u16 = 7;

    /// Proves at compile time that the argument type is thread-safe.
    fn assert_send_sync<T: Send + Sync>() {}

    /// Returns a checked span, which the tested spans always are.
    fn span(start: usize, end: usize) -> Span {
        Span::new(start, end).expect("test spans are in range")
    }

    #[test]
    fn a_typed_pool_handle_is_one_more_than_its_slot() {
        assert_eq!(Handle::<Expr>::from_index(0).raw().get(), 1);
        assert_eq!(Handle::<Expr>::from_index(7).raw().get(), 8);
    }

    #[test]
    fn a_handle_round_trips_through_its_zero_based_index() {
        for index in [0, 1, 2, 4096, u32::MAX as usize - 1] {
            assert_eq!(Handle::<Expr>::from_index(index).index(), index);
        }
    }

    #[test]
    fn an_optional_handle_and_an_optional_list_handle_cost_no_extra_bytes() {
        assert_eq!(size_of::<Handle<Expr>>(), 4);
        assert_eq!(size_of::<Option<Handle<Expr>>>(), 4);
        assert_eq!(size_of::<Text<NameText>>(), 4);
        assert_eq!(size_of::<Option<Text<NameText>>>(), 4);
        assert_eq!(size_of::<List<Handle<Expr>>>(), 8);
        assert_eq!(size_of::<Option<List<Handle<Expr>>>>(), 8);
        assert_eq!(size_of::<List1<Handle<Expr>>>(), 8);
        assert_eq!(size_of::<Option<List1<Handle<Expr>>>>(), 8);
    }

    #[test]
    fn handles_of_one_index_compare_and_hash_alike() {
        let first = Handle::<Expr>::from_index(3);
        let same = Handle::<Expr>::from_index(3);
        let other = Handle::<Expr>::from_index(4);
        assert_eq!(first, same);
        assert_ne!(first, other);
        let mut seen = HashSet::new();
        assert!(seen.insert(first));
        assert!(!seen.insert(same));
        assert!(seen.insert(other));
    }

    #[test]
    fn a_handle_prints_its_short_node_name_and_zero_based_value() {
        assert_eq!(format!("{:?}", Handle::<Expr>::from_index(3)), "Expr#3");
        assert_eq!(
            format!("{:?}", Text::<NameText>::from_index(0)),
            "Text<NameText>#0"
        );
    }

    #[test]
    fn a_list_handle_prints_its_item_name_start_and_length() {
        let start = NonZeroU32::new(1).expect("one is non-zero");
        assert_eq!(
            format!("{:?}", List::<Handle<Expr>>::new(start, 2)),
            "List<Handle<Expr>>@1+2"
        );
        assert_eq!(
            format!("{:?}", List1::<Handle<Expr>>::new(start, start)),
            "List1<Handle<Expr>>@1+1"
        );
    }

    #[test]
    fn the_handle_guard_accepts_the_last_addressable_entry() {
        assert_eq!(
            Handle::<Expr>::from_index(u32::MAX as usize - 1)
                .raw()
                .get(),
            u32::MAX
        );
        assert_eq!(
            offset_handle(u32::MAX as usize - 1, 1, "shared pool").get(),
            u32::MAX - 1
        );
    }

    #[test]
    #[should_panic(expected = "capacity exhausted")]
    fn the_typed_pool_guard_panics_past_the_last_addressable_entry() {
        let _ = Handle::<Expr>::from_index(u32::MAX as usize);
    }

    #[test]
    #[should_panic(expected = "capacity exhausted")]
    fn the_word_guard_panics_when_a_node_would_end_past_the_handle_range() {
        let _ = offset_handle(u32::MAX as usize - 1, 2, "shared pool");
    }

    #[test]
    fn a_typed_pool_returns_every_node_it_stored() {
        let mut pool = TypedPool::new(false);
        let first = pool.push(Pair { left: 1, right: 2 }, None);
        let second = pool.push(Pair { left: 3, right: 4 }, None);
        assert_eq!(pool.len(), 2);
        assert!(!pool.is_empty());
        assert_eq!(pool[first], Pair { left: 1, right: 2 });
        assert_eq!(pool[second], Pair { left: 3, right: 4 });
        assert_eq!(pool.get(first), Some(&Pair { left: 1, right: 2 }));
        assert_eq!(pool.get(Handle::from_index(2)), None);
    }

    #[test]
    fn a_typed_pool_without_capture_drops_the_span_it_is_given() {
        let mut pool = TypedPool::new(false);
        let handle = pool.push(Pair { left: 1, right: 2 }, Some(span(0, 4)));
        assert_eq!(pool.span(handle), None);
    }

    #[test]
    fn a_typed_pool_that_captures_stores_a_synthetic_span_for_a_missing_one() {
        let mut pool = TypedPool::new(true);
        let real = pool.push(Pair { left: 1, right: 2 }, Some(span(0, 4)));
        let synthetic = pool.push(Pair { left: 3, right: 4 }, None);
        assert_eq!(pool.span(real), Some(span(0, 4)));
        assert_eq!(pool.span(synthetic), Some(Span::SYNTHETIC));
        assert_eq!(pool.span(Handle::from_index(2)), None);
    }

    #[test]
    fn a_typed_pool_iterates_its_handles_in_index_order() {
        let mut pool = TypedPool::new(false);
        let first = pool.push(Pair { left: 1, right: 1 }, None);
        let second = pool.push(Pair { left: 2, right: 2 }, None);
        let third = pool.push(Pair { left: 3, right: 3 }, None);
        assert_eq!(pool.handles().collect::<Vec<_>>(), [first, second, third]);
        assert!(TypedPool::<Pair>::new(false).handles().next().is_none());
    }

    #[test]
    #[should_panic(expected = "does not hold the entry")]
    fn indexing_a_typed_pool_out_of_range_panics() {
        let pool = TypedPool::<Pair>::new(false);
        let _ = pool[Handle::from_index(0)];
    }

    #[test]
    fn the_shared_pool_reserves_word_zero() {
        assert_eq!(SharedPool::new(false).len_words(), 1);
        assert_eq!(SharedPool::new(true).len_words(), 1);
    }

    #[test]
    fn the_shared_pool_round_trips_a_struct_node() {
        let mut pool = SharedPool::new(false);
        let handle = pool.alloc(Pair { left: 7, right: 9 }, None);
        assert_eq!(*pool.get(handle), Pair { left: 7, right: 9 });
        assert_eq!(pool.try_get(handle), Some(&Pair { left: 7, right: 9 }));
    }

    #[test]
    fn the_shared_pool_round_trips_an_enum_node() {
        let mut pool = SharedPool::new(false);
        let leaf = pool.alloc(Choice::Leaf(5), None);
        let branch = pool.alloc(
            Choice::Branch(Handle::from_index(0), Handle::from_index(1)),
            None,
        );
        assert_eq!(*pool.get(leaf), Choice::Leaf(5));
        assert_eq!(
            *pool.get(branch),
            Choice::Branch(Handle::from_index(0), Handle::from_index(1))
        );
    }

    #[test]
    fn a_zero_size_shared_pool_node_takes_one_word_of_its_own() {
        let mut pool = SharedPool::new(false);
        let before = pool.len_words();
        let first = pool.alloc(Marker, None);
        let second = pool.alloc(Marker, None);
        assert_ne!(first, second);
        assert_eq!(*pool.get(first), Marker);
        assert_eq!(*pool.get(second), Marker);
        let per_node = 1 + usize::from(TAGGED);
        assert_eq!(pool.len_words(), before + 2 * per_node);
    }

    #[test]
    fn the_shared_pool_keeps_two_interleaved_node_types_apart() {
        let mut pool = SharedPool::new(false);
        let pair_one = pool.alloc(Pair { left: 1, right: 2 }, None);
        let other_one = pool.alloc(
            Other {
                first: 3,
                second: 4,
            },
            None,
        );
        let marker = pool.alloc(Marker, None);
        let pair_two = pool.alloc(Pair { left: 5, right: 6 }, None);
        let other_two = pool.alloc(
            Other {
                first: 7,
                second: 8,
            },
            None,
        );
        assert_eq!(*pool.get(pair_one), Pair { left: 1, right: 2 });
        assert_eq!(
            *pool.get(other_one),
            Other {
                first: 3,
                second: 4
            }
        );
        assert_eq!(*pool.get(marker), Marker);
        assert_eq!(*pool.get(pair_two), Pair { left: 5, right: 6 });
        assert_eq!(
            *pool.get(other_two),
            Other {
                first: 7,
                second: 8
            }
        );
    }

    #[test]
    #[cfg(any(debug_assertions, test))]
    #[should_panic(expected = "shared pool tag mismatch")]
    fn the_shared_pool_rejects_a_handle_of_another_node_type() {
        let mut pool = SharedPool::new(false);
        let pair = pool.alloc(Pair { left: 1, right: 2 }, None);
        let disguised = Handle::<Other>::from_raw(pair.raw());
        let _ = pool.get(disguised);
    }

    #[test]
    fn the_shared_pool_writes_a_span_beside_each_node() {
        let mut pool = SharedPool::new(true);
        let first = pool.alloc(Pair { left: 1, right: 2 }, Some(span(0, 4)));
        let second = pool.alloc(Choice::Leaf(9), None);
        let third = pool.alloc(Pair { left: 3, right: 4 }, Some(span(4, 11)));
        assert_eq!(pool.span(first), Some(span(0, 4)));
        assert_eq!(pool.span(second), Some(Span::SYNTHETIC));
        assert_eq!(pool.span(third), Some(span(4, 11)));
        assert_eq!(*pool.get(first), Pair { left: 1, right: 2 });
        assert_eq!(*pool.get(second), Choice::Leaf(9));
        assert_eq!(*pool.get(third), Pair { left: 3, right: 4 });
    }

    #[test]
    fn the_shared_pool_reports_no_span_without_capture() {
        let mut pool = SharedPool::new(false);
        let handle = pool.alloc(Pair { left: 1, right: 2 }, Some(span(0, 4)));
        assert_eq!(pool.span(handle), None);
    }

    #[test]
    fn the_shared_pool_returns_none_for_a_handle_it_does_not_hold() {
        let mut pool = SharedPool::new(false);
        let _ = pool.alloc(Pair { left: 1, right: 2 }, None);
        let beyond = Handle::<Pair>::from_raw(NonZeroU32::new(4096).expect("non-zero"));
        assert_eq!(pool.try_get(beyond), None);
        assert_eq!(pool.span(beyond), None);
    }

    #[test]
    #[should_panic(expected = "does not hold the entry")]
    fn reading_a_shared_pool_handle_it_does_not_hold_panics() {
        let pool = SharedPool::new(false);
        let beyond = Handle::<Pair>::from_raw(NonZeroU32::new(4096).expect("non-zero"));
        let _ = pool.get(beyond);
    }

    #[test]
    fn an_empty_list_run_is_a_valid_handle_distinct_from_an_absent_list() {
        let mut store = ListStore::new();
        let empty = store.push(std::iter::empty::<Handle<Pair>>());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(store.get(empty).is_empty());
        assert!(store[empty].is_empty());
        assert_eq!(store.len_slots(), 1);
        assert_eq!(Some(empty), Some(empty));
        assert!(Option::<List<Handle<Pair>>>::None.is_none());
    }

    #[test]
    fn the_list_store_returns_each_run_it_stored() {
        let mut store = ListStore::new();
        let first = Handle::<Pair>::from_index(0);
        let second = Handle::<Pair>::from_index(1);
        let third = Handle::<Pair>::from_index(2);
        let left = store.push([first, second]);
        let right = store.push([third]);
        assert_eq!(left.len(), 2);
        assert_eq!(store.get(left), &[first, second]);
        assert_eq!(&store[right], &[third]);
        assert_eq!(store.len_slots(), 4);
    }

    #[test]
    fn push1_stores_a_run_whose_length_cannot_be_zero() {
        let mut store = ListStore::new();
        let first = Handle::<Pair>::from_index(0);
        let second = Handle::<Pair>::from_index(1);
        let list = store.push1(first, [second]);
        assert_eq!(list.len(), 1 + 1);
        assert!(!list.is_empty());
        assert_eq!(store.get1(list), &[first, second]);
        assert_eq!(&store[list], &[first, second]);
    }

    #[test]
    fn a_one_or_more_list_converts_to_a_zero_or_more_list() {
        let mut store = ListStore::new();
        let item = Handle::<Pair>::from_index(0);
        let list = store.push1(item, []);
        assert_eq!(store.get(list.as_list()), &[item]);
        assert_eq!(store.get(List::from(list)), &[item]);
    }

    #[test]
    fn a_text_handle_run_is_stored_like_a_node_handle_run() {
        let mut store = ListStore::new();
        let first = Text::<NameText>::from_index(0);
        let second = Text::<NameText>::from_index(1);
        let list = store.push([first, second]);
        assert_eq!(store.get(list), &[first, second]);
    }

    #[test]
    #[should_panic(expected = "does not hold the entry")]
    fn reading_a_list_run_the_store_does_not_hold_panics() {
        let store = ListStore::new();
        let beyond = List::<Handle<Pair>>::new(NonZeroU32::new(9).expect("non-zero"), 2);
        let _ = store.get(beyond);
    }

    #[test]
    fn nested_scratch_lists_close_independently() {
        let mut scratch = ListScratch::new();
        let mut store = ListStore::new();
        let a = Handle::<Pair>::from_index(0);
        let b = Handle::<Pair>::from_index(1);
        let c = Handle::<Pair>::from_index(2);
        let inner_item = Handle::<Pair>::from_index(3);

        let mut outer = scratch.open();
        scratch.push(&mut outer, a);
        scratch.push(&mut outer, b);

        let mut inner = scratch.open();
        scratch.push(&mut inner, inner_item);
        let inner_list = scratch.close1::<Handle<Pair>>(inner, &mut store);

        scratch.push(&mut outer, c);
        let outer_list = scratch.close::<Handle<Pair>>(outer, &mut store);

        assert_eq!(store.get1(inner_list), &[inner_item]);
        assert_eq!(store.get(outer_list), &[a, b, c]);
        assert!(scratch.is_empty());
        assert_eq!(scratch.len(), 0);
    }

    #[test]
    fn a_scratch_list_closed_empty_stores_an_empty_run() {
        let mut scratch = ListScratch::new();
        let mut store = ListStore::new();
        let list = scratch.open();
        let closed = scratch.close::<Handle<Pair>>(list, &mut store);
        assert!(closed.is_empty());
        assert!(scratch.is_empty());
    }

    #[test]
    #[should_panic(expected = "close1 closed an empty list")]
    fn close1_rejects_an_empty_run() {
        let mut scratch = ListScratch::new();
        let mut store = ListStore::new();
        let list = scratch.open();
        let _ = scratch.close1::<Handle<Pair>>(list, &mut store);
    }

    #[test]
    fn the_text_pool_stores_borrowed_and_owned_text() {
        let source = "alpha beta";
        let mut pool = TextPool::new(true);
        let borrowed = ContentText::matched(source, span(0, 5)).expect("the range is valid UTF-8");
        let alpha: Text<NameText> = pool.push(borrowed, NAME_KIND, Some(span(0, 5)));
        let beta: Text<NameText> = pool.push(ContentText::owned("beta".into()), NAME_KIND, None);

        assert_eq!(pool[alpha].as_str(), "alpha");
        assert_eq!(pool[beta].as_str(), "beta");
        assert!(std::ptr::eq(pool[alpha].as_str().as_ptr(), source.as_ptr()));
        assert_eq!(pool.get(alpha).map(ContentText::as_str), Some("alpha"));
        assert_eq!(pool.len(), 2);
        assert!(!pool.is_empty());
        assert_eq!(pool.get(Text::<NameText>::from_index(2)), None);
    }

    #[test]
    fn the_text_pool_reports_spans_only_when_capturing() {
        let mut capturing = TextPool::new(true);
        let first: Text<NameText> =
            capturing.push(ContentText::owned("x".into()), NAME_KIND, Some(span(0, 1)));
        let second: Text<NameText> =
            capturing.push(ContentText::owned("y".into()), NAME_KIND, None);
        assert_eq!(capturing.span(first), Some(span(0, 1)));
        assert_eq!(capturing.span(second), Some(Span::SYNTHETIC));

        let mut plain = TextPool::new(false);
        let third: Text<NameText> =
            plain.push(ContentText::owned("z".into()), NAME_KIND, Some(span(0, 1)));
        assert_eq!(plain.span(third), None);
    }

    #[test]
    fn the_text_pool_keeps_the_lexical_kind_of_every_token() {
        let mut pool = TextPool::new(false);
        let name: Text<NameText> = pool.push(ContentText::owned("x".into()), NAME_KIND, None);
        let keyword: Text<NameText> =
            pool.push(ContentText::owned("as".into()), KEYWORD_KIND, None);
        assert_eq!(pool.kind(name), Some(NAME_KIND));
        assert_eq!(pool.kind(keyword), Some(KEYWORD_KIND));
        assert_eq!(pool[keyword].as_str(), "as");
        assert_eq!(pool.kind(Text::<NameText>::from_index(2)), None);
    }

    #[test]
    #[should_panic(expected = "does not hold the entry")]
    fn indexing_a_text_pool_out_of_range_panics() {
        let pool = TextPool::new(false);
        let _ = &pool[Text::<NameText>::from_index(0)];
    }

    #[test]
    fn a_side_table_covers_its_pool_and_grows_with_it() {
        let mut pool = TypedPool::new(false);
        let first = pool.push(Pair { left: 1, right: 1 }, None);
        let mut table = SideTable::<Pair, u32>::for_pool(&pool, 0);
        assert_eq!(table.len(), 1);
        assert!(!table.is_empty());

        table[first] = 7;
        let second = pool.push(Pair { left: 2, right: 2 }, None);
        table.grow(&pool, 0);
        assert_eq!(table.len(), 2);
        assert_eq!(table[first], 7);
        assert_eq!(table[second], 0);

        table.grow(&pool, 5);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_side_table_can_be_built_for_a_known_length() {
        let table = SideTable::<Pair, bool>::new(3, false);
        assert_eq!(table.len(), 3);
        assert!(!table[Handle::from_index(2)]);
        assert!(SideTable::<Pair, bool>::new(0, false).is_empty());
    }

    #[test]
    #[should_panic(expected = "does not hold the entry")]
    fn indexing_a_side_table_out_of_range_panics() {
        let table = SideTable::<Pair, u32>::new(1, 0);
        let _ = table[Handle::from_index(1)];
    }

    #[test]
    #[should_panic(expected = "does not hold the entry")]
    fn writing_a_side_table_out_of_range_panics() {
        let mut table = SideTable::<Pair, u32>::new(1, 0);
        table[Handle::from_index(1)] = 1;
    }

    #[test]
    fn every_flat_storage_type_is_send_and_sync_whatever_the_node_type() {
        assert_send_sync::<Handle<Cell<u8>>>();
        assert_send_sync::<Option<Handle<Cell<u8>>>>();
        assert_send_sync::<Text<Cell<u8>>>();
        assert_send_sync::<List<Handle<Cell<u8>>>>();
        assert_send_sync::<List1<Handle<Cell<u8>>>>();
        assert_send_sync::<SideTable<Cell<u8>, u32>>();
        assert_send_sync::<OpenList>();
        assert_send_sync::<ListScratch>();
        assert_send_sync::<ListStore>();
        assert_send_sync::<SharedPool>();
        assert_send_sync::<TypedPool<Pair>>();
        assert_send_sync::<TextPool<'static>>();
    }
}
