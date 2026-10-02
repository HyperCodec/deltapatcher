use deltapatcher::{Differentiable, delta::{ArithmeticDelta, WrappingArithmetic}};

#[derive(Differentiable, Debug, Clone, Copy)]
// This top-level attribute macro is completely optional.
#[deltapatcher(
    // Override the name of the generated delta type.
    // Otherwise, this would default to IVec2Delta
    delta_name = VecDelta,

    // We can also specify things to derive on this
    // generated delta type.
    delta_derive(Debug, Clone),
)]
struct IVec2 {
    // These field-level attributes are required
    #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
    x: i32,

    #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
    y: i32,
}

// note that we can also just implement WrappingArithmetic on IVec2 and use ArithmeticDelta<IVec2> for differentiation.
// this is because any type with WrappingArithmetic also implements Differentiable<ArithmeticDelta<Self>>.
// there is currently no WrappingArithmetic derive macro.
impl WrappingArithmetic for IVec2 {
    fn wrapping_add(self, rhs: Self) -> Self {
        Self {
            x: self.x.wrapping_add(rhs.x),
            y: self.y.wrapping_add(rhs.y),
        }
    }

    fn wrapping_sub(self, rhs: Self) -> Self {
        Self {
            x: self.x.wrapping_sub(rhs.x),
            y: self.y.wrapping_sub(rhs.y)
        }
    }
}

fn main() {
    let v1 = IVec2 { x: 4, y: -5 };
    let v2 = IVec2 { x: -124, y: 46 };

    // use our generated delta type
    let delta1: VecDelta = v2.diff(&v1);

    // use the delta type available through WrappingArithmetic
    let delta2: ArithmeticDelta<IVec2> = v2.diff(&v1);

    dbg!(&delta1, &delta2);

    // we can patch multiple different types of deltas onto the same state.
    let mut state = IVec2 { x: 10, y: 4 };
    state.patch(&delta1);
    dbg!(&state);
    state.patch(&delta2);
    dbg!(&state);

    println!("Run `cargo expand --bin macros --features macros` to see the generated code.");
}