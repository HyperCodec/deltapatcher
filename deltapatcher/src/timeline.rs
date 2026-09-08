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

    pub fn push(&mut self, commit: Commit<D, M>) {
        self.commits.push(commit);
    }

    /// Removes and returns the last commit. Only tail removal is exposed —
    /// removing or inserting a commit in the *middle* of the chain would
    /// invalidate every following commit's delta, since each one is
    /// expressed relative to the state immediately before it. Use
    /// [`Timeline::merge_commits`] if you want to consolidate a range
    /// instead of discarding it.
    pub fn pop(&mut self) -> Option<Commit<D, M>> {
        self.commits.pop()
    }

    pub fn truncate(&mut self, len: usize) {
        self.commits.truncate(len);
    }

    pub fn clear(&mut self) {
        self.commits.clear();
    }

    /// Removes the last n commits, aggregates them, and then returns the delta.
    /// Returns [`None`] if the timeline has less than n elements.
    pub fn pop_aggregate(&mut self, n: usize) -> Option<D> {
        if n > self.len() {
            return None;
        }

        let start = self.len() - n;
        self.commits.drain(start..).map(|c| c.delta).reduce(|mut a, b| {
            a.aggregate_owned(b);
            a
        })
    }

    fn get_starting_index(&self, bound: Bound<&usize>) -> Option<usize> {
        let i = match bound {
            Bound::Excluded(i) => *i + 1,
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
            Bound::Included(i) => *i + 1,
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
        for i in start + 1..end {
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

impl<D: Delta, M> Default for Timeline<D, M> {
    fn default() -> Self {
        Self::new()
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
            commits: iter.into_iter().collect(),
        }
    }
}

impl<D: Delta, M> Extend<Commit<D, M>> for Timeline<D, M> {
    fn extend<I: IntoIterator<Item = Commit<D, M>>>(&mut self, iter: I) {
        self.commits.extend(iter);
    }
}

impl<D: Delta, M> IntoIterator for Timeline<D, M> {
    type Item = Commit<D, M>;
    type IntoIter = std::vec::IntoIter<Commit<D, M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.commits.into_iter()
    }
}

impl<'a, D: Delta, M> IntoIterator for &'a Timeline<D, M> {
    type Item = &'a Commit<D, M>;
    type IntoIter = std::slice::Iter<'a, Commit<D, M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.commits.iter()
    }
}

/// Replays `timeline` against `initial_state`, producing the cache-state
/// vector for it. Shared between [`StateCachedTimeline::from_timeline`]
/// and [`StateCachedTimeline::set_initial_state`], since "rebuild the
/// cache against a given starting state" is the same operation either way.
///
/// Only replays as far as the last cache boundary — trailing commits past
/// it don't need to be reflected in any cached state.
fn build_state_cache<T, D, M>(initial_state: T, timeline: &Timeline<D, M>, interval: usize) -> Vec<T>
where
    T: Differentiable<D> + Clone,
    D: Delta,
{
    let cached_count = timeline.len() / interval;
    let replay_len = cached_count * interval;

    let mut states = Vec::with_capacity(cached_count + 1);
    states.push(initial_state.clone());

    let mut current = initial_state;
    for i in 0..replay_len {
        current.patch(&timeline[i]);
        if (i + 1) % interval == 0 {
            states.push(current.clone());
        }
    }

    states
}

