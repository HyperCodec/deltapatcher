use deltapatcher::{Delta, Differentiable};
use rand::RngExt;

fn main() {
    let start = vec!["a", "b", "c", "d", "e", "f"];
    let end = vec!["a", "c", "d", "f"];

    // we removed the items at index 1 and 4
    let delta = end.differentiate(&start);
    dbg!(&delta);

    // remove items at index 1 and 4 from a different vec
    let mut alt_start = vec!["1", "2", "3", "4", "5", "6"];
    alt_start.patch(&delta);

    dbg!(&alt_start);

    let mut rng = rand::rng();
    let rand_a = (0..rng.random_range(10..=50)).map(|_| rng.random_range(0..10)).collect::<Vec<i32>>();
    let rand_b = (0..rng.random_range(10..=50)).map(|_| rng.random_range(0..10)).collect::<Vec<i32>>();

    let a_to_b = rand_b.differentiate(&rand_a);
    dbg!(&rand_a, &rand_b, &a_to_b);

    let rand_c = (0..rng.random_range(10..=50)).map(|_| rng.random_range(0..10)).collect::<Vec<i32>>();
    let b_to_c = rand_c.differentiate(&rand_b);
    dbg!(&rand_c, &b_to_c);

    // aggregate(A -> B, B -> C) = A -> C.
    let mut a_to_c = a_to_b.clone();
    a_to_c.aggregate_owned(b_to_c);

    dbg!(&a_to_c);

    // we can now reproduce c without knowing the state of b or the deltas leading up to it.
    // this is particularly useful in things like media streaming or games
    // because we can avoid transmitting a full state or multiple changes in states,
    // saving latency and bandwidth
    let mut patched_rand_c = rand_a.clone();
    patched_rand_c.patch(&a_to_c);
    assert_eq!(rand_c, patched_rand_c);
}