//! Reusable widgets, built once and used wherever they apply. Wherever they can they take plain
//! data (samples, pixels, levels), so any part of the interface can use them.

mod icons;
mod meter;
mod picture;
mod scope;
mod spectrum;

pub use icons::{Icon, icon_list};
pub use meter::{Scale, gain_reduction_meter, level_meter, meter_label, signal_meter};
pub use picture::picture;
pub use scope::scope;
pub use spectrum::spectrum;
