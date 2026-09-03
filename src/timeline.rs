use std::{ops::{Bound, Deref, RangeBounds}, slice::SliceIndex};

use crate::delta::{Delta, Differentiable};

#[derive(Debug, Clone)]
pub struct Commit<D: Delta, M> {
    delta: D,
    meta: M,
}

impl<D: Delta, M> Commit<D, M> {
    pub fn new(delta: D, meta: M) -> Self {
        Self {
            delta,
            meta,
        }
    }

    pub fn meta(&self) -> &M {
        &self.meta
    }
}

impl<D: Delta, M> Deref for Commit<D, M> {
    type Target = D;

    fn deref(&self) -> &Self::Target {
        &self.delta
    }
}

// we can think of a timeline as a singly linked list
// where the nodes are states and the pointers are commits.
// the time complexities are very similar in that we have to
// traverse the list to get nodes rather than immediately fetching
// them like from a contiguous array.
#[derive(Debug, Clone)]
pub struct Timeline<D: Delta, M = ()> {
    commits: Vec<Commit<D, M>>,
}

impl<D: Delta, M> Timeline<D, M> {
    /// Adds a commit to the timeline. Returns the index of the commit.
    pub fn add_commit(&mut self, commit: Commit<D, M>) -> usize {
        self.commits.push(commit);
        self.commits.len()-1
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    /// Merges all the commits in the range into one commit.
    /// Returns None if the range is empty, or the index of the merged commit otherwise.
    pub fn merge_commits(&mut self, range: impl RangeBounds<usize>, new_meta: M) -> Option<usize> {
        let i = match range.start_bound() {
            Bound::Excluded(&i) => i+1,
            Bound::Included(&i) => i,
            Bound::Unbounded => 0,
        };
        
        let Some(mut merged) = self.commits.drain(range).reduce(|mut a, b| {
            a.delta.aggregate_owned(b.delta);
            a
        }) else {
            // empty range
            return None;
        };
        merged.meta = new_meta;

        self.commits.insert(i, merged);
    
        Some(i)
    }

    /// Patches multiple commits onto an initial state and returns the resulting final state.
    pub fn build_state<T, R>(&self, state: &mut T, range: R)
    where
        T: Differentiable<D>,
        R: SliceIndex<[Commit<D, M>], Output = [Commit<D, M>]>,
    {
        for commit in &self.commits[range] {
            state.patch(&commit.delta);
        }
    }

    /// Patches the range of commits using the output type's default value as the initial state.
    pub fn build_state_from_default<T, R>(&self, range: R) -> T
    where
        T: Differentiable<D> + Default,
        R: SliceIndex<[Commit<D, M>], Output = [Commit<D, M>]>,
    {
        let mut state = T::default();
        self.build_state(&mut state, range);
        state
    }
}