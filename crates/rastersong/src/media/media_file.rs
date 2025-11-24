//! MediaFile - threaded media file API that communicates with worker thread

use anyhow::Result;
use crossbeam::channel::{Receiver, Sender};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::media_worker::MediaRequest;

use super::audio_samples::AudioSamples;
use super::frame_cache::FrameCache;
use super::frame_metadata::FrameMetadataCache;
use super::media_id::MediaId;
use super::video_frame::VideoFrame;

/// Receiver for async frame decode operations
///
/// This wraps the internal channel receiver to avoid exposing crossbeam types.
pub struct FrameReceiver(Receiver<Result<VideoFrame>>);

/// Media metadata cached at creation time
#[derive(Clone, Debug)]
pub struct MediaMetadata {
    /// Duration in seconds
    pub duration: f64,
    /// Video info (width, height, fps) if video stream exists
    pub video_info: Option<(u32, u32, f64)>,
    /// Audio info (sample_rate, channels) if audio stream exists
    pub audio_info: Option<(u32, u16)>,
}

/// MediaFile represents a threaded media file
///
/// All decode operations are non-blocking and communicate with a worker thread
/// that owns the FFmpeg resources. The worker processes requests sequentially.
pub struct MediaFile {
    /// Unique identifier for this media file
    id: MediaId,
    /// Path to the media file
    path: PathBuf,
    /// Request sender to worker thread
    request_tx: Sender<MediaRequest>,
    /// Shared frame cache (read-only access from main thread, worker writes to it)
    frame_cache: Arc<FrameCache>,
    /// Owned metadata cache (read-only, only used by main thread for GOP lookups)
    metadata_cache: Option<FrameMetadataCache>,
    /// Cached metadata (immutable, set at creation)
    metadata: MediaMetadata,
}

impl MediaFile {
    /// Create a new MediaFile (internal use only, created by MediaStore)
    ///
    /// # Arguments
    /// * `id` - MediaId for this file
    /// * `path` - Path to the media file
    /// * `request_tx` - Request sender channel
    /// * `frame_cache` - Shared frame cache (worker writes, main reads)
    /// * `metadata_cache` - Owned metadata cache (optional, for video files, only main thread uses)
    /// * `metadata` - Cached metadata
    pub(crate) fn new(
        id: MediaId,
        path: PathBuf,
        request_tx: Sender<MediaRequest>,
        frame_cache: Arc<FrameCache>,
        metadata_cache: Option<FrameMetadataCache>,
        metadata: MediaMetadata,
    ) -> Self {
        Self {
            id,
            path,
            request_tx,
            frame_cache,
            metadata_cache,
            metadata,
        }
    }

    /// Start decoding a single video frame at a specific timestamp (non-blocking)
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// FrameReceiver that will receive the decoded VideoFrame when ready
    ///
    /// This method checks the cache first. If the frame is cached, it returns
    /// immediately with the cached frame. Otherwise, it sends a request to the
    /// worker thread and returns immediately. Use `FrameReceiver::try_receive`
    /// to check if the frame is ready without blocking.
    pub fn decode_frame_async(&self, timestamp: f64) -> Result<FrameReceiver> {
        // Fast path: check cache first
        if let Some(frame) = self.get_frame(timestamp) {
            println!(
                "[MediaFile] Frame at timestamp {:.3}s found in cache, returning immediately",
                timestamp
            );
            // Create a pre-filled channel with the cached frame
            let (response_tx, response_rx) = crossbeam::channel::bounded(1);
            let _ = response_tx.send(Ok(frame));
            return Ok(FrameReceiver(response_rx));
        }

        println!(
            "[MediaFile] Frame at timestamp {:.3}s not in cache, sending decode request to worker",
            timestamp
        );

        // Slow path: frame not cached, send request to worker thread
        let (response_tx, response_rx) = crossbeam::channel::bounded(1);

        // Send decode request
        self.request_tx
            .send(MediaRequest::DecodeFrame {
                timestamp,
                response: response_tx,
            })
            .map_err(|_| {
                anyhow::anyhow!("Failed to send decode request (worker thread may have died)")
            })?;

        println!("[MediaFile] Decode request sent, returning receiver");
        Ok(FrameReceiver(response_rx))
    }

