//! The one name for a channel count ("Stereo", "5.1", "3 channels"), used wherever channels are shown.

use rastersong_lang::{tr, tr_args};

/// A channel count in words.
pub fn channels_label(channels: u32) -> String {
    match channels {
        1 => tr("channels.mono").to_owned(),
        2 => tr("channels.stereo").to_owned(),
        6 => tr("channels.surround_5_1").to_owned(),
        8 => tr("channels.surround_7_1").to_owned(),
        n => tr_args("channels.n", &[("count", &n.to_string())]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_common_layouts_and_counts_the_rest() {
        assert_eq!(channels_label(1), "Mono");
        assert_eq!(channels_label(2), "Stereo");
        assert_eq!(channels_label(6), "5.1");
        assert_eq!(channels_label(3), "3 channels");
    }
}
