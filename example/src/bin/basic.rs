use deltapatcher::Differentiable;

fn main() {
    let start = vec!["a", "b", "c", "d"];
    let end = vec!["c", "d"];

    let delta = end.differentiate(&start);

    dbg!(&delta);

    let mut alt_state = vec!["1", "2", "3", "4"];
    alt_state.patch(&delta);

    dbg!(&alt_state);
}