    /// Decode a single video frame at a specific timestamp (blocking version for compatibility)
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// Decoded VideoFrame object (already in RGBA format)
    ///
    /// This method blocks until the frame is decoded. For non-blocking operation,
    /// use `decode_frame_async` instead.
    pub fn decode_frame(&self, timestamp: f64) -> Result<VideoFrame> {
        println!(
            "[MediaFile] decode_frame (blocking) called for timestamp: {:.3}s",
            timestamp
        );
        let receiver = self.decode_frame_async(timestamp)?;

        // Block until we get the result (this is the old blocking API)
        match receiver.0.recv() {
            Ok(result) => {
                println!("[MediaFile] Frame received (blocking)");
                result
            }
            Err(_) => {
                anyhow::bail!("Worker thread disconnected")
            }
        }
    }

    /// Decode audio samples between start and end timestamps
    ///
    /// # Arguments
    /// * `start_time` - Start time in seconds
    /// * `end_time` - End time in seconds
    ///
    /// # Returns
    /// AudioSamples object with f32 interleaved sample data
    pub fn decode_samples(&self, start_time: f64, end_time: f64) -> Result<AudioSamples> {
        // Create response channel
        let (response_tx, response_rx) = crossbeam::channel::bounded(1);

        // Send decode request
        self.request_tx
            .send(MediaRequest::DecodeSamples {
                start_time,
                end_time,
                response: response_tx,
            })
            .map_err(|_| {
                anyhow::anyhow!("Failed to send decode request (worker thread may have died)")
            })?;

        // Wait for response with timeout
        match response_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result,
            Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                anyhow::bail!("Decode request timed out after 30 seconds")
            }
            Err(crossbeam::channel::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("Worker thread disconnected")
            }
        }
    }

    /// Get the MediaId for this file
    pub fn id(&self) -> MediaId {
        self.id
    }

    /// Get the path to this file
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the duration of the media file in seconds
    pub fn duration(&self) -> f64 {
        self.metadata.duration
    }

    /// Check if this file has a video stream
    pub fn has_video(&self) -> bool {
        self.metadata.video_info.is_some()
    }

    /// Check if this file has an audio stream
    pub fn has_audio(&self) -> bool {
        self.metadata.audio_info.is_some()
    }

    /// Get video metadata (width, height, fps) if video stream exists
    pub fn video_info(&self) -> Option<(u32, u32, f64)> {
        self.metadata.video_info
    }

    /// Get audio metadata (sample_rate, channels) if audio stream exists
    pub fn audio_info(&self) -> Option<(u32, u16)> {
        self.metadata.audio_info
    }

    /// Get the frame boundaries for a given timestamp
    ///
    /// Given a timestamp, returns the start and end time of the video frame
    /// that contains that timestamp. This is useful for syncing audio samples
    /// with video frames.
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// Some((frame_start, frame_end)) if the media has video, None otherwise
    pub fn frame_boundaries(&self, timestamp: f64) -> Option<(f64, f64)> {
        // Only works for media with video
        let (_, _, fps) = self.metadata.video_info?;

        // Calculate frame duration
        let frame_duration = 1.0 / fps;

        // Find which frame this timestamp falls into
        let frame_number = (timestamp / frame_duration).floor();

        // Calculate frame boundaries
        let frame_start = frame_number * frame_duration;
        let frame_end = (frame_number + 1.0) * frame_duration;

        Some((frame_start, frame_end))
    }

    /// Get the GOP index for a given timestamp
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// Some(GOP index) if video stream exists and timestamp is valid, None otherwise
    pub fn get_gop_index(&self, timestamp: f64) -> Option<usize> {
        self.metadata_cache
            .as_ref()?
            .get_frame_by_timestamp(timestamp)
            .map(|frame_metadata| frame_metadata.gop_id)
    }

    /// Check if a frame at the given timestamp is cached
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// - `Some(true)` if the frame is cached
    /// - `Some(false)` if the frame is not cached but timestamp is valid
    /// - `None` if the media has no video stream or timestamp is invalid
    ///
    /// This is a fast operation (<1ms) that checks the cache directly without
    /// communicating with the worker thread.
    pub fn is_frame_cached(&self, timestamp: f64) -> Option<bool> {
        let metadata_cache = self.metadata_cache.as_ref()?;
        let frame_metadata = metadata_cache.get_frame_by_timestamp(timestamp)?;

        Some(
            self.frame_cache
                .contains_frame(frame_metadata.gop_id, frame_metadata.frame_number),
        )
    }

    /// Get a frame directly from the cache if available (fast path)
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// - `Some(frame)` if the frame is cached and available
    /// - `None` if the frame is not cached, media has no video, or timestamp is invalid
    ///
    /// This is a fast operation (<1ms) that retrieves the frame directly from
    /// the cache without communicating with the worker thread. Use this for
    /// displaying frames during playback when they should already be cached.
    /// If the frame is not cached, use `decode_frame_async()` to trigger decoding.
    pub fn get_frame(&self, timestamp: f64) -> Option<VideoFrame> {
        let metadata_cache = self.metadata_cache.as_ref()?;
        let frame_metadata = metadata_cache.get_frame_by_timestamp(timestamp)?;

        self.frame_cache
            .get_frame(frame_metadata.gop_id, frame_metadata.frame_number)
    }

    /// Check if a GOP at the given index is decoded (cached)
    ///
    /// # Arguments
    /// * `gop_index` - GOP index to check
    ///
    /// # Returns
    /// true if the GOP is decoded and cached, false otherwise
    pub fn is_gop_decoded(&self, gop_index: usize) -> bool {
        self.frame_cache.contains_gop(gop_index)
    }

    /// Manually decode a GOP by index (non-blocking)
    ///
    /// # Arguments
    /// * `gop_index` - GOP index to decode
    ///
    /// # Returns
    /// Receiver that will receive the result when GOP decode completes
    ///
    /// This method sends a request to decode the specified GOP and returns immediately.
    /// The GOP will be decoded in the background and stored in the cache.
    pub fn decode_gop_async(&self, gop_index: usize) -> Result<Receiver<Result<()>>> {
        println!(
            "[MediaFile] Sending decode_gop request for GOP index: {}",
            gop_index
        );

        // Check if already decoded
        if self.frame_cache.contains_gop(gop_index) {
            println!(
                "[MediaFile] GOP {} already decoded, returning immediately",
                gop_index
            );
            let (tx, rx) = crossbeam::channel::bounded(1);
            let _ = tx.send(Ok(()));
            return Ok(rx);
        }

        // Create response channel
        let (response_tx, response_rx) = crossbeam::channel::bounded(1);

        // Send decode request
        self.request_tx
            .send(MediaRequest::DecodeGop {
                gop_index,
                response: response_tx,
            })
            .map_err(|_| {
                anyhow::anyhow!("Failed to send GOP decode request (worker thread may have died)")
            })?;

        println!("[MediaFile] GOP decode request sent, returning receiver");
        Ok(response_rx)
    }

    /// Shutdown the worker thread
    ///
    /// Sends a shutdown request to the worker thread.
    /// The worker will finish processing any in-flight requests and then exit.
    pub fn shutdown(&self) {
        // Send shutdown request (ignore errors if channel is closed)
        let _ = self.request_tx.send(MediaRequest::Shutdown);
    }
}

impl FrameReceiver {
    /// Try to receive a frame from a decode request (non-blocking)
    ///
    /// # Returns
    /// - `Ok(Some(frame))` if frame is ready
    /// - `Ok(None)` if frame is not ready yet
    /// - `Err(e)` if there was an error or channel disconnected
    pub fn try_receive(&self) -> Result<Option<VideoFrame>> {
        match self.0.try_recv() {
            Ok(result) => {
                println!("[FrameReceiver] Frame received from worker");
                result.map(Some)
            }
            Err(crossbeam::channel::TryRecvError::Empty) => {
                // Frame not ready yet
                Ok(None)
            }
            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                anyhow::bail!("Worker thread disconnected")
            }
        }
    }
}
