// TODO find a home for these things

use std::ops::{Deref, DerefMut};

use crate::{Delta, Diff};

/// A wrapper that holds two copies of a type and tracks
/// changes between them to generate deltas on demand.
#[derive(Debug, Clone, Default)]
pub struct ChangeBucket<T> {
    old: T,
    new: T,
}

impl<T: Clone> ChangeBucket<T> {
    pub fn new(initial: T) -> Self {
        Self {
            old: initial.clone(),
            new: initial,
        }
    }

    /// Computes the delta from `old` to `new` and resets the `old` baseline
    /// to match `new`.
    pub fn pop<D>(&mut self) -> D
    where
        T: Diff<D>,
        D: Delta,
    {
        let delta = self.new.diff(&self.old);
        self.old = self.new.clone();
        delta
    }

    /// Inspects the current delta without resetting the baseline (`old`).
    pub fn peek<D>(&self) -> D
    where
        T: Diff<D>,
        D: Delta,
    {
        self.new.diff(&self.old)
    }

    /// Reverts `new` back to the last committed `old` state.
    pub fn discard(&mut self) {
        self.new = self.old.clone();
    }

    /// Advances the `old` baseline to match `new` without producing a delta.
    pub fn checkpoint(&mut self) {
        self.old = self.new.clone();
    }
}

impl<T: PartialEq> ChangeBucket<T> {
    /// Returns `true` if `new` has diverged from `old`.
    pub fn is_dirty(&self) -> bool {
        self.old != self.new
    }
}

impl<T> Deref for ChangeBucket<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.new
    }
}

impl<T> DerefMut for ChangeBucket<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.new
    }
}