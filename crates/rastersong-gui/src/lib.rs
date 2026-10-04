//! The RasterSong desktop application. It holds UI state only: decoding, rendering and caching
//! all happen in the engine.

mod app;
pub mod audio_out;
pub mod editor;
mod timeline;

pub use app::{App, STARTER_GRAPH};
pub use audio_out::AudioOut;
pub use editor::{GraphEditor, without_layout};
