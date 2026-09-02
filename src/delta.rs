use std::{collections::{BTreeMap, BTreeSet}, ops::{Add, AddAssign, Sub}};

pub trait Differentiable<D: Delta> {
    /// Get the delta between the final state (self) and initial.
    fn differentiate(&self, initial: &Self) -> D;

    /// Apply a given delta onto self.
    fn apply_delta(&mut self, delta: &D);
}
// TODO implement a bytemuck differentiable that's essentially Vec<u8> differentiation.

/// An abstract trait representing the change in two states.
/// These should be internally expressed against the state of the
/// parent object immediately before it is applied.
pub trait Delta {
    /// Layer another delta on top of the current one.
    fn aggregate(&mut self, next: &Self);
}

/// A delta which represents item changes in a vector or slice
#[derive(Debug, Clone)]
pub struct SliceDelta<T> {
    entries: BTreeMap<u32, SliceDeltaEntry<T>>,
}

impl<T> SliceDelta<T> {
    pub fn aggregate_entry(&mut self, mut next: SliceDeltaEntry<T>) {
        // find the entry immediately before next
        if let Some((&start, existing)) = self.entries.range(..=next.start_index).next_back() {
            if existing.overlaps(&next) {
                let mut existing = self.entries.remove(&start).unwrap();
                existing.merge(next);
                next = existing;
            }
        }

        // find all entries after the first one which need to be merged
        // TODO stop rebuilding the iterator every iteration.
        while let Some((&start, existing)) = self.entries.range(next.start_index..).next() {
            if !next.overlaps(existing) {
                break;
            }

            let existing = self.entries.remove(&start).unwrap();
            next.merge(existing);
        }

        self.entries.insert(next.start_index, next);
    }
}

impl<T> Delta for SliceDelta<T> {
    fn aggregate(&mut self, next: &Self) {
        todo!()
    }
}

impl<T> Differentiable<SliceDelta<T>> for Vec<T> {
    fn apply_delta(&mut self, delta: &SliceDelta<T>) {
        for entry in delta.entries.values() {
            let start = entry.start_index;
            let end = entry.end_index();
            
            todo!()
        }
    }

    fn differentiate(&self, initial: &Self) -> SliceDelta<T> {
        todo!()
    }
}
// TODO implement for [T] but throw error instead of increasing the size of the delta.


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceDeltaEntry<T> {
    /// The start index of the entry within the vec 
    start_index: u32,

    /// The data (added or removed) at that starting index
    block: SliceDeltaBlock<T>,
}

impl<T> SliceDeltaEntry<T> {
    /// Computes the entry's end index based on the start index and length.
    pub const fn end_index(&self) -> u32 {
        self.start_index + self.len()
    }

    /// Gets the length of the inner block.
    pub const fn len(&self) -> u32 {
        self.block.len()
    }

    /// Whether the index is within the affected region of this entry.
    pub const fn contains_index(&self, i: u32) -> bool {
        i >= self.start_index && i < self.end_index()
    }

    /// Whether another entry is entirely within the affected region of this entry.
    pub const fn contains(&self, other: &Self) -> bool {
        other.start_index >= self.start_index && other.end_index() <= self.end_index()
    }

    /// Whether the two entries overlap with each other.
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.start_index < other.end_index() && other.end_index() < self.end_index()
    }

    pub const fn touches(&self, other: &Self) -> bool {
        self.end_index() == other.start_index
            || other.end_index() == self.start_index
    }


    /// Merges a newer entry on top of the existing one (self). This consumes the new entry.
    /// Possible cases:
    /// Add, Remove, one fully contains the other: remove the content of next from the add block of self, and change self to Removed if next contains self.
    /// Add, Remove, one does not fully contain the other: invalid case, panic.
    /// Add, Add: TODO
    /// Remove, Remove: Simply extend the current entry.
    /// Remove, Add: TODO
    pub fn merge(&mut self, next: Self) {
        match (&mut self.block, &next.block) {
            (SliceDeltaBlock::Added(a), SliceDeltaBlock::Added(b)) => todo!(),
            (SliceDeltaBlock::Added(a), SliceDeltaBlock::Removed(blen)) => {
                if self.contains(&next) {
                    todo!()
                } else if next.contains(&self) {
                    todo!()
                } else {
                    panic!("Invalid operation: cannot merge two entries of different sign which do not completely contain each other");
                }
            },
            (SliceDeltaBlock::Removed(alen), SliceDeltaBlock::Added(b)) => todo!(),
            (SliceDeltaBlock::Removed(alen), SliceDeltaBlock::Removed(blen)) => todo!(),
        }
    }
}

impl<T: Clone> Delta for SliceDeltaEntry<T> {
    fn aggregate(&mut self, next: &Self) {
        self.merge(next.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SliceDeltaBlock<T> {
    /// Add a vector of values
    Added(Vec<T>),
    
    /// Remove n values
    Removed(u32),

    // TODO maybe other variants such as swaps
}

impl<T> SliceDeltaBlock<T> {
    pub const fn len(&self) -> u32 {
        match self {
            Self::Added(v) => v.len() as u32,
            Self::Removed(l) => *l,
        }
    }
}

impl<T> Default for SliceDeltaBlock<T> {
    fn default() -> Self {
        Self::Removed(0)
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

impl<A, B> Differentiable<AdditiveDelta<B>> for A
where 
    A: AddAssign<B>,
    for<'a> &'a A: Sub<Output = B>,
    B: AddAssign + Copy,
{
    fn differentiate(&self, initial: &Self) -> AdditiveDelta<B> {
        AdditiveDelta(self - initial)
    }

    fn apply_delta(&mut self, delta: &AdditiveDelta<B>) {
        *self += delta.0;
    }
}