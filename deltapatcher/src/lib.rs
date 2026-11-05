#![cfg_attr(not(feature = "std"), no_std)]

//! A library for computing, composing, and applying **deltas** between states.
//!
//! # Core Traits
//!
//! - [`Patch<D>`] — implemented by types that can apply a delta `D` to mutate their state in-place.
//! - [`Diff<D>`] — implemented by types that can produce a delta `D` relative to an initial state (requires [`Patch<D>`]).
//! - [`Delta`][delta::Delta] — implemented by the delta type itself; the key operation is
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

pub use delta::{Delta, Diff, Patch};

#[cfg(feature = "macros")]
pub use deltapatcher_macros as macros;

#[cfg(feature = "alloc")]
extern crate alloc;