//! A library for computing, composing, and applying **deltas** between states.
//!
//! # Core Traits
//!
//! - [`Differentiable<D>`][delta::Differentiable] — implemented by types that can produce a
//!   delta `D` relative to another instance of themselves, and apply one via `patch`.
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
//! # Timelines
//!
//! [`Timeline<D, M>`][timeline::Timeline] stores an ordered sequence of
//! [`Commit<D, M>`][timeline::Commit] values and supports aggregate queries, range merging, and
//! state replay.
//!
//! [`StateCachedTimeline<T, D, M>`][timeline::StateCachedTimeline] adds periodic state
//! snapshots at a configurable interval, reducing arbitrary state-retrieval and delta-query
//! cost from `O(N)` to `O(interval)`. It also supports efficient **bidirectional** seeking via
//! [`delta_between`][timeline::StateCachedTimeline::delta_between] — the same call handles both
//! forward and inverse (backward) deltas.
//!
//! # Utilities
//!
//! [`ChangeBucket<T, D>`][util::ChangeBucket] holds "before" and "after" copies of a value.
//! Mutate it freely, then call `pop()` to extract the accumulated delta and reset the
//! baseline — handy for pipelines that only want to emit a commit when something actually
//! changed.
//!
//! # Feature Flags
//!
//! | Feature | Description |
//! |---|---|
//! | `macros` | Enables `#[derive(Differentiable)]` for auto-generating delta types for structs |
//! | `serde` | Derives `serde::Serialize`/`Deserialize` on all delta and timeline types |

pub mod delta;
pub mod timeline;
pub mod util;

pub use delta::{Delta, Differentiable};

#[cfg(feature = "macros")]
pub use deltapatcher_macros as macros;