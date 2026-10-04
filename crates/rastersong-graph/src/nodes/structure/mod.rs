//! Nodes that take signals apart and rebuild them. They change a signal's shape and leave its
//! values alone, and they are the only nodes that care about layout.

mod combine;
mod interleave;
mod pack;
mod split;

pub use combine::Combine;
pub use interleave::Interleave;
pub use pack::Pack;
pub use split::Split;

use crate::Layout;

fn expect_rgb(layout: Layout) -> Result<(), String> {
    if layout.samples_per_pixel == 3 {
        Ok(())
    } else {
        Err(format!("expects an RGB signal, got {layout}"))
    }
}
