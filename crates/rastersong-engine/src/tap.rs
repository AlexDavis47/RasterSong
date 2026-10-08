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
///
/// The stretch happens at the project's size, as Video Output's does, and the result is then
/// scaled down to the picture in two dimensions. (Stretching straight to the smaller picture
/// would read the rows of a picture as one run and lay them side by side.)
fn picture(signal: &Signal, project: (u32, u32)) -> Picture {
    let (width, height) = picture_size(project.0, project.1);
    let (full_width, full_height) = (project.0.max(1), project.1.max(1));
    let mut full = vec![0.0; (full_width * full_height * 3) as usize];
    Stretcher::default().stretch(
        &signal.data,
        signal.layout.samples_per_pixel as usize,
        &mut full,
        3,
        Interpolation::Hold,
    );
    let floats = shrink(&full, (full_width, full_height), (width, height));
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

/// Scales an RGB picture of size `from` down to `to` (no larger on either side), each pixel the
/// average of the pixels it covers.
fn shrink(rgb: &[f32], from: (u32, u32), to: (u32, u32)) -> Vec<f32> {
    if from == to {
        return rgb.to_vec();
    }
    let (fw, fh) = (from.0 as usize, from.1 as usize);
    let (tw, th) = (to.0 as usize, to.1 as usize);
    let span = |i: usize, to: usize, from: usize| {
        (i * from / to)..((i + 1) * from / to).max(i * from / to + 1)
    };
    let mut out = vec![0.0; tw * th * 3];
    for y in 0..th {
        let rows = span(y, th, fh);
        for x in 0..tw {
            let columns = span(x, tw, fw);
            let mut sum = [0.0f32; 3];
            let mut count = 0.0;
            for sy in rows.clone() {
                for sx in columns.clone() {
                    let at = (sy * fw + sx) * 3;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += rgb[at + c];
                    }
                    count += 1.0;
                }
            }
            let at = (y * tw + x) * 3;
            for (c, total) in sum.iter().enumerate() {
                out[at + c] = total / count;
            }
        }
    }
    out
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

    #[test]
    fn a_picture_larger_than_the_tap_is_scaled_down_in_two_dimensions() {
        // A horizontal ramp, bright on the right, with the bottom half dark.
        let (w, h) = (1280, 720);
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = if y < h / 2 { x as f32 / w as f32 } else { 0.0 };
                data.extend([v, v, v]);
            }
        }
        let signal = Signal::from_data(Layout::new(w, h, 3), data);
        let picture = read(request(), &signal, (w, h), 30.0).picture.unwrap();
        assert_eq!((picture.width, picture.height), (320, 180));
        let red = |x: u32, y: u32| picture.rgb[((y * picture.width + x) * 3) as usize];
        for y in [0, 40, 89] {
            assert!(red(0, y) < 3 && red(160, y).abs_diff(128) < 3 && red(319, y) > 250);
        }
        for y in [90, 179] {
            assert_eq!(red(200, y), 0);
        }
    }
}
