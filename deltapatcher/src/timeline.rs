use std::{ops::{Bound, Deref, DerefMut, RangeBounds}, slice::SliceIndex};

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
    pub fn new() -> Self {
        Self {
            commits: Vec::new(),
        }
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

impl<D: Delta, M> DerefMut for Timeline<D, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.commits
    }
}

impl<D: Delta> FromIterator<D> for Timeline<D, ()> {
    fn from_iter<T: IntoIterator<Item = D>>(iter: T) -> Self {
        iter.into_iter().map(|d| Commit::new(d, ())).collect()
    }
}

impl<D: Delta, M> FromIterator<Commit<D, M>> for Timeline<D, M> {
    fn from_iter<T: IntoIterator<Item = Commit<D, M>>>(iter: T) -> Self {
        Self {
            commits: iter.into_iter().collect()
        }
    }
}

/// A timeline type that makes backwards traversal and arbitrary
/// state retrieval faster (constant-time) by caching the state
/// at fixed intervals. 
pub struct StateCachedTimeline<T, D, M = ()>
where 
    T: Differentiable<D> + Clone,
    D: Delta,
{
    timeline: Timeline<D, M>,
    states: Vec<T>,

    /// The number of commits that must be made
    /// before storing a new state. 1 = store for every commit.
    /// Must not be 0.
    state_cache_interval: usize,
}

