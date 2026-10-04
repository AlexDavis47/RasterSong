/// Describes how a signal's samples map onto an image. Effect nodes ignore it; structural nodes
/// and unit conversion (e.g. "one row") use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// 1 for a single channel, 3 for interleaved RGB.
    pub samples_per_pixel: u32,
}

impl Layout {
    pub const EMPTY: Self = Self::new(0, 0, 0);

    pub const fn new(width: u32, height: u32, samples_per_pixel: u32) -> Self {
        Self {
            width,
            height,
            samples_per_pixel,
        }
    }

    /// A single-channel image.
    pub const fn mono(width: u32, height: u32) -> Self {
        Self::new(width, height, 1)
    }

    /// An image with interleaved R, G, B samples.
    pub const fn rgb(width: u32, height: u32) -> Self {
        Self::new(width, height, 3)
    }

    /// One frame's worth of mono audio: a single row of `samples`.
    pub const fn audio(samples: u32) -> Self {
        Self::new(samples, 1, 1)
    }

    /// Samples in one frame-sized block.
    pub const fn len(&self) -> usize {
        self.width as usize * self.height as usize * self.samples_per_pixel as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Samples in one row; the unit behind "rows" and "cycles per row" parameters.
    pub const fn samples_per_row(&self) -> usize {
        self.width as usize * self.samples_per_pixel as usize
    }
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.samples_per_pixel {
            1 => write!(f, "{}×{} mono", self.width, self.height),
            3 => write!(f, "{}×{} RGB", self.width, self.height),
            n => write!(f, "{}×{}×{}", self.width, self.height, n),
        }
    }
}

/// One frame of a signal: a block of samples plus its layout.
///
/// Video signals are nominally in `0.0..=1.0` (black to full intensity); audio signals in
/// `-1.0..=1.0`. Nothing clamps values between nodes, only the output does.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Signal {
    pub data: Vec<f32>,
    pub layout: Layout,
}

impl Signal {
    pub const EMPTY: Self = Self {
        data: Vec::new(),
        layout: Layout::EMPTY,
    };

    pub fn zeros(layout: Layout) -> Self {
        Self {
            data: vec![0.0; layout.len()],
            layout,
        }
    }

    pub fn from_data(layout: Layout, data: Vec<f32>) -> Self {
        assert_eq!(
            data.len(),
            layout.len(),
            "data does not match layout {layout}"
        );
        Self { data, layout }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
