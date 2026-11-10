#[cfg(feature = "alloc")]
use alloc::{collections::{BTreeMap, VecDeque}, vec::Vec, vec};

#[cfg(feature = "deku")]
use deku::{no_std_io::{Read, Seek, Write}, prelude::*};

use crate::{Delta, Diff, Patch};

// TODO reversable delta trait

/// A delta which represents item changes in a vector or slice
// TODO create a variant of [`SliceDelta`] for types like structs which may contain
// large enough items to diff rather than cloning.
#[cfg(feature = "alloc")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SliceDelta<T> {
    /// The non-overlapping splices expressed in
    /// the coordinate system of the vector before
    /// the delta is applied.
    entries: BTreeMap<u32, SliceDeltaEntry<T>>,
}

#[cfg(feature = "alloc")]
impl<T> Default for SliceDelta<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

#[cfg(feature = "alloc")]
impl<T> SliceDelta<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `next` into the entry map, merging it with any entries whose
    /// ranges it touches or overlaps.
    ///
    /// `next` must already be expressed in the same coordinate space as the
    /// rest of `entries` (i.e. positions in the pre-delta vector) — this is
    /// *not* the method that translates a "next" [`Delta`]'s B-space
    /// coordinates into A-space; that translation happens in
    /// [`Delta::aggregate`].
    ///
    /// Entries that merely *touch* `next` (no gap between them) are
    /// concatenated, which is lossless. Entries that genuinely *overlap*
    /// `next`'s range are dropped in favor of `next`, since a `Vec<T>` of
    /// added items carries no addressable sub-range information to resolve
    /// a partial conflict — `next` is treated as authoritative for any
    /// range it claims.
    pub fn aggregate_entry(&mut self, mut next: SliceDeltaEntry<T>) {
        // at most one existing entry can touch/overlap `next` from the left
        // (entries are non-overlapping, so the immediate predecessor is the
        // only candidate).
        let lower = self
            .entries
            .range(..next.start_index)
            .next_back()
            .filter(|(_, e)| e.remove_end() >= next.start_index)
            .map(|(&k, _)| k);

        if let Some(k) = lower {
            let prev = self.entries.remove(&k).unwrap();
            next = merge_two(prev, next);
        }

        // any number of entries can touch/overlap from the right, so keep
        // absorbing until nothing else qualifies.
        loop {
            let upper = self
                .entries
                .range(next.start_index..)
                .next()
                .filter(|(_, e)| e.start_index <= next.remove_end())
                .map(|(&k, _)| k);

            match upper {
                Some(k) => {
                    let other = self.entries.remove(&k).unwrap();
                    next = merge_two(next, other);
                }
                None => break,
            }
        }

        self.entries.insert(next.start_index, next);
    }
}

#[cfg(all(feature = "alloc", feature = "deku"))]
impl<T, Ctx> DekuWriter<Ctx> for SliceDelta<T>
where
    Ctx: Copy,
    u32: DekuWriter<Ctx>,
    SliceDeltaEntry<T>: DekuWriter<Ctx>,
{
    fn to_writer<W: Write + Seek>(
        &self,
        writer: &mut Writer<W>,
        ctx: Ctx,
    ) -> Result<(), DekuError> {
        // length prefix
        (self.entries.len() as u32).to_writer(writer, ctx)?;
        
        for entry in self.entries.values() {
            entry.to_writer(writer, ctx)?;
        }
        
        Ok(())
    }
}

#[cfg(all(feature = "alloc", feature = "deku"))]
impl<'a, T, Ctx> DekuReader<'a, Ctx> for SliceDelta<T>
where
    Ctx: Copy,
    u32: DekuReader<'a, Ctx>,
    SliceDeltaEntry<T>: DekuReader<'a, Ctx>,
{
    fn from_reader_with_ctx<R: Read + Seek>(
        reader: &mut Reader<R>,
        ctx: Ctx,
    ) -> Result<Self, DekuError> {
        // length prefix
        let count = u32::from_reader_with_ctx(reader, ctx)? as usize;
        
        // realistically, a client-side reader wouldn't even need to build a B-tree,
        // so it could avoid this overhead, but I have to implement it here for symmetry.
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            let entry = SliceDeltaEntry::<T>::from_reader_with_ctx(reader, ctx)?;
            entries.insert(entry.start_index, entry);
        }
        
        Ok(Self { entries })
    }
}

