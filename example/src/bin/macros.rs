use deltapatcher::{Differentiable, Delta, delta::{ArithmeticDelta, WrappingArithmetic}};

// ── Struct example ────────────────────────────────────────────────────────────

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

// ── Enum example ──────────────────────────────────────────────────────────────

/// A simple shape enum demonstrating all three variant kinds:
/// named fields, unnamed (tuple) fields, and unit variants.
///
/// The derive macro generates a `ShapeDelta` enum with a matching variant for
/// each source variant (carrying per-field deltas) and an extra `Replace`
/// variant for transitions between different variants.
#[derive(Differentiable, Debug, Clone, PartialEq)]
#[deltapatcher(delta_derive(Debug))]
#[allow(dead_code)]
enum Shape {
    /// Rectangle with named width/height fields.
    Rect {
        #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
        width: i32,
        #[deltapatcher(delta_ty = ArithmeticDelta<i32>)]
        height: i32,
    },
    /// Circle with an unnamed radius field.
    Circle(
        #[deltapatcher(delta_ty = ArithmeticDelta<u32>)]
        u32,
    ),
    /// A dimensionless point — no fields.
    Point,
}

fn main() {
    // ── Struct demo ───────────────────────────────────────────────────────────
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

    // ── Enum demo — same variant ──────────────────────────────────────────────
    println!("\n── Enum demo ────");

    let rect_a = Shape::Rect { width: 10, height: 20 };
    let rect_b = Shape::Rect { width: 30, height: 5  };

    // Both are Rect: the generated delta is ShapeDelta::Rect { width: ..., height: ... }
    let same_variant_delta = rect_b.diff(&rect_a);
    dbg!(&same_variant_delta);

    let mut s = rect_a.clone();
    s.patch(&same_variant_delta);
    assert_eq!(s, rect_b);
    println!("Same-variant patch: {s:?} == {rect_b:?}");

    // ── Enum demo — cross-variant (Replace) ───────────────────────────────────
    let circle = Shape::Circle(99);

    // Rect → Circle: variants differ, so the delta is ShapeDelta::Replace(Circle(99))
    let replace_delta = circle.diff(&rect_b);
    dbg!(&replace_delta);

    let mut s = rect_b.clone();
    s.patch(&replace_delta);
    assert_eq!(s, circle);
    println!("Cross-variant patch: {s:?} == {circle:?}");

    // ── Enum demo — aggregate ─────────────────────────────────────────────────
    // Two same-variant deltas collapse into one.
    let rect_c = Shape::Rect { width: 3, height: 42 };

    let mut d_ab = rect_b.diff(&rect_a);
    let     d_bc = rect_c.diff(&rect_b);
    d_ab.aggregate(&d_bc);

    let mut s = rect_a.clone();
    s.patch(&d_ab);
    assert_eq!(s, rect_c);
    println!("Aggregated same-variant patch: {s:?} == {rect_c:?}");

    // A Replace followed by a same-variant delta collapses into a single Replace
    // that already encodes the fully-updated final state.
    let mut d_replace_then_same = circle.diff(&rect_c); // Replace(Circle(99))
    let circle_b = Shape::Circle(7);
    let d_circle = circle_b.diff(&circle);              // Circle(delta)
    d_replace_then_same.aggregate(&d_circle);

    let mut s = rect_c.clone();
    s.patch(&d_replace_then_same);
    assert_eq!(s, circle_b);
    println!("Aggregated replace+same patch: {s:?} == {circle_b:?}");

    println!("\nRun `cargo expand --bin macros --features macros` to see the generated code.");
}