// TODO find a home for these things

use std::ops::{Deref, DerefMut};

use crate::{Delta, Differentiable};

/// A wrapper that holds two copies of a type and tracks
/// the difference between the initial and final value to generate a delta.
pub struct ChangeBucket<T, D>
where 
    T: Differentiable<D> + Clone,
    D: Delta
{
    old: T,
    new: T,
    _pd: std::marker::PhantomData<D>,
}

impl<T, D> ChangeBucket<T, D>
where 
    T: Differentiable<D> + Clone,
    D: Delta
{
    pub fn new(initial: T) -> Self {
        Self {
            old: initial.clone(),
            new: initial,
            _pd: std::marker::PhantomData,
        }
    }

    /// Pop the current changes into a delta.
    /// This empties the changes in the bucket.
    pub fn pop(&mut self) -> D {
        let delta = self.new.differentiate(&self.old);
        self.old = self.new.clone();
        delta
    }
}

impl<T, D> Deref for ChangeBucket<T, D>
where 
    T: Differentiable<D> + Clone,
    D: Delta
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.new
    }
}

impl<T, D> DerefMut for ChangeBucket<T, D>
where 
    T: Differentiable<D> + Clone,
    D: Delta
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.new
    }
}