/// Merges two entries known to be touching or overlapping, with `a` sorted
/// before `b` (`a.start_index <= b.start_index`). See [`SliceDelta::aggregate_entry`]
/// for the semantics when they genuinely overlap.
#[cfg(feature = "alloc")]
fn merge_two<T>(a: SliceDeltaEntry<T>, b: SliceDeltaEntry<T>) -> SliceDeltaEntry<T> {
    debug_assert!(a.start_index <= b.start_index);
    let start = a.start_index;
    let remove_end = a.remove_end().max(b.remove_end());
    let mut add = a.add;
    add.extend(b.add);
    SliceDeltaEntry {
        start_index: start,
        remove: remove_end - start,
        add,
    }
}

/// One piece of the vector produced by a `SliceDelta`, used only internally
/// to compose two deltas. Either a run of items carried over unchanged from
/// the pre-delta vector (identified by its range there), or a run of items
/// that were literally inserted and therefore don't exist in the pre-delta
/// vector at all.
#[cfg(feature = "alloc")]
enum Segment<T> {
    /// `a_end == None` means "extends to the end of the vector" — this lets
    /// us represent the untouched tail without ever knowing the vector's
    /// actual length.
    Unchanged { a_start: u32, a_end: Option<u32> },
    Added(Vec<T>),
}

#[cfg(feature = "alloc")]
impl<T> Segment<T> {
    fn len(&self) -> Option<u32> {
        match self {
            Segment::Unchanged { a_start, a_end } => a_end.map(|e| e - a_start),
            Segment::Added(items) => Some(items.len() as u32),
        }
    }
}

/// Discards exactly `want` B-space positions from the front of `segments`,
/// splitting the boundary segment if needed.
#[cfg(feature = "alloc")]
fn take_front<T: Clone>(segments: &mut VecDeque<Segment<T>>, mut want: u32) {
    while want > 0 {
        let seg = segments
            .front_mut()
            .expect("next's entries remove more content than is available");
        match seg.len() {
            Some(avail) if avail <= want => {
                want -= avail;
                segments.pop_front();
            }
            _ => {
                match seg {
                    Segment::Unchanged { a_start, .. } => *a_start += want,
                    Segment::Added(items) => {
                        items.drain(0..want as usize);
                    }
                }
                want = 0;
            }
        }
    }
}

