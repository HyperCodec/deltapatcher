use core::{ops::{Bound, Deref, DerefMut, RangeBounds}, slice::SliceIndex};

use alloc::{vec, vec::Vec};
#[cfg(feature = "deku")]
use deku::{ctx::{Endian, Limit}, no_std_io::{Read, Seek, Write}, prelude::*};

use crate::delta::{Delta, Diff};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Commit<D: Delta, M> {
    delta: D,
    meta: M,
}

impl<D: Delta, M> Commit<D, M> {
    /// Constructs a new commit from the given delta and metadata.
    pub fn new(delta: D, meta: M) -> Self {
        Self {
            delta,
            meta,
        }
    }

    /// Constructs a new commit with the given delta
    /// and uses [`M::default`][Default::default] for the metadata.
    pub fn new_with_default(delta: D) -> Self
    where 
        M: Default,
    {
        Self::new(
            delta,
            M::default(),
        )
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

#[cfg(feature = "deku")]
impl<D, M, Ctx1, Ctx2> DekuWriter<(Ctx1, Ctx2)> for Commit<D, M>
where 
    D: Delta + DekuWriter<Ctx1>,
    M: DekuWriter<Ctx2>,
{
    fn to_writer<W: Write + Seek>(&self, writer: &mut Writer<W>, (ctx1, ctx2): (Ctx1, Ctx2)) -> Result<(), DekuError> {
        self.delta.to_writer(writer, ctx1)?;
        self.meta.to_writer(writer, ctx2)
    }
}

#[cfg(feature = "deku")]
impl<'a, D, M, Ctx1, Ctx2> DekuReader<'a, (Ctx1, Ctx2)> for Commit<D, M>
where 
    D: Delta + DekuReader<'a, Ctx1>,
    M: DekuReader<'a, Ctx2>,
{
    fn from_reader_with_ctx<R: Read + Seek>(reader: &mut Reader<R>, (ctx1, ctx2): (Ctx1, Ctx2),) -> Result<Self, DekuError>
    where
        Self: Sized
    {
        let delta = D::from_reader_with_ctx(reader, ctx1)?;
        let meta = M::from_reader_with_ctx(reader, ctx2)?;

        Ok(Self {
            delta,
            meta
        })
    }
}

// we can think of a timeline as a singly linked list
// where the nodes are states and the pointers are commits.
// the time complexities are very similar in that we have to
// traverse the list to get nodes rather than immediately fetching
// them like from a contiguous array.
#[derive(Debug, Clone, PartialEq)]
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
    /// This runs in O(n). If this is not ideal, use [`StateCachedTimeline`] instead.
    pub fn build_state<T, R>(&self, state: &mut T, range: R)
    where
        T: Diff<D>,
        R: SliceIndex<[Commit<D, M>], Output = [Commit<D, M>]>,
    {
        for commit in &self.commits[range] {
            state.patch(&commit.delta);
        }
    }

    /// Patches the range of commits using the output type's default value as the initial state.
    /// This runs in O(n). If this is not ideal, use [`StateCachedTimeline`] instead.
    pub fn build_state_from_default<T, R>(&self, range: R) -> T
    where
        T: Diff<D> + Default,
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
    type IntoIter = alloc::vec::IntoIter<Commit<D, M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.commits.into_iter()
    }
}

impl<'a, D: Delta, M> IntoIterator for &'a Timeline<D, M> {
    type Item = &'a Commit<D, M>;
    type IntoIter = core::slice::Iter<'a, Commit<D, M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.commits.iter()
    }
}

impl<D: Delta + Clone, M: Clone> Delta for Timeline<D, M> {
    fn aggregate(&mut self, next: &Self) {
        self.aggregate_owned(next.clone());   
    }

    fn aggregate_owned(&mut self, next: Self)
    where
        Self: Sized
    {
        self.commits.extend(next.commits);
    }
}

#[cfg(feature = "deku")]
impl<D, M, Ctx> DekuWriter<Ctx> for Timeline<D, M>
where 
    Ctx: Copy,
    D: Delta,
    Commit<D, M>: DekuWriter<Ctx>,
{
    fn to_writer<W: Write + Seek>(&self, writer: &mut Writer<W>, ctx: Ctx) -> Result<(), DekuError> {
        write_commits(&self.commits, writer, ctx)
    }
}

