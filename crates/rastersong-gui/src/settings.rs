//! App settings that belong to the user rather than to a project, kept between sessions.

use rastersong_engine::PreviewScale;
use serde::{Deserialize, Serialize};

use crate::theme::{ThemeChoice, WireStyle};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub wire_style: WireStyle,
    /// Show each node's latency and warmup under it in the graph editor.
    pub node_stats: bool,
    /// Preview resolution as a divisor of full resolution (1 = full, 2 = half, …).
    pub preview_divisor: u32,
    /// Playback volume, `0..=1`.
    pub volume: f32,
    /// Click on every beat during playback, to check the tempo by ear.
    pub metronome: bool,
    /// Duplicate and Paste connect the new nodes to the sources of the originals.
    pub keep_connections: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            wire_style: WireStyle::default(),
            node_stats: false,
            preview_divisor: 2,
            volume: 0.8,
            metronome: false,
            keep_connections: true,
        }
    }
}

impl Settings {
    /// The key settings are stored under in eframe's storage.
    pub const STORAGE_KEY: &str = "rastersong-settings";

    pub fn preview_scale(&self) -> PreviewScale {
        PreviewScale::from_divisor(self.preview_divisor).unwrap_or(PreviewScale::Half)
    }

    pub fn set_preview_scale(&mut self, scale: PreviewScale) {
        self.preview_divisor = scale.divisor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_bad_values_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "preview_divisor": 3 }"#).unwrap();
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.preview_scale(), PreviewScale::Half);
        let settings: Settings = serde_json::from_str(r#"{ "theme": "Light" }"#).unwrap();
        assert_eq!(settings.theme, ThemeChoice::Light);
    }
}