impl<T, D, M> StateCachedTimeline<T, D, M>
where 
    T: Differentiable<D> + Clone,
    D: Delta,
{
    pub fn new(state_cache_interval: usize, initial_state: T) -> Self {
        assert!(state_cache_interval >= 1, "state_cache_interval must be >= 1");
        Self {
            timeline: Timeline::new(),
            states: vec![initial_state],
            state_cache_interval,
        }
    }

    pub fn from_timeline(state_cache_interval: usize, timeline: Timeline<D, M>, initial_state: T) -> Self {
        assert!(state_cache_interval >= 1, "state_cache_interval must be >= 1");

        let mut states = Vec::with_capacity(timeline.len() / state_cache_interval);
        states.push(initial_state.clone());

        let mut current_state = initial_state;

        // the last state_cache_interval-1 commits don't need
        // to be iterated over since they aren't being cached right now.
        for i in 1..timeline.len().saturating_sub(state_cache_interval-1) {
            current_state.patch(&timeline[i]);
            if i % state_cache_interval == 0 {
                states.push(current_state.clone());
            }
        }

        Self {
            timeline,
            states,
            state_cache_interval,
        }
    }

    pub fn from_iter(state_cache_interval: usize, initial_state: T, iter: impl Iterator<Item = Commit<D, M>>) -> Self {
        let mut t = Self::new(state_cache_interval, initial_state);
        t.extend_next(iter);
        t
    }

    /// Gets the state immediately preceding
    /// the commit at index `i`. This runs in
    /// `O(state_cache_interval * T::patch)` worst case.
    /// Returns [`None`] if the index is out of bounds.
    pub fn get_state_before(&self, i: usize) -> Option<T> {
        let (cache_idx, cache) = self.get_nearest_state(i)?;

        let mut state = cache.clone();
        for j in cache_idx..i {
            state.patch(&self.timeline[j].delta);
        }

        Some(state)
    }

    /// Gets the state immediately following
    /// the commit at index `i`. This runs in
    /// `O(state_cache_interval * T::patch)` worst case.
    /// Returns [`None`] if the index is out of bounds.
    pub fn get_state_after(&self, i: usize) -> Option<T> {
        let (cache_idx, cache) = self.get_nearest_state(i)?;

        let mut state = cache.clone();
        for j in cache_idx..=i {
            state.patch(&self.timeline[j].delta);
        }

        Some(state)
    }

    /// Returns a reference to the nearest preceding
    /// cached state and its index in the timeline.
    /// Returns [`None`] if the index is out of bounds.
    pub fn get_nearest_state(&self, i: usize) -> Option<(usize, &T)> {
        if i >= self.timeline.len() {
            return None;
        }
        let cache_i = self.get_cache_index(i);
        let cache = &self.states[cache_i];
        Some((self.cache_to_timeline_index(cache_i), cache))
    }

    /// Gets the index of the nearest preceding cached state
    fn get_cache_index(&self, i: usize) -> usize {
        debug_assert!(i < self.timeline.len());
    
        i / self.state_cache_interval
    }

    /// Gets the exact index of the commit immediately following the state at index i.
    fn cache_to_timeline_index(&self, i: usize) -> usize {
        debug_assert!(i < self.states.len());

        i * self.state_cache_interval
    }

    const fn should_cache(&self, i: usize) -> bool {
        i % self.state_cache_interval == 0
    }

    // TODO invariant that there is always an intial state at index 0, even if there are no commits.
    // (i.e. we can cache states for commits that don't exist yet)
    /// Pushes a commit to the timeline, caching the current state if necessary.
    /// This will panic if the timeline is empty, since have no wa3y to initialize the state.
    /// If you need to push on a timeline that may be empty, see [`push`][StateCachedTimeline::push] or [`push_first`][StateCachedTimeline::push_first].
    /// Use [`extend_next`][StateCachedTimeline::extend_next] for repeated insertions.
    pub fn push_next(&mut self, commit: Commit<D, M>) {
        assert!(!self.timeline.is_empty(), "timeline must not be empty");

        let i = self.timeline.len();
        self.timeline.commits.push(commit);

        if self.should_cache(i) {
            // i is guaranteed to be in bounds since the
            // timeline always has at least 1 element before insertion
            let state = unsafe { self.get_state_before(i).unwrap_unchecked() };
            self.states.push(state);
        }
    }

    /// Pushes a commit to the timeline, caching the current state if necessary.
    /// If the timeline is empty, this will initialize a new state from [`T::default`][Default::default]
    /// Use [`extend`][StateCachedTimeline::extend] for repeated iterations.
    pub fn push(&mut self, commit: Commit<D, M>)
    where 
        T: Default,
    {
        let i = self.timeline.len();
        self.timeline.commits.push(commit);
        if self.should_cache(i) {
            let state = self.get_state_before(i).unwrap_or_default();
            self.states.push(state);
        }
    }

    /// Push the first commit to an empty timeline, using the provided value as the initial state.
    /// Panics if the timeline is not empty.
    /// Use [`extend_first`][StateCachedTimeline::extend_first] for [`push_first`][StateCachedTimeline::push_first]
    /// + repeated [`push_next`][StateCachedTimeline::push_next] iterations.
    pub fn push_first(&mut self, commit: Commit<D, M>, state: T) {
        assert!(self.is_empty(), "timeline must be empty");

        self.timeline.commits.push(commit);
        self.states.push(state);
    }

    /// Pushes multiple commits to the timeline, caching states where necessary.
    /// This will panic if the timeline is empty.
    pub fn extend_next(&mut self, commits: impl Iterator<Item = Commit<D, M>>) {
        assert!(!self.timeline.is_empty(), "timeline must not be empty");

        let start_i = self.timeline.len();

        // always get the state of the first commit so that we can patch it.
        let state = unsafe { self.get_state_before(start_i).unwrap_unchecked() };
        
        // extend
        self.extend_with_state(state, commits);
    }

    /// Pushes multiple commits to the timeline, caching states where necessary.
    /// If the timeline is empty, it will infer intial state from [`T::default`][Default::default].
    pub fn extend(&mut self, commits: impl Iterator<Item = Commit<D, M>>)
    where
        T: Default,
    {
        let start_i = self.timeline.len();
        let state = self.get_state_before(start_i).unwrap_or_default();

        // extend
        self.extend_with_state(state, commits);
    }

    pub fn extend_first(&mut self, state: T, commits: impl Iterator<Item = Commit<D, M>>) {
        assert!(self.timeline.is_empty(), "timeline must be empty");
        self.extend_with_state(state, commits);
    }

    /// Extend by providing the state immediately before the first commit in the iterator.
    fn extend_with_state(&mut self, mut state: T, commits: impl Iterator<Item = Commit<D, M>>) {
        let mut i = self.timeline.len();
        for commit in commits {
            // push prev iteration state for the commit at i.            
            if self.should_cache(i) {
                self.states.push(state.clone());
            }

            // build state linearly by patching it with
            // each commit as we add it
            state.patch(&commit.delta);
            self.timeline.commits.push(commit);
            i += 1;
        }
    }
}

impl<T, D, M> Deref for StateCachedTimeline<T, D, M>
where 
    T: Differentiable<D> + Clone,
    D: Delta,
{
    type Target = Timeline<D, M>;
    
    fn deref(&self) -> &Self::Target {
        &self.timeline
    }
}

// TODO test suite (especially for the cached one)