/// Moves exactly `want` B-space positions' worth of segments from the front
/// of `segments` onto the back of `output`, splitting the boundary segment
/// if needed.
#[cfg(feature = "alloc")]
fn advance_to<T: Clone>(segments: &mut VecDeque<Segment<T>>, output: &mut Vec<Segment<T>>, mut want: u32) {
    while want > 0 {
        let avail = segments
            .front()
            .expect("next's entries start past the end of the aggregated vector")
            .len();
        match avail {
            Some(avail) if avail <= want => {
                want -= avail;
                output.push(segments.pop_front().unwrap());
            }
            _ => {
                let front = segments.front_mut().unwrap();
                match front {
                    Segment::Unchanged { a_start, .. } => {
                        let split = *a_start + want;
                        output.push(Segment::Unchanged {
                            a_start: *a_start,
                            a_end: Some(split),
                        });
                        *a_start = split;
                    }
                    Segment::Added(items) => {
                        let head: Vec<T> = items.drain(0..want as usize).collect();
                        output.push(Segment::Added(head));
                    }
                }
                want = 0;
            }
        }
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone> Delta for SliceDelta<T> {
    fn aggregate(&mut self, next: &Self) {
        self.aggregate_owned(next.clone());
    }

    fn aggregate_owned(&mut self, next: Self) {
        // Step 1: express B as segments over self's own (owned) entries.
        let mut segments: VecDeque<Segment<T>> = VecDeque::new();
        let mut cursor_a = 0u32;
        for entry in core::mem::take(&mut self.entries).into_values() {
            if entry.start_index > cursor_a {
                segments.push_back(Segment::Unchanged {
                    a_start: cursor_a,
                    a_end: Some(entry.start_index),
                });
            }
            cursor_a = entry.remove_end();
            if !entry.add.is_empty() {
                segments.push_back(Segment::Added(entry.add)); // moved
            }
        }
        segments.push_back(Segment::Unchanged {
            a_start: cursor_a,
            a_end: None,
        });

        // Step 2: sweep next's (owned) entries across those segments.
        let mut output: Vec<Segment<T>> = Vec::new();
        let mut b_cursor = 0u32;
        for entry in next.entries.into_values() {
            advance_to(&mut segments, &mut output, entry.start_index - b_cursor);
            take_front(&mut segments, entry.remove);
            b_cursor = entry.remove_end(); // compute first, while `entry` is intact
            if !entry.add.is_empty() {
                output.push(Segment::Added(entry.add)); // now safe to move
            }
        }
        output.extend(segments);

        // Step 3: translate the merged segment list back into A-space entries.
        let mut new_entries = BTreeMap::new();
        let mut next_expected_a = 0u32;
        let mut add_buffer: Vec<T> = Vec::new();
        for segment in output {
            match segment {
                Segment::Added(items) => add_buffer.extend(items),
                Segment::Unchanged { a_start, a_end } => {
                    if a_start != next_expected_a || !add_buffer.is_empty() {
                        let entry = SliceDeltaEntry::new(
                            next_expected_a,
                            a_start - next_expected_a,
                            core::mem::take(&mut add_buffer),
                        );
                        new_entries.insert(entry.start_index, entry);
                    }
                    next_expected_a = a_end.unwrap_or(a_start);
                }
            }
        }

        self.entries = new_entries;
    }
}

/// Computes a [`SliceDelta`] representing the changes needed to turn
/// `initial` into `final_`, via LCS-based diffing. Shared between the
/// `Vec<T>` and `[T]` [`Diff`] impls — diffing itself doesn't
/// care whether the result will later be applied to a resizable or
/// fixed-size container; only `patch` differs between them.
#[cfg(feature = "alloc")]
fn diff_slices<T: Clone + PartialEq>(initial: &[T], final_: &[T]) -> SliceDelta<T> {
    let n = initial.len();
    let m = final_.len();

    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if initial[i] == final_[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut entries = BTreeMap::new();
    let (mut i, mut j) = (0usize, 0usize);
    let mut run_start: Option<usize> = None;
    let mut run_add: Vec<T> = Vec::new();
    let mut run_remove = 0u32;

    macro_rules! flush {
        () => {
            #[allow(unused_assignments)]
            {
                if let Some(start) = run_start.take() {
                    if run_remove != 0 || !run_add.is_empty() {
                        let entry = SliceDeltaEntry::new(
                            start as u32,
                            run_remove,
                            core::mem::take(&mut run_add),
                        );
                        entries.insert(entry.start_index, entry);
                    }
                    run_remove = 0;
                }
            }
        };
    }

    while i < n && j < m {
        if initial[i] == final_[j] {
            flush!();
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            run_start.get_or_insert(i);
            run_remove += 1;
            i += 1;
        } else {
            run_start.get_or_insert(i);
            run_add.push(final_[j].clone());
            j += 1;
        }
    }
    if i < n {
        run_start.get_or_insert(i);
        run_remove += (n - i) as u32;
    }
    if j < m {
        run_start.get_or_insert(n);
        run_add.extend(final_[j..].iter().cloned());
    }
    flush!();

    SliceDelta { entries }
}

#[cfg(feature = "alloc")]
impl<T: Clone + PartialEq> Diff<SliceDelta<T>> for Vec<T> {
    fn diff(&self, initial: &Self) -> SliceDelta<T> {
        diff_slices(initial, self)
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone> Patch<SliceDelta<T>> for Vec<T> {
    fn patch(&mut self, delta: &SliceDelta<T>) {
        for entry in delta.entries.values().rev() {
            let start = entry.start_index as usize;
            let end = start + entry.remove as usize;

            self.splice(start..end, entry.add.iter().cloned());
        }
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone + PartialEq> Diff<SliceDelta<T>> for [T] {
    fn diff(&self, initial: &Self) -> SliceDelta<T> {
        diff_slices(initial, self)
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone + PartialEq> Patch<SliceDelta<T>> for [T] {
    fn patch(&mut self, delta: &SliceDelta<T>) {
        let mut buffer: Vec<T> = Vec::with_capacity(self.len());
        let mut read = 0usize;

        for entry in delta.entries.values() {
            let start = entry.start_index as usize;
            buffer.extend_from_slice(&self[read..start]);
            read = start + entry.remove as usize;
            buffer.extend(entry.add.iter().cloned());
        }

        buffer.extend_from_slice(&self[read..]);

        assert!(
            buffer.len() <= self.len(),
            "patch would increase the length of the slice"
        );

        self[..buffer.len()].clone_from_slice(&buffer);
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone + PartialEq, const N: usize> Diff<SliceDelta<T>> for [T; N] {
    fn diff(&self, initial: &Self) -> SliceDelta<T> {
        <[T] as Diff<SliceDelta<T>>>::diff(self, initial)
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone + PartialEq, const N: usize> Patch<SliceDelta<T>> for [T; N] {
    fn patch(&mut self, delta: &SliceDelta<T>) {
        <[T]>::patch(self, delta)
    }
}

/// A splicing entry in the delta
#[cfg(feature = "alloc")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SliceDeltaEntry<T> {
    /// The start index of the entry within the vec
    start_index: u32,

    /// The number of items removed as a result of the entry
    remove: u32,

    /// The items added as a result of the entry.
    /// When applied, these are added after the others are removed.
    add: Vec<T>,
}

#[cfg(feature = "alloc")]
impl<T> SliceDeltaEntry<T> {
    pub fn new(start_index: u32, remove: u32, add: Vec<T>) -> Self {
        assert!(remove != 0 || !add.is_empty());

        Self {
            start_index,
            remove,
            add,
        }
    }

    pub fn insert(start_index: u32, add: Vec<T>) -> Self {
        Self::new(start_index, 0, add)
    }

    pub fn remove(start_index: u32, remove: u32) -> Self {
        Self::new(start_index, remove, Vec::new())
    }

    pub fn replace(start_index: u32, new_values: Vec<T>) -> Self {
        Self::new(start_index, new_values.len() as u32, new_values)
    }

    pub const fn remove_end(&self) -> u32 {
        self.start_index + self.remove
    }

    pub const fn add_len(&self) -> u32 {
        self.add.len() as u32
    }

    pub const fn end_after(&self) -> u32 {
        self.start_index + self.add_len()
    }

    pub const fn delta_len(&self) -> i64 {
        self.add_len() as i64 - self.remove as i64
    }

    pub const fn contains_index(&self, index: u32) -> bool {
        index >= self.start_index && index < self.remove_end()
    }
}

#[cfg(all(feature = "alloc", feature = "deku"))]
impl<T, Ctx> DekuWriter<Ctx> for SliceDeltaEntry<T>
where
    Ctx: Copy,
    u32: DekuWriter<Ctx>,
    T: DekuWriter<Ctx>,
{
    fn to_writer<W: Write + Seek>(
        &self,
        writer: &mut Writer<W>,
        ctx: Ctx,
    ) -> Result<(), DekuError> {
        self.start_index.to_writer(writer, ctx)?;
        self.remove.to_writer(writer, ctx)?;
    
        (self.add.len() as u32).to_writer(writer, ctx)?;
        
        for item in &self.add {
            item.to_writer(writer, ctx)?;
        }
        
        Ok(())
    }
}

#[cfg(all(feature = "alloc", feature = "deku"))]
impl<'a, T, Ctx> DekuReader<'a, Ctx> for SliceDeltaEntry<T>
where
    Ctx: Copy,
    u32: DekuReader<'a, Ctx>,
    T: DekuReader<'a, Ctx>,
{
    fn from_reader_with_ctx<R: Read + Seek>(
        reader: &mut Reader<R>,
        ctx: Ctx,
    ) -> Result<Self, DekuError> {
        let start_index = u32::from_reader_with_ctx(reader, ctx)?;
        let remove = u32::from_reader_with_ctx(reader, ctx)?;
        
        let add_count = u32::from_reader_with_ctx(reader, ctx)? as usize;
        
        let mut add = Vec::with_capacity(add_count);
        for _ in 0..add_count {
            add.push(T::from_reader_with_ctx(reader, ctx)?);
        }
        
        Ok(Self {
            start_index,
            remove,
            add,
        })
    }
}

/// A delta defined by addition/subtraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArithmeticDelta<T: WrappingArithmetic>(pub T);

impl<T> Delta for ArithmeticDelta<T>
where 
    T: WrappingArithmetic,
{
    fn aggregate(&mut self, next: &Self) {
        self.0 = self.0.wrapping_add(next.0);
    }
}

#[cfg(feature = "deku")]
impl<T, Ctx> DekuWriter<Ctx> for ArithmeticDelta<T>
where 
    T: WrappingArithmetic + DekuWriter<Ctx>,
{
    fn to_writer<W: Write+Seek>(&self, writer: &mut Writer<W>, ctx: Ctx,) -> Result<(), DekuError> {
        self.0.to_writer(writer, ctx)
    }
}

#[cfg(feature = "deku")]
impl<'a, T, Ctx> DekuReader<'a, Ctx> for ArithmeticDelta<T>
where 
    T: WrappingArithmetic + DekuReader<'a, Ctx>,
{
    fn from_reader_with_ctx<R: Read + Seek>(reader: &mut Reader<R>, ctx: Ctx,) -> Result<Self, DekuError>
    where
        Self: Sized {
        T::from_reader_with_ctx(reader, ctx).map(Self)
    }
}

/// A trait that implements wrapping (overflow) addition and subtraction.
/// It is primarily used within [`ArithmeticDelta`] for generics.
/// With standard library integers like [`i32`], these methods are identical to the provided
/// [`wrapping_add`][i32::wrapping_add] and [`wrapping_sub`][i32::wrapping_sub] definitions for those types
/// (`std` doesn't provide traits for overflow operations for some reason; only methods).
pub trait WrappingArithmetic: Copy {
    /// Add `rhs` to `Self`, returning a value wrapped
    /// about the maximum and minimum values for this type.
    fn wrapping_add(self, rhs: Self) -> Self;
    
    /// Subtract `rhs` from `Self`, returning a value
    /// wrapped about the maximum and minimum values for this type.
    fn wrapping_sub(self, rhs: Self) -> Self;
}

macro_rules! impl_wrapping_arith {
    ($T: ty) => {
        impl WrappingArithmetic for $T {
            fn wrapping_add(self, rhs: $T) -> $T {
                <$T>::wrapping_add(self, rhs)
            }

            fn wrapping_sub(self, rhs: $T) -> $T {
                <$T>::wrapping_sub(self, rhs)
            }
        }
    };
    ($($T: ty),*) => {
        $(impl_wrapping_arith!($T);)*
    };
}

impl_wrapping_arith! {
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize
}

impl<T: WrappingArithmetic> Diff<ArithmeticDelta<Self>> for T
{
    fn diff(&self, initial: &Self) -> ArithmeticDelta<Self> {
        ArithmeticDelta(self.wrapping_sub(*initial))
    }
}

impl<T: WrappingArithmetic> Patch<ArithmeticDelta<Self>> for T
{
    fn patch(&mut self, delta: &ArithmeticDelta<Self>) {
        *self = self.wrapping_add(delta.0);
    }
}

// TODO generic additive (or scalar?) delta for types which don't care about overflows.

#[cfg(test)]
mod tests {
    use super::*;

    // Note: `D` intentionally has no `Delta` bound — `Diff`/`Patch` work with
    // any delta type, including ones that don't support aggregation.
    fn assert_roundtrip<T, D>(initial: T, final_: T)
    where 
        T: Diff<D> + Clone + PartialEq + core::fmt::Debug,
    {
        let delta = final_.diff(&initial);
        let mut applied = initial.clone();
        applied.patch(&delta);
        assert_eq!(applied, final_);
    }

    /// Tests that `Diff`/`Patch` can be implemented for a delta type that
    /// does not implement `Delta` (a "specific-usecase delta").
    mod without_delta {
        use super::*;

        /// A delta that only records whether a `bool` changed. It has no
        /// meaningful `aggregate` implementation, so it does not implement `Delta`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct Flip(bool);

        impl Diff<Flip> for bool {
            fn diff(&self, initial: &Self) -> Flip {
                Flip(self != initial)
            }
        }

        impl Patch<Flip> for bool {
            fn patch(&mut self, delta: &Flip) {
                if delta.0 {
                    *self = !*self;
                }
            }
        }

        #[test]
        fn diff_and_apply() {
            assert_roundtrip::<bool, Flip>(true, false);
            assert_roundtrip::<bool, Flip>(false, true);
            assert_roundtrip::<bool, Flip>(true, true);
            assert_roundtrip::<bool, Flip>(false, false);
        }
    }

    #[cfg(feature = "alloc")]
    mod slice {
        use super::*;

        #[test]
        fn diff_and_apply() {
            assert_roundtrip(vec![1, 2, 3, 4], vec![1, 5, 3, 6, 4]);
            assert_roundtrip(vec![1, 2, 3], vec![]);
            assert_roundtrip(vec![], vec![1, 2, 3]);
            assert_roundtrip(vec![1, 2, 3], vec![1, 2, 3]);
        }

        #[test]
        fn aggregate_matches_direct_diff() {
            let a = vec![1, 2, 3, 4, 5];
            let b = vec![1, 9, 3, 4, 10, 5];
            let c = vec![9, 3, 11, 4, 10];

                        let mut d1 = b.diff(&a);
            let d2 = c.diff(&b);
            d1.aggregate(&d2);

            let mut applied = a.clone();
            applied.patch(&d1);
            assert_eq!(applied, c);
        }

        #[test]
        fn aggregate_entry_merges_touching_ranges() {
            let mut delta = SliceDelta::<i32>::new();
            delta.aggregate_entry(SliceDeltaEntry::replace(0, vec![1, 2]));
            delta.aggregate_entry(SliceDeltaEntry::replace(2, vec![3]));

            assert_eq!(delta.entries.len(), 1);
            let entry = delta.entries.values().next().unwrap();
            assert_eq!(entry.start_index, 0);
            assert_eq!(entry.add, vec![1, 2, 3]);
        }

        #[test]
        fn slice_patch_same_length() {
                        let initial = [1, 2, 3, 4];
            let final_ = vec![1, 5, 3, 6];
            let delta = final_.diff(&initial.to_vec());

            let mut buf = initial;
            buf.patch(&delta);
            assert_eq!(buf, [1, 5, 3, 6]);
        }

        #[test]
        fn slice_patch_shrink_ok() {
            let initial = [1, 2, 3, 4, 5];
                        let final_ = vec![1, 3, 5];
            let delta = final_.diff(&initial.to_vec());

            let mut buf = initial;
            buf.patch(&delta);
            assert_eq!(&buf[..3], [1, 3, 5]); // trailing elements are stale, not checked
        }

        #[test]
        #[should_panic(expected = "increase the length")]
        fn slice_patch_growth_panics() {
            let initial = [1, 2, 3];
                        let final_ = vec![1, 2, 3, 4];
            let delta = final_.diff(&initial.to_vec());

            let mut buf = initial;
            buf.patch(&delta);
        }

        #[test]
        fn slice_patch_permutation_no_extra_space() {
            let initial: [u8; 10] = [4, 5, 6, 7, 8, 9, 10, 1, 2, 3];
                        let final_ = vec![8, 1, 5, 2, 3, 6, 7, 9, 10, 4];
            let delta = final_.diff(&initial.to_vec());
            let mut buf = initial;
            buf.patch(&delta); // previously panicked with the compaction version
            assert_eq!(buf, [8, 1, 5, 2, 3, 6, 7, 9, 10, 4]);
        }
    }
    
        mod arithmetic {
        use super::*;

        #[test]
        fn diff_and_apply() {
                    assert_roundtrip(5u32, 10u32);
            assert_roundtrip(-1i32, i32::MAX);
            assert_roundtrip(i32::MIN, i32::MAX);
            assert_roundtrip(u8::MAX, u8::MIN);
            // assert_roundtrip(2.34, 6.78);
            // assert_roundtrip(28930.345f64, 28930.345f64);
        }
    }

        /// Tests for `#[derive(Diff)]` on enums.
    #[cfg(feature = "macros")]
    mod derive_enum {
        use super::*;
        use deltapatcher_macros::Diff;
        // The macro emits `deltapatcher::delta::...` paths.  When the tests
        // run inside the `deltapatcher` crate itself, we need the crate to
        // be accessible under that name.
        extern crate self as deltapatcher;

        // ----------------------------------------------------------------
        // Test types
        // ----------------------------------------------------------------

        /// An enum with every kind of variant (named fields, unnamed fields,
        /// and unit) so that one set of tests covers all three code paths.
        #[derive(Diff, Debug, Clone, PartialEq)]
        enum Shape {
            /// Named-field variant.
            Rect {
                #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
                width: i32,
                #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
                height: i32,
            },
            /// Unnamed-field (tuple) variant.
            Circle(
                #[deltapatcher(delta_ty = ArithmeticDelta<u32>)]
                u32,
            ),
            /// Unit variant.
            Point,
        }

        // ----------------------------------------------------------------
        // same-variant: diff + patch
        // ----------------------------------------------------------------

        #[test]
        fn same_variant_named_roundtrip() {
            assert_roundtrip(
                Shape::Rect { width: 10, height: 20 },
                Shape::Rect { width: 30, height: 5  },
            );
        }

        #[test]
        fn same_variant_unnamed_roundtrip() {
            assert_roundtrip(Shape::Circle(4), Shape::Circle(100));
        }

        #[test]
        fn same_variant_unit_roundtrip() {
            assert_roundtrip(Shape::Point, Shape::Point);
        }

        // ----------------------------------------------------------------
        // cross-variant: diff produces Replace, patch overwrites
        // ----------------------------------------------------------------

        #[test]
        fn cross_variant_named_to_unnamed_roundtrip() {
            assert_roundtrip(
                Shape::Rect { width: 1, height: 2 },
                Shape::Circle(99),
            );
        }

        #[test]
        fn cross_variant_unnamed_to_unit_roundtrip() {
            assert_roundtrip(Shape::Circle(7), Shape::Point);
        }

        #[test]
        fn cross_variant_unit_to_named_roundtrip() {
            assert_roundtrip(
                Shape::Point,
                Shape::Rect { width: -3, height: 42 },
            );
        }

        // ----------------------------------------------------------------
        // aggregate: same variant on both sides
        // ----------------------------------------------------------------

        #[test]
        fn aggregate_same_variant_matches_direct_diff() {
            let a = Shape::Rect { width: 0,  height: 0  };
            let b = Shape::Rect { width: 10, height: -5 };
            let c = Shape::Rect { width: 3,  height: 20 };

            let mut d_ab = b.diff(&a);
            let     d_bc = c.diff(&b);
            d_ab.aggregate(&d_bc);

            let mut result = a.clone();
            result.patch(&d_ab);
            assert_eq!(result, c);
        }

        // ----------------------------------------------------------------
        // aggregate: first delta is Replace (self already replaced)
        // ----------------------------------------------------------------

        /// A → (Replace B) then B → C should collapse to A → (Replace C).
        #[test]
        fn aggregate_replace_then_same_variant() {
            let a = Shape::Circle(1);
            let b = Shape::Rect { width: 10, height: 20 };
            let c = Shape::Rect { width: 30, height: 5  };

            // d1 is Replace(b) because the variants differ
            let mut d1 = b.diff(&a);
            // d2 is a same-variant named-field delta
            let d2 = c.diff(&b);
            d1.aggregate(&d2);

            let mut result = a.clone();
            result.patch(&d1);
            assert_eq!(result, c);
        }

        // ----------------------------------------------------------------
        // aggregate: second delta is Replace (next replaces everything)
        // ----------------------------------------------------------------

        /// A → B then (Replace C) should collapse to A → (Replace C).
        #[test]
        fn aggregate_same_variant_then_replace() {
            let a = Shape::Rect { width: 0, height: 0 };
            let b = Shape::Rect { width: 5, height: 5 };
            let c = Shape::Circle(42);

            let mut d1 = b.diff(&a);
            let     d2 = c.diff(&b); // Replace(Circle(42))
            d1.aggregate(&d2);

            let mut result = a.clone();
            result.patch(&d1);
            assert_eq!(result, c);
        }

        // ----------------------------------------------------------------
        // aggregate: both deltas are Replace
        // ----------------------------------------------------------------

        #[test]
        fn aggregate_replace_then_replace() {
            let a = Shape::Point;
            let b = Shape::Circle(10);
            let c = Shape::Rect { width: -1, height: -1 };

            let mut d1 = b.diff(&a); // Replace(Circle(10))
            let     d2 = c.diff(&b); // Replace(Rect { .. })
            d1.aggregate(&d2);

            let mut result = a.clone();
            result.patch(&d1);
            assert_eq!(result, c);
        }
    }

    #[cfg(feature = "deku")]
    mod deku_tests {
        use super::*;
        use deku::ctx::Endian;
        use deku::reader::Reader;
        use deku::writer::Writer;
        use std::io::Cursor;

        #[test]
        fn test_arithmetic_delta_endianness() {
            let delta = ArithmeticDelta(0x12345678_u32);

            // Test Big Endian serialization
            let mut buf_be = vec![];
            let mut writer_be = Writer::new(Cursor::new(&mut buf_be));
            delta.to_writer(&mut writer_be, Endian::Big).unwrap();
            
            assert_eq!(buf_be, vec![0x12, 0x34, 0x56, 0x78]);

            // Test Big Endian deserialization
            let mut reader_be = Reader::new(Cursor::new(buf_be.as_slice()));
            let decoded_be = ArithmeticDelta::<u32>::from_reader_with_ctx(&mut reader_be, Endian::Big).unwrap();
            
            assert_eq!(delta, decoded_be);

            // Test Little Endian serialization
            let mut buf_le = vec![];
            let mut writer_le = Writer::new(Cursor::new(&mut buf_le));
            delta.to_writer(&mut writer_le, Endian::Little).unwrap();
            
            assert_eq!(buf_le, vec![0x78, 0x56, 0x34, 0x12]);

            // Test Little Endian deserialization
            let mut reader_le = Reader::new(Cursor::new(buf_le.as_slice()));
            let decoded_le = ArithmeticDelta::<u32>::from_reader_with_ctx(&mut reader_le, Endian::Little).unwrap();
            
            assert_eq!(delta, decoded_le);
        }

        #[cfg(feature = "alloc")]
        #[test]
        fn test_slice_delta_serialization_format() {
            let mut delta = SliceDelta::<u16>::new();
            
            // Entry 1: start: 1, remove: 2, add: [0xAAAA, 0xBBBB]
            delta.aggregate_entry(SliceDeltaEntry::new(1, 2, vec![0xAAAA, 0xBBBB]));
            // Entry 2: start: 5, remove: 0, add: [0xCCCC]
            delta.aggregate_entry(SliceDeltaEntry::new(5, 0, vec![0xCCCC]));

            let mut buf = vec![];
            let mut writer = Writer::new(Cursor::new(&mut buf));
            
            // Write out using Little Endian
            delta.to_writer(&mut writer, Endian::Little).unwrap();

            // Expected byte structure (Little Endian):
            // SliceDelta count: 2 (u32) -> [2, 0, 0, 0]
            // Entry 1:
            //   start_index: 1 -> [1, 0, 0, 0]
            //   remove: 2      -> [2, 0, 0, 0]
            //   add_len: 2     -> [2, 0, 0, 0]
            //   add[0]: 0xAAAA -> [0xAA, 0xAA]
            //   add[1]: 0xBBBB -> [0xBB, 0xBB]
            // Entry 2:
            //   start_index: 5 -> [5, 0, 0, 0]
            //   remove: 0      -> [0, 0, 0, 0]
            //   add_len: 1     -> [1, 0, 0, 0]
            //   add[0]: 0xCCCC -> [0xCC, 0xCC]
            let expected_bytes = vec![
                2, 0, 0, 0, // 2 entries
                1, 0, 0, 0, // entry 1 start
                2, 0, 0, 0, // entry 1 remove
                2, 0, 0, 0, // entry 1 add len
                0xAA, 0xAA, // 0xAAAA (Little Endian)
                0xBB, 0xBB, // 0xBBBB (Little Endian)
                5, 0, 0, 0, // entry 2 start
                0, 0, 0, 0, // entry 2 remove
                1, 0, 0, 0, // entry 2 add len
                0xCC, 0xCC, // 0xCCCC (Little Endian)
            ];
            
            assert_eq!(buf, expected_bytes);
        }

        #[cfg(feature = "alloc")]
        #[test]
        fn test_slice_delta_roundtrip() {
            let mut delta = SliceDelta::<i32>::new();
            delta.aggregate_entry(SliceDeltaEntry::new(0, 5, vec![1, 2, 3]));
            delta.aggregate_entry(SliceDeltaEntry::new(10, 0, vec![99, 100]));

            // Serialize
            let mut buf = vec![];
            let mut writer = Writer::new(Cursor::new(&mut buf));
            delta.to_writer(&mut writer, Endian::Big).unwrap();

            // Deserialize
            let mut reader = Reader::new(Cursor::new(buf.as_slice()));
            let decoded = SliceDelta::<i32>::from_reader_with_ctx(&mut reader, Endian::Big).unwrap();

            // Assert exact structural equality
            assert_eq!(delta.entries.len(), decoded.entries.len());
            for (key, original_entry) in &delta.entries {
                let decoded_entry = decoded.entries.get(key).unwrap();
                assert_eq!(original_entry, decoded_entry);
            }
        }
    }
}