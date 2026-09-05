//! The type-erased value stack of the table-driven parser.
//!
//! A reduce action pops the values of its rule's value-carrying symbols and pushes the value it
//! constructs; the automaton loop never inspects a value. Every value therefore lives as raw
//! bytes in one buffer, with a parallel drop stack that remembers where each value starts and
//! how to drop it, so that a failed or abandoned parse can release every partially built value
//! without knowing its type. Fixed tokens never reach this stack (section 6 of
//! `docs/table-driven-parsing.md`): only the state stack sees them.
//!
//! # Safety model
//!
//! The buffer is a vector of 16-byte aligned units, so a value whose alignment is at most 16 can
//! be stored at an offset rounded up to its alignment and read back with an aligned pointer.
//! [`ValueStack::push`] is safe: it copies the value's bytes in and records its drop glue.
//! [`ValueStack::pop`] is `unsafe` because the stack cannot check that the requested type is the
//! type that was pushed; the generated reduce actions guarantee it because both sides are derived
//! from the same grammar rule. A size mismatch is caught by a debug assertion.

use std::mem::{ManuallyDrop, align_of, size_of};
use std::ptr;

/// The maximum alignment a stacked value may have; every unit of the buffer is aligned to it.
const UNIT_ALIGN: usize = 16;

#[derive(Clone, Copy)]
#[repr(C, align(16))]
/// One aligned unit of the byte buffer.
struct Unit([u8; UNIT_ALIGN]);

/// Drop glue for one stacked value, called with the aligned pointer to its bytes.
type DropFn = unsafe fn(*mut u8);

#[derive(Clone, Copy)]
/// One entry of the drop stack: where a value's slot starts, where its bytes begin, and how to
/// drop it.
struct Slot {
    /// Byte length of the buffer before the value was pushed; popping truncates to it.
    start: usize,
    /// Aligned byte offset of the value.
    offset: usize,
    /// Size of the value in bytes, checked on pop in debug builds.
    size: usize,
    /// Drop glue for the value.
    drop: DropFn,
}

/// The value stack: a byte buffer plus a parallel drop stack.
///
/// Dropping the stack drops every value still on it, so a parse that fails or panics after
/// pushing values leaks nothing.
pub struct ValueStack {
    /// Aligned storage; only the first `len` bytes hold values.
    units: Vec<Unit>,
    /// Bytes of `units` in use.
    len: usize,
    /// One entry per stacked value, bottom first.
    slots: Vec<Slot>,
}

impl Default for ValueStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueStack {
    /// Creates an empty stack.
    pub const fn new() -> Self {
        Self {
            units: Vec::new(),
            len: 0,
            slots: Vec::new(),
        }
    }

    /// Returns the number of values on the stack.
    pub fn depth(&self) -> usize {
        self.slots.len()
    }

    /// Pushes one value, taking ownership of it.
    ///
    /// Values with an alignment above 16 are refused at compile time.
    pub fn push<V>(&mut self, value: V) {
        const { assert!(align_of::<V>() <= UNIT_ALIGN, "value alignment exceeds the stack unit") };
        let start = self.len;
        let offset = start.next_multiple_of(align_of::<V>().max(1));
        let end = offset + size_of::<V>();
        let needed_units = end.div_ceil(UNIT_ALIGN);
        if self.units.len() < needed_units {
            self.units.resize(needed_units, Unit([0; UNIT_ALIGN]));
        }
        let value = ManuallyDrop::new(value);
        // SAFETY: `offset + size_of::<V>()` lies inside the initialized units, `offset` is a
        // multiple of `align_of::<V>()` on a 16-aligned buffer, and the destination is not
        // aliased by any live reference; `value` is not dropped again because it is wrapped in
        // `ManuallyDrop`.
        unsafe {
            let destination = self.units.as_mut_ptr().cast::<u8>().add(offset).cast::<V>();
            ptr::write(destination, ptr::read(&*value));
        }
        self.slots.push(Slot {
            start,
            offset,
            size: size_of::<V>(),
            drop: drop_value::<V>,
        });
        self.len = end;
    }

