use crate::Rotation;

/// Rotates a packed RGB24 image clockwise. Returns the rotated pixels; the caller swaps
/// width and height for quarter turns.
pub(crate) fn rotate_rgb24(src: &[u8], width: usize, height: usize, rotation: Rotation) -> Vec<u8> {
    debug_assert_eq!(src.len(), width * height * 3);
    if rotation == Rotation::None {
        return src.to_vec();
    }
    let mut dst = vec![0; src.len()];
    for y in 0..height {
        for x in 0..width {
            let (dx, dy, dst_width) = match rotation {
                Rotation::None => unreachable!(),
                Rotation::Cw90 => (height - 1 - y, x, height),
                Rotation::Cw180 => (width - 1 - x, height - 1 - y, width),
                Rotation::Cw270 => (y, width - 1 - x, height),
            };
            let s = (y * width + x) * 3;
            let d = (dy * dst_width + dx) * 3;
            dst[d..d + 3].copy_from_slice(&src[s..s + 3]);
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2×1 image: red, green.
    const RG: [u8; 6] = [255, 0, 0, 0, 255, 0];

    #[test]
    fn quarter_turns() {
        // Clockwise: red ends up on top, green below.
        assert_eq!(rotate_rgb24(&RG, 2, 1, Rotation::Cw90), RG);
        // Counter-clockwise: green on top.
        assert_eq!(
            rotate_rgb24(&RG, 2, 1, Rotation::Cw270),
            [0, 255, 0, 255, 0, 0]
        );
        assert_eq!(
            rotate_rgb24(&RG, 2, 1, Rotation::Cw180),
            [0, 255, 0, 255, 0, 0]
        );
        assert_eq!(rotate_rgb24(&RG, 2, 1, Rotation::None), RG);
    }

    #[test]
    fn four_quarter_turns_are_identity() {
        let (w, h) = (5, 3);
        let src: Vec<u8> = (0..w * h * 3).map(|i| i as u8).collect();
        let mut img = src.clone();
        let (mut cw, mut ch) = (w, h);
        for _ in 0..4 {
            img = rotate_rgb24(&img, cw, ch, Rotation::Cw90);
            (cw, ch) = (ch, cw);
        }
        assert_eq!(img, src);
    }

    #[test]
    fn from_degrees_snaps_and_wraps() {
        assert_eq!(Rotation::from_degrees_cw(0.0), Rotation::None);
        assert_eq!(Rotation::from_degrees_cw(89.9), Rotation::Cw90);
        assert_eq!(Rotation::from_degrees_cw(-90.0), Rotation::Cw270);
        assert_eq!(Rotation::from_degrees_cw(180.0), Rotation::Cw180);
        assert_eq!(Rotation::from_degrees_cw(-180.0), Rotation::Cw180);
        assert_eq!(Rotation::from_degrees_cw(450.0), Rotation::Cw90);
    }
}
