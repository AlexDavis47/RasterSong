//! Reusable widgets, built once and used wherever they apply. Wherever they can they take plain
//! data (samples, pixels, levels), so any part of the interface can use them.

mod card;
mod channels;
mod drag_bubble;
mod drum;
mod icons;
mod kbd;
mod meter;
mod picture;
mod scope;
mod segmented;
mod spectrum;

pub use card::{
    CARD_GAP, CARD_HEADER_HEIGHT, HALF_GAP, card, card_body, card_header, card_panel, card_space,
    card_title,
};
pub use channels::channels_label;
pub use drag_bubble::{Phase, bloop, bubble_phase, drag_bubble, drop_scale, paint_bubble};
pub use drum::{DRUM_HEIGHT, DRUM_WIDTH, DrumResponse, drum, scroll_steps, wheel_events};
pub use icons::{Icon, IconButton, icon_list, paint as paint_icon};
pub use kbd::{kbd, kbd_text, key_button, paint_kbd};
pub use meter::{Scale, gain_reduction_meter, level_meter, meter_label, signal_meter};
pub use picture::picture;
pub use scope::scope;
pub use segmented::segmented;
pub use spectrum::spectrum;
