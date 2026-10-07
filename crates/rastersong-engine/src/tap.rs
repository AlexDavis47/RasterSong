//! Taps: a read-only look at what one connection carries at one frame.
//!
//! A tap is answered by a renderer of its own that the service keeps beside the one filling the
//! cache, so asking never moves the render-ahead and never touches the cache. It is never part
//! of the cache key and never changes what is rendered.

use std::sync::Arc;

use rastersong_graph::dsp::Stretcher;
use rastersong_graph::{Interpolation, Kind, Layout, Range, Signal};

/// The longest side of the picture a tap makes.
pub const PICTURE_SIDE: u32 = 320;
/// Samples above which a signal that isn't audio is not kept as samples, only as a picture.
const MAX_KEPT_SAMPLES: usize = 1 << 18;

/// Which connection to look at, and when: the output `output` of node `node` while rendering
/// output frame `frame`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TapRequest {
    pub frame: usize,
    pub node: String,
    pub output: usize,
}

/// A signal drawn as a picture, packed RGB8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// What a tap saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Tap {
    pub request: TapRequest,
    /// The layout (and so the tag) of the signal.
    pub layout: Layout,
    /// The signal stretched over a picture, whatever it is.
    pub picture: Option<Picture>,
    /// Samples a second per channel the signal runs at: its frame's length times the frame rate.
    pub rate: f64,
    /// The signal's samples, for audio and for any other signal small enough to keep.
    pub samples: Option<Arc<[f32]>>,
}

/// The answer to a tap request.
#[derive(Debug, Clone, PartialEq)]
pub enum TapOutcome {
    /// Asked for; the render thread hasn't answered yet.
    Pending,
    Ready(Arc<Tap>),
    /// The connection isn't part of what is rendered: its node doesn't feed the output (or the
    /// graph can't render at all).
    NotRendered,
    /// The frame could not be rendered.
    Failed(String),
}

/// The size of a picture of a project of `width` by `height`.
pub fn picture_size(width: u32, height: u32) -> (u32, u32) {
    let scale = (PICTURE_SIDE as f32 / width.max(height).max(1) as f32).min(1.0);
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

/// Draws `signal` stretched over a picture of the project's shape, the way Video Output
/// does: an RGB signal keeps its channels, anything else is read as a flat run of samples and
/// shown as gray. Signals in `-1..=1` are shifted into `0..=1` so that negative values show.
fn picture(signal: &Signal, project: (u32, u32)) -> Picture {
    let (width, height) = picture_size(project.0, project.1);
    let mut floats = vec![0.0; (width * height * 3) as usize];
    Stretcher::default().stretch(
        &signal.data,
        signal.layout.samples_per_pixel as usize,
        &mut floats,
        3,
        Interpolation::Hold,
    );
    let bipolar = signal.layout.tag.range == Range::Bipolar;
    let rgb = floats
        .iter()
        .map(|&x| {
            let x = if bipolar { x * 0.5 + 0.5 } else { x };
            if x.is_finite() {
                (x.clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                0
            }
        })
        .collect();
    Picture { width, height, rgb }
}

/// What a tap of `signal` shows. `project` is the size of the rendered picture.
pub fn read(request: TapRequest, signal: &Signal, project: (u32, u32), frame_rate: f64) -> Tap {
    let audio = signal.layout.tag.kind == Kind::Audio;
    Tap {
        request,
        layout: signal.layout,
        rate: signal.layout.len() as f64 / f64::from(signal.layout.samples_per_pixel.max(1))
            * frame_rate,
        picture: Some(picture(signal, project)),
        samples: (audio || signal.data.len() <= MAX_KEPT_SAMPLES)
            .then(|| signal.data.clone().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> TapRequest {
        TapRequest {
            frame: 0,
            node: "n".into(),
            output: 0,
        }
    }

    #[test]
    fn pictures_fit_the_project_shape_within_the_side_limit() {
        assert_eq!(picture_size(1920, 1080), (320, 180));
        assert_eq!(picture_size(100, 50), (100, 50));
        assert_eq!(picture_size(1080, 1920), (180, 320));
    }

    #[test]
    fn an_audio_signal_is_kept_as_samples_and_a_mono_picture_is_gray() {
        let audio = Signal::from_data(Layout::audio(4), vec![0.1, 0.2, 0.3, 0.4]);
        let tap = read(request(), &audio, (4, 2), 30.0);
        assert!(tap.picture.is_some(), "audio can be seen as a picture too");
        assert_eq!(tap.samples.as_deref(), Some(&[0.1, 0.2, 0.3, 0.4][..]));

        let mono = Signal::from_data(Layout::mono(2, 1), vec![1.0, 0.0]);
        let tap = read(request(), &mono, (2, 1), 30.0);
        let picture = tap.picture.unwrap();
        assert_eq!(picture.rgb, [255, 255, 255, 0, 0, 0]);
    }
}
