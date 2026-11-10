#![cfg_attr(not(feature = "std"), no_std)]

//! A library for computing, composing, and applying **deltas** between states.
//!
//! # Core Traits
//!
//! - [`Patch<D>`] — implemented by types that can apply a delta `D` to mutate their state in-place.
//! - [`Diff<D>`] — implemented by types that can produce a delta `D` relative to an initial state (requires [`Patch<D>`]).
//! - [`Delta`] — implemented by the delta type itself; the key operation is
//!   `aggregate`, which composes two consecutive deltas into one. An aggregated `A → C` delta
//!   lets you reproduce the final state without ever materialising the intermediate `B`.
//!
//! # Built-in Implementations
//!
//! | Type | Delta | Notes |
//! |---|---|---|
//! | `Vec<T>`, `[T]`, `[T; N]` | [`SliceDelta<T>`][delta::SliceDelta] | LCS-based splice diff |
//! | `u8`, `i8`, … `u64`, `i64` | [`ArithmeticDelta<T>`][delta::ArithmeticDelta] | Wrapping addition; any type implementing [`WrappingArithmetic`][delta::WrappingArithmetic] gets this for free |
//!
//! # Macros
//!
//! When the `macros` feature is enabled, procedural macros auto-generate delta types and trait implementations for your custom structs and enums:
//!
//! - [`#[derive(Diff)]`][Diff] — derives both `Diff` and `Patch` for full bidirectional diffing and patching.
//! - [`#[derive(Patch)]`][Patch] — derives only `Patch` (skipping diff generation), ideal for client-side state receivers or read-only replicas.
//!
//! # Timelines
//!
//! [`Timeline<D, M>`][timeline::Timeline] stores an ordered sequence of
//! [`Commit<D, M>`][timeline::Commit] values and supports aggregate queries, range merging, and
//! state replay.
//!
//! [`StateCachedTimeline<T, D, M>`][timeline::StateCachedTimeline] adds periodic state
//! snapshots at a configurable interval, reducing arbitrary state-retrieval and delta-query
//! cost from `O(N)` to `O(interval)`. It also supports efficient **bidirectional** seeking via
//! [`diff_between`][timeline::StateCachedTimeline::diff_between] — the same call handles both
//! forward and inverse (backward) deltas.
//!
//! # Utilities
//!
//! [`ChangeBucket<T>`][util::ChangeBucket] holds "old" and "new" copies of a value.
//! Mutate `new` freely through `DerefMut`, then call `pop()` to extract the accumulated delta and reset the
//! baseline — handy for pipelines that only want to emit a commit when something actually
//! changed.
//!
//! # Feature Flags
//!
//! | Feature | Description |
//! |---|---|
//! | `std` | Enabled by default, adding a dependency on the standard library. Remove this feature for `no_std` environments. |
//! | `alloc` | Enabled by default, linking the `alloc` crate. This is required for timelines and allocating deltas such as `SliceDelta`.
//! | `macros` | Enables `#[derive(Diff)]` and `#[derive(Patch)]` proc-macros for structs and enums |
//! | `serde` | Derives `serde::Serialize`/`Deserialize` on all delta and timeline types |

pub mod delta;
pub mod util;

#[cfg(feature = "alloc")]
pub mod timeline;

#[cfg(feature = "macros")]
pub use deltapatcher_macros as macros;

#[cfg(feature = "macros")]
pub use deltapatcher_macros::{Diff, Patch};

#[cfg(feature = "alloc")]
extern crate alloc;


/// Trait for types that can compute the difference (delta)
/// relative to another state of the same type.
///
/// `Diff` represents the generator side of diff patching.
/// Calling `b.diff(&a)` calculates the delta needed to transform `a` into `b`.
///
/// The delta type `D` is **not** required to implement [`Delta`]. `Diff` and
/// [`Patch`] are general-purpose, auxiliary traits: a delta type that only
/// needs to be produced and applied (for example, a single-use-case delta used
/// internally) can implement them without also supporting aggregation. Delta
/// types that implement [`Delta`] are the "complete" deltas that can be used by
/// downstream utilities which need to compose changes.
///
/// # Contract
///
/// **Round-Trip Invariant**: If a type `T` implements both [`Diff<D>`] and [`Patch<D>`], the following identity
/// should hold for any states `a` and `b`:
///
/// ```rust,ignore
/// let mut state = a.clone();
/// let delta = b.diff(&a);
/// state.patch(&delta);
/// assert_eq!(state, b);
/// ```
///
/// In order to help enforce this invariant, it is required that all types capable
/// of producing a delta (i.e. types implementing [`Diff`]) must also be capable of
/// consuming the same type of delta (via [`Patch`]).
///
/// # Examples
///
/// ```rust
/// use deltapatcher::{Diff, Patch, delta::ArithmeticDelta};
///
/// let start = 10i32;
/// let target = 25i32;
/// let delta = target.diff(&start);
/// assert_eq!(delta, ArithmeticDelta(15));
/// ```
pub trait Diff<D>: Patch<D> {
    /// Computes and returns the delta that transforms `initial` into `self`.
    fn diff(&self, initial: &Self) -> D;
}

