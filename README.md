# deltapatcher

A Rust library for computing, composing, and applying **deltas** (diffs) between states, with built-in support for timelines, state caching, and efficient bidirectional seeking.

## Overview

`deltapatcher` provides two core traits:

- **`Differentiable<D>`** — implemented by any type that can compute a delta `D` to/from another instance of itself, and apply one.
- **`Delta`** — implemented by the delta type itself, allowing deltas to be composed (aggregated) together.

Built-in implementations are provided for `Vec<T>`, slices `[T]`, fixed-size arrays `[T; N]`, and all primitive integer types (with more planned soon).

## Features

| Feature | Description |
|---|---|
| `macros` | Enables the `#[derive(Differentiable)]` proc-macro for auto-generating delta types for structs |
| `serde` | Enables `serde::Serialize`/`Deserialize` on all delta and timeline types |

## Core Concepts

### `Differentiable` and `Delta`

```rust
use deltapatcher::{Differentiable, Delta};

let start = vec![1, 2, 3, 4, 5];
let end   = vec![1, 9, 3, 4, 10, 5];

// Compute the delta from `start` to `end`
let delta = end.diff(&start);

// Apply it to a different starting state
let mut other = start.clone();
other.patch(&delta);
assert_eq!(other, end);
```

### Delta Aggregation

Deltas can be composed together with `aggregate` / `aggregate_owned`. An aggregated delta `A → C` can reproduce the final state without ever materializing the intermediate state `B`:

```rust
let a_to_b = b.diff(&a);
let b_to_c = c.diff(&b);

let mut a_to_c = a_to_b;
a_to_c.aggregate_owned(b_to_c);

let mut result = a.clone();
result.patch(&a_to_c);
assert_eq!(result, c);
```

This is especially useful in networking scenarios (e.g. game state sync, media streaming) where you want to send a single compact delta instead of multiple incremental ones or a full snapshot.

## Built-in Delta Types

- **`SliceDelta<T>`** — LCS-based diff for `Vec<T>`, `[T]`, and `[T; N]`. Stores changes as a compact set of non-overlapping splice entries that can be correctly aggregated without knowing the intermediate state.
- **`ArithmeticDelta<T>`** — wrapping-arithmetic delta for all primitive integer types. Any type implementing the `WrappingArithmetic` trait gets a `Differentiable<ArithmeticDelta<Self>>` impl for free.

Full API details on [docs.rs](https://docs.rs/deltapatcher).

## `#[derive(Differentiable)]` Macro

Enable the `macros` feature to derive `Differentiable` for your own structs. Each field is annotated with the delta type it should use, and the macro generates the delta struct along with its `Delta` and `Differentiable` impls.

```toml
# Cargo.toml
[dependencies]
deltapatcher = { version = "*", features = ["macros"] }
```

```rust
use deltapatcher::{Differentiable, delta::ArithmeticDelta};

#[derive(Differentiable, Debug, Clone, Copy)]
#[deltapatcher(
    delta_name = PosDelta,          // optional: override the generated type name
    delta_derive(Debug, Clone),     // optional: derives to add to the generated type
)]
struct Position {
    #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
    x: i32,

    #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
    y: i32,
}

#[derive(Differentiable, Debug, Clone)]
enum State {
    A {
        #[deltapatcher(delta_ty = ArithmeticDelta<u32>)]
        foo: u32,

        #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
        bar: i32,
    },
    B(
        #[deltapatcher(delta_ty = SliceDelta<u8>)]
        Vec<u8>
    ),
    C,
}
```

Run `cargo expand` to inspect the generated code.

## Timelines

`Timeline<D, M>` is an ordered sequence of `Commit<D, M>` values (delta + optional metadata). It supports aggregate queries, range merging, and state replay. `StateCachedTimeline<T, D, M>` wraps it with periodic state snapshots, dropping arbitrary state-retrieval and delta-query cost from `O(N)` to `O(interval)`.

Full API details on [docs.rs](https://docs.rs/deltapatcher).

### Real-World Example: Video Scrubbing

`StateCachedTimeline` maps naturally onto a video player's seek model. Each frame is a commit; the server stores periodic keyframe-equivalent cached states and can answer any seek request with a lightweight delta rather than a full snapshot:

```rust
// Server: build a cached timeline from raw frames
let server_timeline = StateCachedTimeline::from_commits(
    /*interval=*/ 4,
    initial_frame,
    raw_frames.windows(2).map(|w| Commit::new_with_default(w[1].diff(&w[0]))),
);

// Client: seek forward
let delta = server_timeline.delta_between(current_frame, target_frame).unwrap();
current_frame_data.patch(&delta);

// Client: seek backward — no full snapshot needed
let inv_delta = server_timeline.delta_between(current_frame, earlier_frame).unwrap();
current_frame_data.patch(&inv_delta);
```

See [`example/src/bin/video_stream.rs`](example/src/bin/video_stream.rs) for the full runnable version.

## Utilities

**`ChangeBucket<T, D>`** holds "before" and "after" copies of a value. Mutate it freely through `DerefMut`, then call `pop()` to extract the accumulated delta and reset the baseline — useful for building pipelines that only emit commits when something actually changes.

Full API details on [docs.rs](https://docs.rs/deltapatcher).

## Running the Examples

This repo uses a dedicated `example` crate with multiple binaries. Run any example with:

```shell
just run-example basic
just run-example macros
just run-example video_stream
```

## Roadmap

- **More built-in delta types**
  - `MapDelta` — key-level insert/remove/update entries
  - `SetDelta` — insertion and removal sets for `HashSet` / `BTreeSet`
  - `BitmapDelta` — XOR-based delta for `u8`/`u32`/`u64` bitfields, useful for flag sets and masks
  - `StringDelta` — character- or byte-level diff for `String` / `&str`, wrapping `SliceDelta` with UTF-8 awareness
  - `MyersSliceDelta` - Myers algorithm for huge slice diffing.
- **Compact / bitpacked serialization** — an opt-in binary format (behind a feature flag) that encodes deltas far more densely than `serde` + any text format can. Candidates include bitpacking index and length fields in `SliceDelta` entries and using varint encoding for arithmetic deltas, with the whole thing potentially exposed as a `deku`-backed implementation.
- **Reversible delta trait** — a `ReversibleDelta` trait (or an `invert()` method) so types can cheaply produce their own inverse without needing a `StateCachedTimeline` to reconstruct two states and call `diff`.
- **`#[derive(WrappingArithmetic)]`** — remove the boilerplate of implementing the trait by hand for newtype wrappers and simple structs.
- **Async / streaming timeline** — a `StreamingTimeline` adapter that yields commits as a `Stream`, targeting networked applications that push state changes in real time.
- **`no_std` support** — remove the hard dependency on `std` so the core traits and delta types can be used in embedded and WASM environments.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
