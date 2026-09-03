use std::{
    collections::{BTreeMap, VecDeque},
    ops::{AddAssign, Sub},
};

pub trait Differentiable<D: Delta> {
    /// Get the delta between the final state (self) and initial.
    fn differentiate(&self, initial: &Self) -> D;

    /// Apply a given delta's changes onto self.
    fn patch(&mut self, delta: &D);
}
// TODO implement a bytemuck differentiable that's essentially Vec<u8> differentiation.

/// An abstract trait representing the change in two states.
/// These should be internally expressed against the state
/// from immediately before they are applied.
pub trait Delta {
    /// Layer another delta on top of the current one.
    fn aggregate(&mut self, next: &Self);
    fn aggregate_owned(&mut self, next: Self)
    where
        Self: Sized
    {
        self.aggregate(&next);
    }
}

/// A delta which represents item changes in a vector or slice
#[derive(Debug, Clone)]
pub struct SliceDelta<T> {
    /// The non-overlapping splices expressed in
    /// the coordinate system of the vector before
    /// the delta is applied.
    entries: BTreeMap<u32, SliceDeltaEntry<T>>,
}

impl<T> Default for SliceDelta<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

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
        // At most one existing entry can touch/overlap `next` from the left
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

        // Any number of entries can touch/overlap from the right, so keep
        // absorbing until nothing more qualifies.
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

/// Merges two entries known to be touching or overlapping, with `a` sorted
/// before `b` (`a.start_index <= b.start_index`). See [`SliceDelta::aggregate_entry`]
/// for the semantics when they genuinely overlap.
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
enum Segment<T> {
    /// `a_end == None` means "extends to the end of the vector" — this lets
    /// us represent the untouched tail without ever knowing the vector's
    /// actual length.
    Unchanged { a_start: u32, a_end: Option<u32> },
    Added(Vec<T>),
}

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

impl<T: Clone> Delta for SliceDelta<T> {
    fn aggregate(&mut self, next: &Self) {
        self.aggregate_owned(next.clone());
    }

    fn aggregate_owned(&mut self, next: Self) {
        // Step 1: express B as segments over self's own (owned) entries.
        let mut segments: VecDeque<Segment<T>> = VecDeque::new();
        let mut cursor_a = 0u32;
        for entry in std::mem::take(&mut self.entries).into_values() {
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
                            std::mem::take(&mut add_buffer),
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

impl<T: Clone + PartialEq> Differentiable<SliceDelta<T>> for Vec<T> {
    fn patch(&mut self, delta: &SliceDelta<T>) {
        for entry in delta.entries.values().rev() {
            let start = entry.start_index as usize;
            let end = start + entry.remove as usize;

            self.splice(start..end, entry.add.iter().cloned());
        }
    }

    fn differentiate(&self, initial: &Self) -> SliceDelta<T> {
        let final_ = self;
        let n = initial.len();
        let m = final_.len();

        // Standard LCS DP: lcs[i][j] = length of the LCS of initial[i..]
        // and final_[j..]. O(n*m) time and space — fine for modest inputs;
        // swap for Myers/Hirschberg if this ever needs to handle huge
        // vectors.
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
                                std::mem::take(&mut run_add),
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
                // Equal characters are always part of *some* optimal LCS,
                // so it's safe to greedily take the match here.
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
}
// TODO implement for [T] but throw error instead of increasing the size of the delta.

/// A splicing entry in the delta
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceDeltaEntry<T> {
    /// The start index of the entry within the vec
    start_index: u32,

    /// The number of items removed as a result of the entry
    remove: u32,

    /// The items added as a result of the entry.
    /// When applied, these are added after the others are removed.
    add: Vec<T>,
}

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

/// A delta defined by addition/subtraction.
pub struct AdditiveDelta<T: AddAssign + Copy>(pub T);

impl<T> Delta for AdditiveDelta<T>
where 
    T: AddAssign + Copy,
{
    fn aggregate(&mut self, next: &Self) {
        self.0 += next.0;
    }
}

// arithmetic overflow is necessary for some edge cases to work properly
#[allow(arithmetic_overflow)]
impl<A, B> Differentiable<AdditiveDelta<B>> for A
where 
    A: AddAssign<B>,
    for<'a> &'a A: Sub<Output = B>,
    B: AddAssign + Copy,
{
    fn differentiate(&self, initial: &Self) -> AdditiveDelta<B> {
        AdditiveDelta(self - initial)
    }

    fn patch(&mut self, delta: &AdditiveDelta<B>) {
        *self += delta.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_roundtrip(initial: Vec<i32>, final_: Vec<i32>) {
        let delta = final_.differentiate(&initial);
        let mut applied = initial.clone();
        applied.patch(&delta);
        assert_eq!(applied, final_);
    }

    #[test]
    fn differentiate_and_apply() {
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

        let mut d1 = b.differentiate(&a);
        let d2 = c.differentiate(&b);
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

    // TODO additive delta
}