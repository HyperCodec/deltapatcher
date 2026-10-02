use deltapatcher::{Differentiable, delta::ArithmeticDelta, timeline::{Commit, StateCachedTimeline, Timeline}};

fn main() {
    // --- Serialize and deserialize a SliceDelta ---
    let initial = vec![1u32, 2, 3, 4, 5];
    let updated = vec![1u32, 9, 3, 10, 5];
    let slice_delta = updated.differentiate(&initial);

    let serialized = serde_json::to_string_pretty(&slice_delta).unwrap();
    println!("=== SliceDelta (JSON) ===");
    println!("{serialized}\n");

    // Deserialize back and verify it produces the same patch result
    let deserialized: deltapatcher::delta::SliceDelta<u32> =
        serde_json::from_str(&serialized).unwrap();

    let mut patched = initial.clone();
    patched.patch(&deserialized);
    assert_eq!(patched, updated);
    println!("Deserialized delta applied correctly: {patched:?}\n");

    // --- Serialize and deserialize an ArithmeticDelta ---
    let a: i32 = 42;
    let b: i32 = -17;
    let arith_delta: ArithmeticDelta<i32> = b.differentiate(&a);

    let serialized = serde_json::to_string(&arith_delta).unwrap();
    println!("=== ArithmeticDelta (JSON) ===");
    println!("{serialized}\n");

    let deserialized: ArithmeticDelta<i32> = serde_json::from_str(&serialized).unwrap();
    let mut patched = a;
    patched.patch(&deserialized);
    assert_eq!(patched, b);
    println!("Deserialized ArithmeticDelta applied correctly: {patched}\n");

    // --- Serialize and deserialize a Timeline ---
    let states = vec![0i32, 10, 35, 70];
    let timeline: Timeline<ArithmeticDelta<i32>> = states
        .windows(2)
        .map(|w| {
            let delta: ArithmeticDelta<i32> = w[1].differentiate(&w[0]);
            Commit::new(delta, ())
        })
        .collect();

    let serialized = serde_json::to_string_pretty(&timeline).unwrap();
    println!("=== Timeline (JSON) ===");
    println!("{serialized}\n");

    let deserialized: Timeline<ArithmeticDelta<i32>> = serde_json::from_str(&serialized).unwrap();
    let rebuilt: i32 = deserialized.build_state_from_default(..);
    assert_eq!(rebuilt, *states.last().unwrap());
    println!("Deserialized Timeline rebuilt state correctly: {rebuilt}\n");

    // --- Serialize and deserialize a StateCachedTimeline ---
    let mut cached = StateCachedTimeline::new(2, 0i32);
    for delta in [5, 15, 25, 35].map(ArithmeticDelta) {
        cached.push(Commit::new(delta, ()));
    }

    let serialized = serde_json::to_string_pretty(&cached).unwrap();
    println!("=== StateCachedTimeline (JSON) ===");
    println!("{serialized}\n");

    let deserialized: StateCachedTimeline<i32, ArithmeticDelta<i32>> =
        serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.current_state(), cached.current_state());
    println!(
        "Deserialized StateCachedTimeline current state: {}\n",
        deserialized.current_state()
    );
}