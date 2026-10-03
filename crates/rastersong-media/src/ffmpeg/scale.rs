use std::ptr;

use ffmpeg::ffi;
use ffmpeg::format::Pixel;
use ffmpeg::frame;
use ffmpeg_next as ffmpeg;

use crate::MediaError;

/// Converts decoded frames to packed RGB24 at a requested size.
///
/// Uses the dynamic `sws_scale_frame` API, which reads the pixel format, size, color matrix and
/// range from each source frame and reconfigures itself when they change. One scaler is created
/// per video source and reused for every frame.
pub(crate) struct Scaler {
    ctx: *mut ffi::SwsContext,
    dst: frame::Video,
}

// The context is owned exclusively by this struct and only used through `&mut self`.
unsafe impl Send for Scaler {}

impl Scaler {
    pub fn new() -> Result<Self, MediaError> {
        // SAFETY: plain allocation; null is handled.
        let ctx = unsafe { ffi::sws_alloc_context() };
        if ctx.is_null() {
            return Err(MediaError::Decode("could not allocate a scaler".into()));
        }
        // SAFETY: ctx is a valid, freshly allocated context. These are public option fields.
        unsafe {
            (*ctx).flags = ffi::SwsFlags::SWS_BICUBIC as u32
                | ffi::SwsFlags::SWS_ACCURATE_RND as u32
                | ffi::SwsFlags::SWS_FULL_CHR_H_INT as u32;
            (*ctx).threads = 0; // automatic
        }
        Ok(Self {
            ctx,
            dst: frame::Video::empty(),
        })
    }

    /// Converts `src` to `width` × `height` RGB24 with tightly packed rows.
    pub fn convert(
        &mut self,
        src: &frame::Video,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, MediaError> {
        if self.dst.width() != width || self.dst.height() != height {
            self.dst = frame::Video::new(Pixel::RGB24, width, height);
        }
        // SAFETY: both frames are valid; dst has allocated buffers matching its format and size.
        let ret = unsafe { ffi::sws_scale_frame(self.ctx, self.dst.as_mut_ptr(), src.as_ptr()) };
        if ret < 0 {
            return Err(MediaError::Decode(format!(
                "pixel format conversion failed: {}",
                ffmpeg::Error::from(ret)
            )));
        }

        let row = width as usize * 3;
        let stride = self.dst.stride(0);
        let data = self.dst.data(0);
        let mut out = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            out.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        Ok(out)
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: ctx was allocated by sws_alloc_context and is freed exactly once.
        unsafe { ffi::sws_free_context(&mut self.ctx) };
        self.ctx = ptr::null_mut();
    }
}
