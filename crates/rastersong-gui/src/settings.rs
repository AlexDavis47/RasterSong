//! App settings that belong to the user rather than to a project, kept between sessions.

use rastersong_engine::PreviewScale;
use serde::{Deserialize, Serialize};

use crate::theme::{ThemeChoice, WireStyle};

/// Where other languages are looked for: a `lang` folder next to the program.
pub fn locale_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("lang")))
        .unwrap_or_else(|| std::path::PathBuf::from("lang"))
}

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
    /// The language of the interface: a folder of `.lang` files in the program's `lang` folder, or
    /// `en` for the text built in.
    pub language: String,
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
            language: rastersong_lang::ENGLISH_CODE.to_owned(),
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

    /// Switches the interface to `language`, falling back to English when it can't be loaded.
    pub fn apply_language(&mut self) {
        if rastersong_lang::set_locale(&self.language, &locale_dir()).is_err() {
            self.language = rastersong_lang::ENGLISH_CODE.to_owned();
            let _ = rastersong_lang::set_locale(rastersong_lang::ENGLISH_CODE, &locale_dir());
        }
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
