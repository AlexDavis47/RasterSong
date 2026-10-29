//! Reusable widgets, built once and used wherever they apply. Wherever they can they take plain
//! data (samples, pixels, levels), so any part of the interface can use them.

mod beat_lights;
mod card;
mod channels;
mod choice_card;
mod drag_bubble;
mod drum;
mod icons;
mod inspector;
mod kbd;
mod meter;
mod picture;
mod scope;
mod segmented;
mod spectrum;

pub use beat_lights::beat_lights;
pub use card::{
    CARD_GAP, CARD_HEADER_HEIGHT, HALF_GAP, card, card_body, card_header, card_panel, card_space,
    card_title,
};
pub use channels::channels_label;
pub use choice_card::choice_card;
pub use drag_bubble::{Phase, bloop, bubble_phase, drag_bubble, drop_scale, paint_bubble};
pub use drum::{DRUM_HEIGHT, DRUM_WIDTH, DrumResponse, drum, scroll_steps, wheel_events};
pub use icons::{Icon, IconButton, icon_list, paint as paint_icon};
pub use inspector::{
    CHOICE_WIDTH, about, inspector, inspector_body, inspector_header, kind_row, name_style, pill,
    rule, section, section_label, setting_row, setting_toggle, swatch, toggle_box,
};
pub use kbd::{KeyButton, kbd, kbd_text, paint_kbd};
pub use meter::{Scale, gain_reduction_meter, level_meter, meter_label, signal_meter};
pub use picture::picture;
pub use scope::scope;
pub use segmented::segmented;
pub use spectrum::spectrum;