    /// Pops the top value as `V`.
    ///
    /// # Safety
    ///
    /// `V` must be the type the top value was pushed with. The generated reduce actions satisfy
    /// this because the push in one rule's action and the pop in its parent's action are derived
    /// from the same grammar symbol.
    pub unsafe fn pop<V>(&mut self) -> V {
        let slot = self
            .slots
            .pop()
            .expect("value stack underflow: a reduce action popped more values than its rule holds");
        debug_assert_eq!(
            slot.size,
            size_of::<V>(),
            "value stack type mismatch: the popped type has a different size from the pushed type"
        );
        // SAFETY: the slot was recorded by `push` for a value of type `V` (the caller's
        // guarantee), whose bytes are still initialized at `slot.offset` because only `pop` and
        // `unwind` release slots and each releases exactly its own. After the read the bytes are
        // logically moved out, and truncating `len` to `slot.start` forgets them.
        let value = unsafe {
            let source = self.units.as_ptr().cast::<u8>().add(slot.offset).cast::<V>();
            ptr::read(source)
        };
        self.len = slot.start;
        value
    }

    /// Drops every value on the stack, top first, and empties it.
    pub fn unwind(&mut self) {
        while let Some(slot) = self.slots.pop() {
            // SAFETY: the slot's bytes are still initialized (see `pop`), `slot.drop` is the
            // drop glue of the type that was pushed, and the pointer is aligned for that type.
            unsafe {
                let pointer = self.units.as_mut_ptr().cast::<u8>().add(slot.offset);
                (slot.drop)(pointer);
            }
            self.len = slot.start;
        }
    }
}

impl Drop for ValueStack {
    fn drop(&mut self) {
        self.unwind();
    }
}

/// Drops the `V` stored at an aligned pointer.
unsafe fn drop_value<V>(pointer: *mut u8) {
    // SAFETY: the caller passes the aligned pointer `push` recorded for a value of type `V` that
    // has not been popped.
    unsafe { ptr::drop_in_place(pointer.cast::<V>()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A value that counts its drops.
    struct Counted(Rc<Cell<usize>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn push_and_pop_round_trip_values_of_mixed_sizes_and_alignments() {
        let mut stack = ValueStack::new();
        stack.push(7_u8);
        stack.push(String::from("hello"));
        stack.push(0x1234_5678_9abc_def0_u64);
        stack.push(());
        stack.push([1_u8, 2, 3]);
        assert_eq!(stack.depth(), 5);
        // SAFETY: the pops mirror the pushes in reverse order.
        unsafe {
            assert_eq!(stack.pop::<[u8; 3]>(), [1, 2, 3]);
            stack.pop::<()>();
            assert_eq!(stack.pop::<u64>(), 0x1234_5678_9abc_def0);
            assert_eq!(stack.pop::<String>(), "hello");
            assert_eq!(stack.pop::<u8>(), 7);
        }
        assert_eq!(stack.depth(), 0);
        assert_eq!(stack.len, 0);
    }

    #[test]
    fn popped_values_are_not_dropped_by_the_stack() {
        let drops = Rc::new(Cell::new(0));
        let mut stack = ValueStack::new();
        stack.push(Counted(drops.clone()));
        // SAFETY: the pop mirrors the push.
        let value = unsafe { stack.pop::<Counted>() };
        assert_eq!(drops.get(), 0);
        drop(value);
        assert_eq!(drops.get(), 1);
        drop(stack);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn unwind_drops_every_value_top_first_and_drop_drops_the_rest() {
        let order = Rc::new(std::cell::RefCell::new(Vec::new()));
        struct Ordered(Rc<std::cell::RefCell<Vec<u32>>>, u32);
        impl Drop for Ordered {
            fn drop(&mut self) {
                self.0.borrow_mut().push(self.1);
            }
        }
        let mut stack = ValueStack::new();
        stack.push(Ordered(order.clone(), 1));
        stack.push(String::from("between"));
        stack.push(Ordered(order.clone(), 2));
        stack.unwind();
        assert_eq!(*order.borrow(), [2, 1]);
        assert_eq!(stack.depth(), 0);

        stack.push(Ordered(order.clone(), 3));
        drop(stack);
        assert_eq!(*order.borrow(), [2, 1, 3]);
    }

    #[test]
    fn the_buffer_grows_across_many_pushes_and_alignment_padding_is_reclaimed() {
        let mut stack = ValueStack::new();
        for index in 0..1000_u32 {
            stack.push(index as u8);
            stack.push(u64::from(index));
        }
        // SAFETY: the pops mirror the pushes in reverse order.
        unsafe {
            for index in (0..1000_u32).rev() {
                assert_eq!(stack.pop::<u64>(), u64::from(index));
                assert_eq!(stack.pop::<u8>(), index as u8);
            }
        }
        assert_eq!(stack.len, 0);
    }
}