#[cfg(feature = "deku")]
impl<'a, D, M, Ctx> DekuReader<'a, Ctx> for Timeline<D, M>
where 
    Ctx: Copy,
    D: Delta,
    Commit<D, M>: DekuReader<'a, Ctx>,
{
    fn from_reader_with_ctx<R: Read + Seek>(reader: &mut Reader<R>, ctx: Ctx) -> Result<Self, DekuError>
    where
        Self: Sized {
        let commits = read_commits(reader, ctx)?;
        Ok(Self {
            commits
        })
    }
}

#[cfg(feature = "deku")]
fn write_commits<W, D, M, Ctx>(commits: &[Commit<D, M>], writer: &mut Writer<W>, ctx: Ctx) -> Result<(), DekuError>
where 
    W: Write + Seek,
    Ctx: Copy,
    D: Delta,
    Commit<D, M>: DekuWriter<Ctx>,
{
    (commits.len() as u64).to_writer(writer, Endian::Big)?;
    commits.to_writer(writer, ctx)
}

#[cfg(feature = "deku")]
fn read_commits<'a, R, D, M, Ctx>(reader: &mut Reader<R>, ctx: Ctx) -> Result<Vec<Commit<D, M>>, DekuError>
where 
    R: Read + Seek,
    Ctx: Copy,
    D: Delta,
    Commit<D, M>: DekuReader<'a, Ctx>,
{
    let len = u64::from_reader_with_ctx(reader, Endian::Big)?;
    Vec::<Commit<D, M>>::from_reader_with_ctx(reader, (Limit::new_count(len as usize), ctx))
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
    T: Diff<D> + Clone,
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StateCachedTimeline<T, D, M = ()>
where
    T: Diff<D> + Clone,
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
    T: Diff<D> + Clone,
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

    /// Build the timeline from the commits, interval, and an initial state.
    pub fn from_commits(
        state_cache_interval: usize,
        initial_state: T,
        iter: impl IntoIterator<Item = Commit<D, M>>,
    ) -> Self {
        let mut t = Self::new(state_cache_interval, initial_state);
        t.extend(iter);
        t
    }

    /// Use [`T::default`][Default::default] for the default state and
    /// build the timeline from the commits and interval.
    pub fn from_commits_with_default(
        state_cache_interval: usize,
        iter: impl IntoIterator<Item = Commit<D, M>>,
    ) -> Self
    where 
        T: Default,
    {
        Self::from_commits(state_cache_interval, T::default(), iter)
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

    /// Computes the aggregate delta across a range of commits in `O(state_cached_interval)` time
    /// instead of `O(N)` linear delta aggregation.
    ///
    /// It reconstructs the state before `start` and the state before `end`,
    /// then calls [`diff`][Differentiable::diff] directly between them.
    /// 
    /// This is almost always preferable to using regular [`get_aggregate`][Timeline::get_aggregate],
    /// except for when the cost of diffing two states completely eclipses the cost of aggregating `N` deltas.
    pub fn get_aggregate_diffed(&self, range: impl RangeBounds<usize>) -> Option<D> {
        let (start, end) = self.timeline.get_bounds(range)?;

        let state_start = self.get_state_before(start)?;
        let state_end = self.get_state_before(end)?;

        Some(state_end.diff(&state_start))
    }

    /// Computes a direct differential delta between any two frame indices (`from` and `to`).
    ///
    /// This works bidirectionally:
    /// - Forward (`from < to`): yields the aggregated forward delta.
    /// - Backward (`from > to`): yields an inverted delta that transforms state `from` into state `to`.
    ///
    /// Running time is `O(state_cached_interval)` worst-case regardless of how far apart the frames are.
    pub fn delta_between(&self, from: usize, to: usize) -> Option<D> {
        if from > self.timeline.len() || to > self.timeline.len() {
            return None;
        }

        let state_from = self.get_state_before(from)?;
        let state_to = self.get_state_before(to)?;

        Some(state_to.diff(&state_from))
    }

    /// Whether there is a hanging (final) state
    /// in the cache.
    fn has_hanging(&self) -> bool {
        self.states.len() > self.timeline.len() / self.state_cache_interval
    }
}

impl<T, D, M> Extend<Commit<D, M>> for StateCachedTimeline<T, D, M>
where
    T: Diff<D> + Clone,
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
    T: Diff<D> + Clone,
    D: Delta,
{
    type Target = Timeline<D, M>;

    fn deref(&self) -> &Self::Target {
        &self.timeline
    }
}

/// A context type used in [`StateCachedTimeline`]'s `deku` implementation.
/// Dictates whether the cache will be serialized/deserialized along with everything else,
/// or ommitted and regenerated.
#[cfg(feature = "deku")]
#[derive(Debug, Clone, Copy, Default)]
pub enum IncludeCache {
    /// Include just the raw [`Timeline`] in deku serialization/deserialization.
    TimelineOnly,
    /// Include both the state cache and commits in deku serialization/deserialization.
    #[default]
    CacheAndTimeline
}

#[cfg(feature = "deku")]
impl<T, D, M, Ctx1, Ctx2> DekuWriter<(IncludeCache, Ctx1, Ctx2)> for StateCachedTimeline<T, D, M>
where 
    Ctx1: Copy,
    Ctx2: Copy,
    T: Diff<D> + Clone + DekuWriter<Ctx2>,
    D: Delta,
    Commit<D, M>: DekuWriter<Ctx1>,
{
    fn to_writer<W: Write + Seek>(&self, writer: &mut Writer<W>, (include_cache, ctx1, ctx2): (IncludeCache, Ctx1, Ctx2)) -> Result<(), DekuError> {
        self.timeline.to_writer(writer, ctx1)?;
        (self.state_cache_interval as u64).to_writer(writer, Endian::Big)?;

        match include_cache {
            IncludeCache::TimelineOnly => {
                // initial state. always valid to index at 0 due to the inveriants of this data structure.
                self.states[0].to_writer(writer, ctx2)?;
            },
            IncludeCache::CacheAndTimeline => {
                // don't need to write the length of the vector because it
                // can be calculated from the interval and existing length.
                // however, we need to tell the receiver whether there was an additional hanging state.
                self.has_hanging().to_writer(writer, ())?;
                self.states.to_writer(writer, ctx2)?;
            }
        }

        Ok(())
    }
}

// rust compiler doesn't like this impl :(
// need to settle for something less generic.
// it's possible with a newtype with phantomdata, but i don't think that's worth it.
//
// #[cfg(feature = "deku")]
// impl<T, D, M, Ctx1, Ctx2> DekuWriter<()> for StateCachedTimeline<T, D, M>
// where 
//     Ctx1: Copy + Default,
//     Ctx2: Copy + Default,
//     T: Diff<D> + Clone + DekuWriter<Ctx2>,
//     D: Delta,
//     Commit<D, M>: DekuWriter<Ctx1>
// {
//     fn to_writer<W: Write + Seek>(&self, writer: &mut Writer<W>, _: (),) -> Result<(), DekuError> {
//         self.to_writer(writer, (IncludeCache::default(), Ctx1::default(), Ctx2::default()))
//     }
// }

#[cfg(feature = "deku")]
impl<'a, T, D, M, Ctx1, Ctx2> DekuReader<'a, (IncludeCache, Ctx1, Ctx2)> for StateCachedTimeline<T, D, M>
where 
    Ctx1: Copy,
    Ctx2: Copy,
    T: Diff<D> + Clone + DekuReader<'a, Ctx2>,
    D: Delta,
    Commit<D, M>: DekuReader<'a, Ctx1>,
{
    fn from_reader_with_ctx<R: Read + Seek>(reader: &mut Reader<R>, (include_cache, ctx1, ctx2): (IncludeCache, Ctx1, Ctx2)) -> Result<Self, DekuError>
    where
        Self: Sized
    {
        let timeline = Timeline::from_reader_with_ctx(reader, ctx1)?;
        let state_cache_interval = u64::from_reader_with_ctx(reader, Endian::Big)? as usize;

        match include_cache {
            IncludeCache::TimelineOnly => {
                let initial_state = T::from_reader_with_ctx(reader, ctx2)?;
                Ok(Self::from_timeline(state_cache_interval, timeline, initial_state))
            },
            IncludeCache::CacheAndTimeline => {
                let has_hanging: bool = bool::from_reader_with_ctx(reader, ())?;
                let states_len = timeline.len() / state_cache_interval + has_hanging as usize;
                let states = Vec::<T>::from_reader_with_ctx(reader, (Limit::new_count(states_len), ctx2))?;
                Ok(Self {
                    timeline,
                    state_cache_interval,
                    states
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

use crate::delta::{ArithmeticDelta, Patch};

    use super::*;

    // --- Commit Tests ---

    #[test]
    fn test_commit_creation_and_deref() {
        let commit = Commit::new(ArithmeticDelta(5i32), "metadata");
        assert_eq!(commit.meta(), &"metadata");
        // Deref to ArithmeticDelta<i32>
        assert_eq!(commit.0, 5);

        let commit_default_meta: Commit<ArithmeticDelta<i32>, String> =
            Commit::new_with_default(ArithmeticDelta(10));
        assert_eq!(commit_default_meta.meta(), "");
        assert_eq!(commit_default_meta.0, 10);
    }

    // --- Timeline Tests ---

    #[test]
    fn test_timeline_basic_operations() {
        let mut timeline = Timeline::new();
        assert!(timeline.is_empty());
        assert_eq!(timeline.len(), 0);

        timeline.push(Commit::new(ArithmeticDelta(10i32), ()));
        timeline.push(Commit::new(ArithmeticDelta(20i32), ()));
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].0, 10);
        assert_eq!(timeline[1].0, 20);

        let popped = timeline.pop().expect("Should pop a commit");
        assert_eq!(popped.0, 20);
        assert_eq!(timeline.len(), 1);

        timeline.clear();
        assert!(timeline.is_empty());
    }

    #[test]
    fn test_timeline_truncate() {
        let mut timeline: Timeline<ArithmeticDelta<i32>, ()> = vec![
            Commit::new(ArithmeticDelta(1), ()),
            Commit::new(ArithmeticDelta(2), ()),
            Commit::new(ArithmeticDelta(3), ()),
        ]
        .into_iter()
        .collect();

        timeline.truncate(2);
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[1].0, 2);

        timeline.truncate(10);
        assert_eq!(timeline.len(), 2);
    }

    #[test]
    fn test_timeline_pop_aggregate() {
        let mut timeline: Timeline<ArithmeticDelta<i32>, ()> = vec![
            Commit::new(ArithmeticDelta(1), ()),
            Commit::new(ArithmeticDelta(2), ()),
            Commit::new(ArithmeticDelta(3), ()),
            Commit::new(ArithmeticDelta(4), ()),
        ]
        .into_iter()
        .collect();

        // Pop last 2 commits: 3 + 4 = 7
        let agg = timeline.pop_aggregate(2);
        assert_eq!(agg, Some(ArithmeticDelta(7)));
        assert_eq!(timeline.len(), 2);

        // Asking for more elements than timeline size returns None
        assert_eq!(timeline.pop_aggregate(5), None);
        assert_eq!(timeline.len(), 2);

        // Popping 0 elements should return None
        assert_eq!(timeline.pop_aggregate(0), None);
        assert_eq!(timeline.len(), 2);

        // Pop remaining elements
        let agg_all = timeline.pop_aggregate(2);
        assert_eq!(agg_all, Some(ArithmeticDelta(3)));
        assert!(timeline.is_empty());
    }

    #[test]
    fn test_timeline_get_aggregate() {
        let timeline: Timeline<ArithmeticDelta<i32>, ()> = vec![
            Commit::new(ArithmeticDelta(10), ()),
            Commit::new(ArithmeticDelta(20), ()),
            Commit::new(ArithmeticDelta(30), ()),
            Commit::new(ArithmeticDelta(40), ()),
        ]
        .into_iter()
        .collect();

        assert_eq!(timeline.get_aggregate(..), Some(ArithmeticDelta(100)));
        assert_eq!(timeline.get_aggregate(1..3), Some(ArithmeticDelta(50)));
        assert_eq!(timeline.get_aggregate(2..=3), Some(ArithmeticDelta(70)));

        // Empty / Invalid Ranges
        assert_eq!(timeline.get_aggregate(2..2), None);
        assert_eq!(timeline.get_aggregate(3..1), None);
        assert_eq!(timeline.get_aggregate(0..10), None);
    }

    #[test]
    fn test_timeline_merge_commits() {
        let mut timeline: Timeline<ArithmeticDelta<i32>, &'static str> = vec![
            Commit::new(ArithmeticDelta(1), "a"),
            Commit::new(ArithmeticDelta(2), "b"),
            Commit::new(ArithmeticDelta(3), "c"),
            Commit::new(ArithmeticDelta(4), "d"),
        ]
        .into_iter()
        .collect();

        // Merge index 1..3 (commits 2 and 3 -> aggregated to 5)
        let merged_idx = timeline.merge_commits(1..3, "b+c");
        assert_eq!(merged_idx, Some(1));
        assert_eq!(timeline.len(), 3);

        assert_eq!(timeline[0].0, 1);
        assert_eq!(*timeline[0].meta(), "a");

        assert_eq!(timeline[1].0, 5);
        assert_eq!(*timeline[1].meta(), "b+c");

        assert_eq!(timeline[2].0, 4);
        assert_eq!(*timeline[2].meta(), "d");

        // Empty range returns None
        assert_eq!(timeline.merge_commits(2..2, "empty"), None);
        // Start index out of bounds returns None
        assert_eq!(timeline.merge_commits(10..12, "oob"), None);
    }

    #[test]
    fn test_timeline_build_state() {
        let timeline: Timeline<ArithmeticDelta<i32>, ()> = vec![
            Commit::new(ArithmeticDelta(5), ()),
            Commit::new(ArithmeticDelta(10), ()),
            Commit::new(ArithmeticDelta(15), ()),
        ]
        .into_iter()
        .collect();

        let mut state: i32 = 100;
        timeline.build_state(&mut state, 0..2);
        assert_eq!(state, 115);

        let state_default: i32 = timeline.build_state_from_default(..);
        assert_eq!(state_default, 30);
    }

    #[test]
    fn test_timeline_conversions_and_iterators() {
        // FromIterator for Delta (with unit metadata)
        let t1: Timeline<ArithmeticDelta<i32>, ()> =
            vec![ArithmeticDelta(1), ArithmeticDelta(2)]
                .into_iter()
                .collect();
        assert_eq!(t1.len(), 2);

        // FromIterator for Commit
        let commits = vec![Commit::new(ArithmeticDelta(3), "meta")];
        let mut t2: Timeline<ArithmeticDelta<i32>, &'static str> =
            commits.into_iter().collect();
        assert_eq!(t2.len(), 1);

        // Extend
        t2.extend(vec![Commit::new(ArithmeticDelta(4), "meta2")]);
        assert_eq!(t2.len(), 2);

        // Reference IntoIterator
        let sum: i32 = (&t2).into_iter().map(|c| c.0).sum();
        assert_eq!(sum, 7);

        // Owned IntoIterator
        let owned_sum: i32 = t2.into_iter().map(|c| c.0).sum();
        assert_eq!(owned_sum, 7);
    }

    #[test]
    fn test_timeline_delta_trait_impl() {
        let mut t1: Timeline<ArithmeticDelta<i32>, ()> =
            vec![ArithmeticDelta(1), ArithmeticDelta(2)]
                .into_iter()
                .collect();
        let t2: Timeline<ArithmeticDelta<i32>, ()> =
            vec![ArithmeticDelta(3), ArithmeticDelta(4)]
                .into_iter()
                .collect();

        // aggregate by clone
        t1.aggregate(&t2);
        assert_eq!(t1.len(), 4);
        assert_eq!(t1[2].0, 3);

        // aggregate_owned
        t1.aggregate_owned(vec![ArithmeticDelta(5)].into_iter().collect());
        assert_eq!(t1.len(), 5);
        assert_eq!(t1[4].0, 5);
    }

    // --- StateCachedTimeline Tests ---

    #[test]
    #[should_panic(expected = "state_cache_interval must be >= 1")]
    fn test_cached_timeline_zero_interval_panics() {
        StateCachedTimeline::<i32, ArithmeticDelta<i32>, ()>::new(0, 0);
    }

    #[test]
    fn test_cached_timeline_queries() {
        // Interval = 2
        let mut ct = StateCachedTimeline::new(2, 0i32);

        ct.push(Commit::new(ArithmeticDelta(10), ())); // index 0
        ct.push(Commit::new(ArithmeticDelta(20), ())); // index 1
        ct.push(Commit::new(ArithmeticDelta(30), ())); // index 2
        ct.push(Commit::new(ArithmeticDelta(40), ())); // index 3

        assert_eq!(ct.get_state_before(0), Some(0));
        assert_eq!(ct.get_state_before(1), Some(10));
        assert_eq!(ct.get_state_before(2), Some(30));
        assert_eq!(ct.get_state_before(3), Some(60));
        assert_eq!(ct.get_state_before(4), Some(100));
        assert_eq!(ct.get_state_before(5), None);

        assert_eq!(ct.get_state_after(0), Some(10));
        assert_eq!(ct.get_state_after(1), Some(30));
        assert_eq!(ct.get_state_after(2), Some(60));
        assert_eq!(ct.get_state_after(3), Some(100));
        assert_eq!(ct.get_state_after(4), None);

        assert_eq!(ct.current_state(), 100);
    }

    #[test]
    fn test_cached_timeline_get_nearest_state() {
        let mut ct = StateCachedTimeline::new(3, 10i32);
        for i in 1..=6 {
            ct.push(Commit::new(ArithmeticDelta(i), ()));
        }

        // Cache positions created during push:
        // index 0 (initial state = 10)
        // index 3 (state = 10 + 1 + 2 + 3 = 16)

        let (idx_0, state_0) = ct.get_nearest_state(2).unwrap();
        assert_eq!(idx_0, 0);
        assert_eq!(state_0, &10);

        let (idx_3, state_3) = ct.get_nearest_state(4).unwrap();
        assert_eq!(idx_3, 3);
        assert_eq!(state_3, &16);

        // Before pushing commit 6, nearest cache for index 6 is index 3
        let (idx_6_before, state_6_before) = ct.get_nearest_state(6).unwrap();
        assert_eq!(idx_6_before, 3);
        assert_eq!(state_6_before, &16);

        // Pushing 7th commit (index 6) caches the state before index 6
        ct.push(Commit::new(ArithmeticDelta(7), ()));
        let (idx_6, state_6) = ct.get_nearest_state(6).unwrap();
        assert_eq!(idx_6, 6);
        assert_eq!(state_6, &31);

        assert_eq!(ct.get_nearest_state(8), None);
    }

    #[test]
    fn test_cached_timeline_constructors() {
        let timeline: Timeline<ArithmeticDelta<i32>, ()> = vec![
            Commit::new(ArithmeticDelta(5), ()),
            Commit::new(ArithmeticDelta(15), ()),
            Commit::new(ArithmeticDelta(25), ()),
        ]
        .into_iter()
        .collect();

        // from_timeline
        let ct1 = StateCachedTimeline::from_timeline(2, timeline, 100i32);
        assert_eq!(ct1.current_state(), 145);
        assert_eq!(ct1.get_state_before(2), Some(120));

        // from_commits
        let ct2 = StateCachedTimeline::from_commits(
            1,
            50i32,
            vec![Commit::new(ArithmeticDelta(10), ()), Commit::new(ArithmeticDelta(20), ())],
        );
        assert_eq!(ct2.current_state(), 80);

        // from_commits_with_default
        let ct3 = StateCachedTimeline::<i32, ArithmeticDelta<i32>, ()>::from_commits_with_default(
            2,
            vec![Commit::new(ArithmeticDelta(7), ())],
        );
        assert_eq!(ct3.current_state(), 7);
    }

    #[test]
    fn test_cached_timeline_set_initial_state() {
        let mut ct = StateCachedTimeline::new(2, 0i32);
        ct.push(Commit::new(ArithmeticDelta(10), ()));
        ct.push(Commit::new(ArithmeticDelta(20), ()));
        ct.push(Commit::new(ArithmeticDelta(30), ()));

        assert_eq!(ct.current_state(), 60);

        // Re-base with a new initial state
        ct.set_initial_state(100);

        assert_eq!(ct.get_state_before(0), Some(100));
        assert_eq!(ct.get_state_before(1), Some(110));
        assert_eq!(ct.get_state_before(2), Some(130));
        assert_eq!(ct.current_state(), 160);
    }

    #[test]
    fn test_cached_timeline_pop_truncate_clear() {
        let mut ct = StateCachedTimeline::new(2, 0i32);
        for _ in 0..5 {
            ct.push(Commit::new(ArithmeticDelta(10), ()));
        }

        assert_eq!(ct.current_state(), 50);

        let popped = ct.pop();
        assert_eq!(popped.unwrap().0, 10);
        assert_eq!(ct.len(), 4);
        assert_eq!(ct.current_state(), 40);

        ct.truncate(1);
        assert_eq!(ct.len(), 1);
        assert_eq!(ct.current_state(), 10);

        ct.clear();
        assert_eq!(ct.len(), 0);
        assert_eq!(ct.current_state(), 0);
    }

    #[test]
    fn test_cached_timeline_extend() {
        let mut ct = StateCachedTimeline::new(2, 5i32);

        ct.extend(vec![
            Commit::new(ArithmeticDelta(10), ()),
            Commit::new(ArithmeticDelta(20), ()),
            Commit::new(ArithmeticDelta(30), ()),
        ]);

        assert_eq!(ct.len(), 3);
        assert_eq!(ct.get_state_before(0), Some(5));
        assert_eq!(ct.get_state_before(2), Some(35));
        assert_eq!(ct.current_state(), 65);
    }

    #[test]
    fn test_wrapping_arithmetic_overflow_behavior() {
        let mut ct = StateCachedTimeline::new(1, 250u8);
        ct.push(Commit::new(ArithmeticDelta(10u8), ()));

        assert_eq!(ct.current_state(), 4u8);

        let delta = 4u8.diff(&250u8);
        assert_eq!(delta.0, 10u8);
    }

    #[test]
    fn test_state_cached_timeline_get_aggregate_diffed() {
        // Initial state = 0, interval = 2
        // Commits: +10, +20, +30, +40
        // State 0 (initial) = 0
        // State 1 = 10
        // State 2 = 30
        // State 3 = 60
        // State 4 = 100
        let commits = vec![
            Commit::new(ArithmeticDelta(10), ()),
            Commit::new(ArithmeticDelta(20), ()),
            Commit::new(ArithmeticDelta(30), ()),
            Commit::new(ArithmeticDelta(40), ()),
        ];
        let sct = StateCachedTimeline::from_commits(2, 0i32, commits);

        // Full range (matches standard get_aggregate)
        assert_eq!(sct.get_aggregate_diffed(..), Some(ArithmeticDelta(100)));
                assert_eq!(sct.get_aggregate_diffed(..), sct.get_aggregate(..));

        // Sub-range 1..3 (commits 1 & 2 -> 20 + 30 = 50)
        assert_eq!(sct.get_aggregate_diffed(1..3), Some(ArithmeticDelta(50)));
        assert_eq!(sct.get_aggregate_diffed(1..3), sct.get_aggregate(1..3));

        // Inclusive range 2..=3 (commits 2 & 3 -> 30 + 40 = 70)        
        assert_eq!(sct.get_aggregate_diffed(2..=3), Some(ArithmeticDelta(70)));
        assert_eq!(sct.get_aggregate_diffed(2..=3), sct.get_aggregate(2..=3));

        // Empty / Invalid Ranges
        assert_eq!(sct.get_aggregate_diffed(2..2), None);
        assert_eq!(sct.get_aggregate_diffed(3..1), None);
        assert_eq!(sct.get_aggregate_diffed(0..10), None);
    }

    #[test]
    fn test_state_cached_timeline_diff_between() {
        // Initial state = 100, cache interval = 3
        // Commit 0 (+5)  -> state 1 = 105
        // Commit 1 (+15) -> state 2 = 120
        // Commit 2 (+25) -> state 3 = 145
        // Commit 3 (-10) -> state 4 = 135
        let commits = vec![
            Commit::new(ArithmeticDelta(5), ()),
            Commit::new(ArithmeticDelta(15), ()),
            Commit::new(ArithmeticDelta(25), ()),
            Commit::new(ArithmeticDelta(-10), ()),
        ];
        let sct = StateCachedTimeline::from_commits(3, 100i32, commits);

        // Forward diffs
        assert_eq!(sct.delta_between(0, 4), Some(ArithmeticDelta(35)));  // 135 - 100 = 35
        assert_eq!(sct.delta_between(1, 3), Some(ArithmeticDelta(40)));  // 145 - 105 = 40

        // Backward diffs (Delta Inversion)
        assert_eq!(sct.delta_between(4, 0), Some(ArithmeticDelta(-35))); // 100 - 135 = -35
        assert_eq!(sct.delta_between(3, 1), Some(ArithmeticDelta(-40))); // 105 - 145 = -40

        // Same position yields zero delta
        assert_eq!(sct.delta_between(2, 2), Some(ArithmeticDelta(0)));

        // Verify patching with inverted delta correctly restores an older state
        let mut current_state = sct.get_state_before(4).unwrap(); // 135
        let inverted_delta = sct.delta_between(4, 1).unwrap();     // -30
        current_state.patch(&inverted_delta);
        assert_eq!(current_state, sct.get_state_before(1).unwrap()); // 105

        // Out of bounds bounds check
        assert_eq!(sct.delta_between(0, 10), None);
        assert_eq!(sct.delta_between(10, 0), None);
    }

    #[cfg(test)]
    mod deku_tests {
        use super::*; // Make sure this imports Commit, Timeline, StateCachedTimeline, ArithmeticDelta, IncludeCache, etc.
        use std::io::Cursor;

        // A mock metadata struct to test the tuple context parameters `(Ctx1, Ctx2)`
        #[derive(Debug, PartialEq, DekuRead, DekuWrite, Clone)]
        struct TestMeta(u8);

        #[test]
        fn test_commit_deku_serialization() {
            let commit = Commit::new(ArithmeticDelta(42u32), TestMeta(7));
            
            // Pass Big Endian down to both the ArithmeticDelta and TestMeta
            let ctx = (deku::ctx::Endian::Big, ());
            
            // 1. Serialize using Writer + Cursor
            let mut buf = Vec::new();
            let mut cursor_write = Cursor::new(&mut buf);
            let mut writer = Writer::new(&mut cursor_write);
            commit.to_writer(&mut writer, ctx).expect("Failed to serialize Commit");
            
            // 2. Deserialize using Reader + Cursor
            let mut cursor_read = Cursor::new(&buf);
            let mut reader = Reader::new(&mut cursor_read);
            let deserialized = Commit::<ArithmeticDelta<u32>, TestMeta>::from_reader_with_ctx(&mut reader, ctx)
                .expect("Failed to deserialize Commit");
            
            assert_eq!(commit, deserialized);
        }

        #[test]
        fn test_timeline_deku_serialization() {
            let mut timeline = Timeline::new();
            timeline.push(Commit::new(ArithmeticDelta(10u32), TestMeta(1)));
            timeline.push(Commit::new(ArithmeticDelta(20u32), TestMeta(2)));
            timeline.push(Commit::new(ArithmeticDelta(30u32), TestMeta(3)));

            let ctx = (deku::ctx::Endian::Big, ());

            // Serialize
            let mut buf = Vec::new();
            let mut cursor_write = Cursor::new(&mut buf);
            let mut writer = Writer::new(&mut cursor_write);
            timeline.to_writer(&mut writer, ctx).expect("Failed to serialize Timeline");

            // Deserialize
            let mut cursor_read = Cursor::new(&buf);
            let mut reader = Reader::new(&mut cursor_read);
            let deserialized = Timeline::<ArithmeticDelta<u32>, TestMeta>::from_reader_with_ctx(&mut reader, ctx)
                .expect("Failed to deserialize Timeline");

            assert_eq!(timeline, deserialized);
        }

        #[test]
        fn test_empty_timeline_deku_serialization() {
            let timeline = Timeline::<ArithmeticDelta<u32>, TestMeta>::new();
            let ctx = (deku::ctx::Endian::Big, ());

            let mut buf = Vec::new();
            let mut cursor_write = Cursor::new(&mut buf);
            let mut writer = Writer::new(&mut cursor_write);
            timeline.to_writer(&mut writer, ctx).expect("Failed to serialize empty Timeline");

            let mut cursor_read = Cursor::new(&buf);
            let mut reader = Reader::new(&mut cursor_read);
            let deserialized = Timeline::<ArithmeticDelta<u32>, TestMeta>::from_reader_with_ctx(&mut reader, ctx)
                .expect("Failed to deserialize empty Timeline");

            assert_eq!(timeline, deserialized);
        }

        #[test]
        fn test_state_cached_timeline_deku_serialization() {
            let mut sct = StateCachedTimeline::<u32, ArithmeticDelta<u32>, TestMeta>::new(2, 100);
            sct.push(Commit::new(ArithmeticDelta(10u32), TestMeta(1)));
            sct.push(Commit::new(ArithmeticDelta(20u32), TestMeta(2)));

            // StateCachedTimeline expects a 3-part context: (IncludeCache, DeltaCtx, MetaCtx)
            // If your caching logic uses a different default enum variant, adjust `IncludeCache::Yes` to match.
            let ctx = (IncludeCache::CacheAndTimeline, (deku::ctx::Endian::Big, ()), deku::ctx::Endian::Big);

            // Serialize
            let mut buf = Vec::new();
            let mut cursor_write = Cursor::new(&mut buf);
            let mut writer = Writer::new(&mut cursor_write);
            sct.to_writer(&mut writer, ctx).expect("Failed to serialize StateCachedTimeline");

            // Deserialize
            let mut cursor_read = Cursor::new(&buf);
            let mut reader = Reader::new(&mut cursor_read);
            let deserialized = StateCachedTimeline::<u32, ArithmeticDelta<u32>, TestMeta>::from_reader_with_ctx(&mut reader, ctx)
                .expect("Failed to deserialize StateCachedTimeline");
            
            // Assert properties instead of exact equality if caching state doesn't fully serialize,
            // or just use standard Eq checks if your Deku impl matches it byte-for-byte.
            assert_eq!(sct.len(), deserialized.len());
            assert_eq!(sct.current_state(), deserialized.current_state());
            assert_eq!(sct.get_state_before(0), deserialized.get_state_before(0));
        }
    }
}