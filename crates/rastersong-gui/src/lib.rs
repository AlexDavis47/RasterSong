//! The RasterSong desktop application. It holds UI state only: decoding, rendering and caching
//! all happen in the engine.

mod app;
pub mod audio_out;
pub mod editor;
pub mod effects;
pub mod history;
pub mod name_edit;
pub mod preview;
pub mod settings;
pub mod theme;
pub mod timeline;
pub mod track_ops;

pub use app::{App, STARTER_GRAPH};
pub use audio_out::AudioOut;
pub use editor::{GraphEditor, without_layout};
pub use settings::Settings;
pub use theme::{Theme, ThemeChoice};