/// Trait for types that can be updated in-place by applying a delta.
///
/// `Patch` represents the consumption side of differential patching. A type
/// implementing [`Patch<D>`] can mutate its state using a delta `D`, without
/// requiring the capability to compute diffs itself.
///
/// The delta type `D` is **not** required to implement [`Delta`]; see the
/// [`Diff`] docs for the rationale.
///
/// # Examples
///
/// ```rust
/// use deltapatcher::{Patch, delta::ArithmeticDelta};
///
/// let mut value = 10i32;
/// value.patch(&ArithmeticDelta(5));
/// assert_eq!(value, 15);
/// ```
pub trait Patch<D> {
    /// Apply `delta` to `self` in place, advancing it from its current state
    /// to a new state along the delta.
    ///
    /// # Contract
    /// **Round-Trip Invariant**: For a given call to `patch(&mut self, delta)`, if the delta parameter was
    /// generated by [`diff`][Diff::diff] with `self` as the initial state, then the resulting value of `self`
    /// after the call to [`patch`][Self::patch] must be equivalent to the final state passed to [`diff`][Diff::diff].
    /// See [`Diff`] docs for more info.
    ///
    /// # Arguments
    ///
    /// * `delta` — The delta between states.
    ///
    /// # Example
    ///
    /// ```rust
    /// use deltapatcher::{Diff, Patch};
    ///
    /// let before = vec![1u32, 2, 3];
    /// let after  = vec![1u32, 2, 3, 4, 5];
    ///
    /// let delta = after.diff(&before);
    ///
    /// let mut state = before.clone();
    /// state.patch(&delta);
    /// assert_eq!(state, after);
    /// ```
    fn patch(&mut self, delta: &D);

    /// A fast path for [`patch`][Self::patch] which takes an owned value.
    fn patch_owned(&mut self, delta: D) {
        self.patch(&delta);
    }
}

/// A value that encodes the change between two states and can be composed
/// with other deltas of the same type.
///
/// A delta is always expressed relative to the state *immediately before* it
/// is applied. Composing two deltas with [`aggregate`][Delta::aggregate]
/// produces a single delta that skips the intermediate state entirely —
/// useful for compressing a sequence of changes before sending them over the
/// network or storing them in a timeline.
///
/// `Delta` is deliberately independent of [`Diff`] and [`Patch`]: those traits
/// work with any delta type, whether or not it supports aggregation. Implement
/// `Delta` only for delta types that should be fully composable and usable by
/// downstream utilities (such as timelines) that need to aggregate changes.
///
/// # Contract
///
/// Aggregation must be **associative**: aggregating `(A→B)` with `(B→C)` and
/// then with `(C→D)` must produce the same result as aggregating `(A→B)` with
/// the pre-aggregated `(B→D)`.
///
/// # Implementing `Delta`
///
/// You typically pair `Delta` with [`Diff`] and [`Patch`] impls on the state
/// type, but this is not required. The only required method is
/// [`aggregate`][Delta::aggregate]; [`aggregate_owned`][Delta::aggregate_owned]
/// has a default implementation that simply borrows its argument, but you
/// should override it whenever moving the data is cheaper than cloning.
///
/// ```rust
/// use deltapatcher::Delta;
///
/// /// A delta for a simple counter: just the signed difference.
/// #[derive(Clone, Copy)]
/// struct CounterDelta(i64);
///
/// impl Delta for CounterDelta {
///     fn aggregate(&mut self, next: &Self) {
///         self.0 += next.0;
///     }
/// }
/// ```
///
/// # Example: composing deltas to skip an intermediate state
///
/// ```rust
/// use deltapatcher::{Delta, Patch, Diff};
///
/// let a = vec![1i32, 2, 3];
/// let b = vec![1i32, 9, 3];     // A → B
/// let c = vec![1i32, 9, 3, 4]; // B → C
///
/// let mut a_to_b = b.diff(&a);
/// let     b_to_c = c.diff(&b);
///
/// // Compose into a single A → C delta — `b` is never needed again.
/// a_to_b.aggregate_owned(b_to_c);
///
/// let mut result = a.clone();
/// result.patch(&a_to_b);
/// assert_eq!(result, c);
/// ```
///
/// # See also
///
/// - [`Diff`] — the trait for types that *produce* deltas.
/// - [`Patch`] — the trait for types that *consume* deltas.
/// - [`delta`] — module containing builtin delta types.
pub trait Delta {
    /// Compose `next` on top of `self` so that applying the result to the
    /// state *before* `self` transitions it all the way to the state *after*
    /// `next`, without materializing the intermediate state.
    ///
    /// After this call `self` represents the aggregated `A → C` delta, where
    /// `self` was originally `A → B` and `next` is `B → C`.
    ///
    /// Prefer [`aggregate_owned`][Delta::aggregate_owned] in hot paths where
    /// you already own `next` and the implementation can avoid a clone.
    fn aggregate(&mut self, next: &Self);

    /// Compose `next` on top of `self` by value.
    ///
    /// This is the preferred hot-path alternative to
    /// [`aggregate`][Delta::aggregate] when the caller already owns `next`,
    /// because implementations can move its internal data rather than cloning
    /// it. The default implementation simply delegates to `aggregate`, so
    /// override this whenever moving is cheaper.
    ///
    /// # Example
    ///
    /// ```rust
    /// use deltapatcher::{Delta, Patch, Diff};
    ///
    /// let a = vec![1u8, 2, 3];
    /// let b = vec![1u8, 5, 3];
    /// let c = vec![1u8, 5, 7];
    ///
    /// let mut d1 = b.diff(&a);
    /// let     d2 = c.diff(&b); // owned — no extra clone needed
    /// d1.aggregate_owned(d2);
    ///
    /// let mut result = a.clone();
    /// result.patch(&d1);
    /// assert_eq!(result, c);
    /// ```
    fn aggregate_owned(&mut self, next: Self)
    where
        Self: Sized
    {
        self.aggregate(&next);
    }
}