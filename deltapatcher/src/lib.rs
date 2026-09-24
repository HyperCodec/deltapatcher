pub mod delta;
pub mod timeline;
pub mod util;

pub use delta::{Delta, Differentiable};

#[cfg(feature = "macros")]
pub use deltapatcher_macros as macros;