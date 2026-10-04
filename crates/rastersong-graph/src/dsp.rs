//! Building blocks shared by the runtime and the nodes.

use crate::Interpolation;

/// Resamples one frame block `src` to `dst.len()` samples.
///
/// `group` keeps runs of output samples together: with `group = 3`, each RGB pixel of the output
/// maps to a single source position, so a mono signal stretched over RGB affects a pixel's R, G
/// and B equally. Positions are centre-aligned, so a block maps onto exactly the same span of
/// time or space at any length. Linear interpolation clamps at the block edges.
pub fn resample(src: &[f32], dst: &mut [f32], group: usize, mode: Interpolation) {
    if src.len() == dst.len() || src.is_empty() {
        if src.len() == dst.len() {
            dst.copy_from_slice(src);
        } else {
            dst.fill(0.0);
        }
        return;
    }
    let groups = dst.len() / group;
    let ratio = src.len() as f64 / groups as f64;
    let last = src.len() - 1;
    for (g, chunk) in dst.chunks_mut(group).enumerate() {
        let pos = (g as f64 + 0.5) * ratio - 0.5;
        let value = match mode {
            Interpolation::Hold => src[(((g as f64 + 0.5) * ratio) as usize).min(last)],
            Interpolation::Linear => {
                let pos = pos.clamp(0.0, last as f64);
                let i = pos as usize;
                let frac = (pos - i as f64) as f32;
                let next = src[(i + 1).min(last)];
                src[i] + (next - src[i]) * frac
            }
        };
        chunk.fill(value);
    }
}

/// A ring buffer delay line with fractional (linearly interpolated) reads.
#[derive(Debug, Clone, Default)]
pub struct DelayLine {
    buffer: Vec<f32>,
    /// Index the next sample will be written to.
    write: usize,
}

impl DelayLine {
    /// A delay line that can read up to `max_delay` samples into the past.
    pub fn new(max_delay: usize) -> Self {
        Self {
            buffer: vec![0.0; max_delay + 2],
            write: 0,
        }
    }

    pub fn max_delay(&self) -> usize {
        self.buffer.len().saturating_sub(2)
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
    }

    /// The sample pushed `delay` samples ago, where 0 is the most recent. Fractional delays
    /// interpolate linearly; delays beyond [`Self::max_delay`] are clamped.
    pub fn read(&self, delay: f64) -> f32 {
        let delay = delay.clamp(0.0, self.max_delay() as f64);
        let whole = delay as usize;
        let frac = (delay - whole as f64) as f32;
        let len = self.buffer.len();
        let newer = self.buffer[(self.write + len - 1 - whole) % len];
        if frac == 0.0 {
            return newer;
        }
        let older = self.buffer[(self.write + len - 2 - whole) % len];
        newer + (older - newer) * frac
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_repeats_samples() {
        let mut dst = [0.0; 6];
        resample(&[1.0, 2.0, 3.0], &mut dst, 1, Interpolation::Hold);
        assert_eq!(dst, [1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
    }

    #[test]
    fn hold_keeps_pixels_together() {
        // Two mono samples over two RGB pixels: each pixel's R, G, B get the same value.
        let mut dst = [0.0; 6];
        resample(&[1.0, 2.0], &mut dst, 3, Interpolation::Hold);
        assert_eq!(dst, [1.0, 1.0, 1.0, 2.0, 2.0, 2.0]);
    }

    #[test]
    fn linear_ramps_and_clamps() {
        let mut dst = [0.0; 4];
        resample(&[0.0, 1.0], &mut dst, 1, Interpolation::Linear);
        assert_eq!(dst, [0.0, 0.25, 0.75, 1.0]);
    }

    #[test]
    fn downsampling_picks_centres() {
        let mut dst = [0.0; 2];
        resample(&[1.0, 2.0, 3.0, 4.0], &mut dst, 1, Interpolation::Hold);
        assert_eq!(dst, [2.0, 4.0]);
    }

    #[test]
    fn same_length_copies_and_empty_source_is_silence() {
        let mut dst = [9.0; 2];
        resample(&[1.0, 2.0], &mut dst, 1, Interpolation::Linear);
        assert_eq!(dst, [1.0, 2.0]);
        resample(&[], &mut dst, 1, Interpolation::Hold);
        assert_eq!(dst, [0.0, 0.0]);
    }

    #[test]
    fn delay_line_reads_whole_and_fractional_delays() {
        let mut line = DelayLine::new(4);
        for x in [1.0, 2.0, 3.0, 4.0] {
            line.push(x);
        }
        assert_eq!(line.read(0.0), 4.0);
        assert_eq!(line.read(3.0), 1.0);
        assert_eq!(line.read(0.5), 3.5);
        assert_eq!(line.read(10.0), line.read(4.0), "clamped to max delay");
        line.reset();
        assert_eq!(line.read(1.0), 0.0);
    }
}