/// A timeline type that makes backwards traversal and arbitrary
/// state retrieval faster (constant-time) by caching the state
/// at fixed intervals.
///
/// # Invariants
/// - `states` is never empty. `states[0]` always holds the initial
///   state — the state immediately before commit `0` — even when
///   `timeline` has no commits at all.
/// - `states[k]` holds the state immediately before commit
///   `k * state_cache_interval` in `timeline`.
/// - The cache may have up to one "hanging" entry beyond what's strictly
///   required: a cached state may map to `timeline.len()` itself (the
///   current/latest state), even though no commit exists there yet.
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

        let states = build_state_cache(initial_state, &timeline, state_cache_interval);

        Self {
            timeline,
            states,
            state_cache_interval,
        }
    }

    pub fn from_iter(
        state_cache_interval: usize,
        initial_state: T,
        iter: impl IntoIterator<Item = Commit<D, M>>,
    ) -> Self {
        let mut t = Self::new(state_cache_interval, initial_state);
        t.extend(iter);
        t
    }

    /// Replaces the initial state and recomputes every cached state
    /// against it. `O(timeline.len() * T::patch)` — this has to re-walk
    /// the whole chain, since every existing cached state was derived
    /// from the old initial state.
    pub fn set_initial_state(&mut self, state: T) {
        self.states = build_state_cache(state, &self.timeline, self.state_cache_interval);
    }

    /// Gets the state immediately preceding
    /// the commit at index `i`. `i` may be `0..=self.timeline.len()`;
    /// the upper bound asks for the current state (immediately after the
    /// last commit). This runs in
    /// `O(state_cache_interval * T::patch)` worst case.
    /// Returns [`None`] if `i` is out of that range.
    pub fn get_state_before(&self, i: usize) -> Option<T> {
        let (cache_idx, cache) = self.get_nearest_state(i)?;

        let mut state = cache.clone();
        for j in cache_idx..i {
            state.patch(&self.timeline[j]);
        }

        Some(state)
    }

    /// Gets the state immediately following
    /// the commit at index `i`. `i` must be an existing commit index.
    /// This runs in `O(state_cache_interval * T::patch)` worst case.
    /// Returns [`None`] if the index is out of bounds.
    pub fn get_state_after(&self, i: usize) -> Option<T> {
        if i >= self.timeline.len() {
            return None;
        }

        let (cache_idx, cache) = self.get_nearest_state(i)?;

        let mut state = cache.clone();
        for j in cache_idx..=i {
            state.patch(&self.timeline[j]);
        }

        Some(state)
    }

    /// The state immediately after the last commit — or the initial
    /// state, if there are no commits yet.
    pub fn current_state(&self) -> T {
        self.get_state_before(self.timeline.len())
            .expect("timeline.len() is always in bounds for get_state_before")
    }

    /// Returns a reference to the nearest preceding
    /// cached state and its index in the timeline.
    ///
    /// `i` may be any index in `0..=self.timeline.len()`: the upper bound
    /// asks for the current state, a "hanging" cache lookup (see the
    /// struct-level docs). Returns [`None`] if `i` is out of that range.
    pub fn get_nearest_state(&self, i: usize) -> Option<(usize, &T)> {
        if i > self.timeline.len() {
            return None;
        }
        let cache_i = self.get_cache_index(i);
        let cache = &self.states[cache_i];
        Some((self.cache_to_timeline_index(cache_i), cache))
    }

    /// Gets the index into `states` of the nearest preceding cached state
    /// for commit index `i` (or `i == timeline.len()`, for the current
    /// state). Clamped to the last entry actually present, since a cache
    /// exactly at `i` isn't guaranteed to exist yet — see the "hanging
    /// cache" note on [`StateCachedTimeline`].
    fn get_cache_index(&self, i: usize) -> usize {
        debug_assert!(i <= self.timeline.len());

        (i / self.state_cache_interval).min(self.states.len() - 1)
    }

    /// Gets the exact index of the commit immediately following the state at index i.
    fn cache_to_timeline_index(&self, i: usize) -> usize {
        debug_assert!(i < self.states.len());

        i * self.state_cache_interval
    }

    const fn should_cache(&self, i: usize) -> bool {
        i % self.state_cache_interval == 0
    }

    /// Pushes a commit to the timeline, caching the resulting state if it
    /// lands on a cache boundary.
    pub fn push(&mut self, commit: Commit<D, M>) {
        let i = self.timeline.len();
        self.timeline.push(commit);

        // i == 0 is always already covered by the invariant initial cache
        // entry, so there's nothing to do in that case.
        if i != 0 && self.should_cache(i) {
            // i is guaranteed to be in bounds since the timeline always
            // has at least 1 element before insertion.
            let state = unsafe { self.get_state_before(i).unwrap_unchecked() };
            self.states.push(state);
        }
    }

    /// Removes and returns the last commit, discarding any cached state
    /// that depended on it. See [`Timeline::pop`] for why only tail
    /// removal is supported.
    pub fn pop(&mut self) -> Option<Commit<D, M>> {
        let commit = self.timeline.pop()?;
        self.drop_stale_cache_entries();
        Some(commit)
    }

    pub fn truncate(&mut self, len: usize) {
        self.timeline.truncate(len);
        self.drop_stale_cache_entries();
    }

    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// Drops any cached state whose boundary now lies past the end of the
    /// timeline, after a `pop`/`truncate` shortened it. `states[0]` is
    /// never dropped — it's the invariant initial-state entry.
    fn drop_stale_cache_entries(&mut self) {
        let len = self.timeline.len();
        while self.states.len() > 1 && self.cache_to_timeline_index(self.states.len() - 1) > len {
            self.states.pop();
        }
    }
}

impl<T, D, M> Extend<Commit<D, M>> for StateCachedTimeline<T, D, M>
where
    T: Differentiable<D> + Clone,
    D: Delta,
{
    fn extend<I: IntoIterator<Item = Commit<D, M>>>(&mut self, iter: I) {
        let start_i = self.timeline.len();
        // start_i == self.timeline.len() is always a valid query.
        let mut state = unsafe { self.get_state_before(start_i).unwrap_unchecked() };

        let mut i = start_i;
        for commit in iter {
            if i != 0 && self.should_cache(i) {
                self.states.push(state.clone());
            }

            // build state linearly by patching it with
            // each commit as we add it
            state.patch(&commit.delta);
            self.timeline.push(commit);
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

// TODO test