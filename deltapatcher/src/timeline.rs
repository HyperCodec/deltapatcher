use std::{ops::{Bound, Deref, RangeBounds}, slice::SliceIndex};

use crate::delta::{Delta, Differentiable};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Timeline<D: Delta, M = ()> {
    commits: Vec<Commit<D, M>>,
}

impl<D: Delta, M> Timeline<D, M> {
    /// Adds a commit to the timeline. Returns the index of the commit.
    pub fn add_commit(&mut self, commit: Commit<D, M>) -> usize {
        self.commits.push(commit);
        self.commits.len()-1
    }

    /// Removes and return the most recent commit.
    /// Returns [`None`] if the timeline is empty.
    pub fn pop_commit(&mut self) -> Option<Commit<D, M>> {
        self.commits.pop()
    }

    /// Removes the last n commits, aggregates them, and then returns the delta.
    /// Returns [`None`] if the timeline has less than n elements.
    pub fn pop_aggregate(&mut self, n: usize) -> Option<D> {
        if n > self.len() {
            return None;
        }

        let start = self.len() - n;
        self.commits.drain(start..).map(|c|c.delta).reduce(|mut a, b| {
            a.aggregate_owned(b);
            a
        })
    }

    fn get_starting_index(&self, bound: Bound<&usize>) -> Option<usize> {
        let i = match bound {
            Bound::Excluded(i) => *i+1,
            Bound::Included(i) => *i,
            Bound::Unbounded => 0,
        };

        if i < self.len() {
            Some(i)
        } else {
            None
        }
    }

    fn get_ending_index(&self, bound: Bound<&usize>) -> Option<usize> {
        let i = match bound {
            Bound::Excluded(i) => *i,
            Bound::Included(i) => *i+1,
            Bound::Unbounded => self.len(),
        };
        if i <= self.len() {
            Some(i)
        } else {
            None
        }
    }

    fn get_bounds(&self, range: impl RangeBounds<usize>) -> Option<(usize, usize)> {
        let start = self.get_starting_index(range.start_bound())?;
        let end = self.get_ending_index(range.end_bound())?;
        if start < end {
            Some((start, end))
        } else {
            None
        }
    }

    /// Merges all the commits in the range into one commit,
    /// returning the index of the merged commit.
    /// The returned delta represents the change between the state immediately before the left
    /// bound to immediately after the right bound.
    /// Returns [`None`] if the range is empty or out of bounds.
    pub fn merge_commits(&mut self, range: impl RangeBounds<usize>, new_meta: M) -> Option<usize> {
        let i = self.get_starting_index(range.start_bound())?;
        
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

    /// Gets an aggregate delta from a range of indices.
    /// The returned delta represents the change between the state immediately before the left
    /// bound to immediately after the right bound.
    /// Returns [`None`] if the range is empty or out of bounds.
    pub fn get_aggregate(&self, range: impl RangeBounds<usize>) -> Option<D>
    where 
        D: Clone,
    {
        let (start, end) = self.get_bounds(range)?;

        if start >= end {
            return None;
        }

        let mut delta = self.commits[start].delta.clone();
        for i in start+1..end {
            delta.aggregate(&self.commits[i].delta);
        }

        Some(delta)
    }

    /// Patches multiple commits onto an initial state.
    /// The initial state should represent he state immediately
    /// before the lower bound is applied, and the resulting state
    /// should represent the state immediately after the upper bound is applied.
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

impl<D: Delta, M> Deref for Timeline<D, M> {
    type Target = [Commit<D, M>];

    fn deref(&self) -> &Self::Target {
        &self.commits
    }
}