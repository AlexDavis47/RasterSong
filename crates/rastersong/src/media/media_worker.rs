//! Media worker thread that owns FFmpeg resources and processes decode requests

use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, Sender};
use ffmpeg_next as ffmpeg;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use super::audio_decoder::AudioDecoder;
use super::audio_samples::AudioSamples;
use super::frame_cache::FrameCache;
use super::media_id::MediaId;
use super::video_decoder::VideoDecoder;
use super::video_frame::VideoFrame;

/// Request types sent to the media worker
pub enum MediaRequest {
    /// Decode a single video frame at a specific timestamp
    DecodeFrame {
        timestamp: f64,
        response: Sender<Result<VideoFrame>>,
    },
    /// Decode a GOP by index (manual pre-decode)
    DecodeGop {
        gop_index: usize,
        response: Sender<Result<()>>,
    },
    /// Decode audio samples between start and end timestamps
    DecodeSamples {
        start_time: f64,
        end_time: f64,
        response: Sender<Result<AudioSamples>>,
    },
    /// Shutdown the worker thread
    Shutdown,
}

/// Media worker that owns FFmpeg resources and processes decode requests
pub struct MediaWorker {
    /// Unique identifier for this media file
    id: MediaId,
    /// Path to the media file
    path: PathBuf,
    /// FFmpeg format context (the opened file handle)
    format_context: ffmpeg::format::context::Input,
    /// Video decoder (if video stream exists)
    video_decoder: Option<VideoDecoder>,
    /// Audio decoder (if audio stream exists)
    audio_decoder: Option<AudioDecoder>,
    /// Shared frame cache
    frame_cache: Arc<FrameCache>,
    /// Request receiver
    request_rx: Receiver<MediaRequest>,
}

impl MediaWorker {
    /// Create a new media worker
    ///
    /// # Arguments
    /// * `id` - MediaId for this file
    /// * `path` - Path to the media file
    /// * `format_context` - Already opened FFmpeg format context
    /// * `video_decoder` - Video decoder (if video stream exists)
    /// * `audio_decoder` - Audio decoder (if audio stream exists)
    /// * `frame_cache` - Shared frame cache
    /// * `request_rx` - Request receiver channel
    pub fn new(
        id: MediaId,
        path: PathBuf,
        format_context: ffmpeg::format::context::Input,
        video_decoder: Option<VideoDecoder>,
        audio_decoder: Option<AudioDecoder>,
        frame_cache: Arc<FrameCache>,
        request_rx: Receiver<MediaRequest>,
    ) -> Self {
        Self {
            id,
            path,
            format_context,
            video_decoder,
            audio_decoder,
            frame_cache,
            request_rx,
        }
    }

    /// Run the worker loop, processing requests until shutdown
    pub fn run(mut self) {
        println!(
            "[MediaWorker] Worker thread started for media ID: {}",
            self.id
        );
        loop {
            println!("[MediaWorker] Waiting for request...");
            match self.request_rx.recv() {
                Ok(MediaRequest::DecodeFrame {
                    timestamp,
                    response,
                }) => {
                    println!(
                        "[MediaWorker] Received DecodeFrame request for timestamp: {:.3}s",
                        timestamp
                    );
                    let result = self.handle_decode_frame(timestamp);
                    println!("[MediaWorker] DecodeFrame completed, sending response");
                    // Send response (ignore if receiver is dropped)
                    let _ = response.send(result);
                }
                Ok(MediaRequest::DecodeGop {
                    gop_index,
                    response,
                }) => {
                    println!(
                        "[MediaWorker] Received DecodeGop request for GOP index: {}",
                        gop_index
                    );
                    let result = self.handle_decode_gop(gop_index);
                    println!("[MediaWorker] DecodeGop completed, sending response");
                    // Send response (ignore if receiver is dropped)
                    let _ = response.send(result);
                }
                Ok(MediaRequest::DecodeSamples {
                    start_time,
                    end_time,
                    response,
                }) => {
                    println!(
                        "[MediaWorker] Received DecodeSamples request for {:.3}s - {:.3}s",
                        start_time, end_time
                    );
                    let result = self.handle_decode_samples(start_time, end_time);
                    println!("[MediaWorker] DecodeSamples completed, sending response");
                    // Send response (ignore if receiver is dropped)
                    let _ = response.send(result);
                }
                Ok(MediaRequest::Shutdown) => {
                    println!("[MediaWorker] Received Shutdown request, exiting");
                    break;
                }
                Err(_) => {
                    println!("[MediaWorker] Channel closed, shutting down");
                    // Channel closed, shutdown
                    break;
                }
            }
        }
        println!("[MediaWorker] Worker thread exiting");
    }

    /// Handle a decode frame request
    fn handle_decode_frame(&mut self, timestamp: f64) -> Result<VideoFrame> {
        println!(
            "[MediaWorker::handle_decode_frame] Starting decode for timestamp: {:.3}s",
            timestamp
        );

        let video_decoder = self
            .video_decoder
            .as_mut()
            .context("No video decoder available")?;

        // Get frame metadata for timestamp
        let frame_metadata = video_decoder
            .metadata_cache()
            .get_frame_by_timestamp(timestamp)
            .context("No frame found at specified timestamp")?;

        let gop_id = frame_metadata.gop_id;
        let frame_number = frame_metadata.frame_number;

        println!(
            "[MediaWorker::handle_decode_frame] Frame metadata: GOP={}, frame={}",
            gop_id, frame_number
        );

        // Check cache first (non-blocking read)
        if let Some(cached_frame) = self.frame_cache.get_frame(gop_id, frame_number) {
            println!(
                "[MediaWorker::handle_decode_frame] Frame found in cache, returning cached frame"
            );
            return Ok(cached_frame);
        }

        println!(
            "[MediaWorker::handle_decode_frame] Frame not in cache, decoding GOP {} (this may take a while)...",
            gop_id
        );
        // Frame not in cache, decode the entire GOP
        let start = std::time::Instant::now();
        video_decoder
            .decode_gop(&mut self.format_context, gop_id, &self.frame_cache)
            .with_context(|| {
                format!(
                    "Failed to decode GOP {} for frame at timestamp {}",
                    gop_id, timestamp
                )
            })?;
        let elapsed = start.elapsed();
        println!(
            "[MediaWorker::handle_decode_frame] GOP {} decoded in {:.2}ms",
            gop_id,
            elapsed.as_secs_f64() * 1000.0
        );

        // After GOP decoding, retrieve the frame from cache
        println!("[MediaWorker::handle_decode_frame] Retrieving frame from cache");
        self.frame_cache
            .get_frame(gop_id, frame_number)
            .context("Frame not found in cache after GOP decoding")
    }

    /// Handle a manual GOP decode request
    fn handle_decode_gop(&mut self, gop_index: usize) -> Result<()> {
        println!(
            "[MediaWorker::handle_decode_gop] Starting manual decode for GOP index: {}",
            gop_index
        );

        let video_decoder = self
            .video_decoder
            .as_mut()
            .context("No video decoder available")?;

        // Check if already decoded
        if self.frame_cache.contains_gop(gop_index) {
            println!(
                "[MediaWorker::handle_decode_gop] GOP {} already decoded",
                gop_index
            );
            return Ok(());
        }

        // Decode the entire GOP
        println!(
            "[MediaWorker::handle_decode_gop] Decoding GOP {} (this may take a while)...",
            gop_index
        );
        let start = std::time::Instant::now();
        video_decoder
            .decode_gop(&mut self.format_context, gop_index, &self.frame_cache)
            .with_context(|| format!("Failed to decode GOP {}", gop_index))?;
        let elapsed = start.elapsed();
        println!(
            "[MediaWorker::handle_decode_gop] GOP {} decoded in {:.2}ms",
            gop_index,
            elapsed.as_secs_f64() * 1000.0
        );

        Ok(())
    }

    /// Handle a decode samples request
    fn handle_decode_samples(&mut self, start_time: f64, end_time: f64) -> Result<AudioSamples> {
        let audio_decoder = self
            .audio_decoder
            .as_mut()
            .context("No audio decoder available")?;

        // Decode FFmpeg audio frame
        let ffmpeg_audio =
            audio_decoder.decode_samples(&mut self.format_context, start_time, end_time)?;

        // Convert to AudioSamples wrapper
        AudioSamples::from_ffmpeg(&ffmpeg_audio, start_time, end_time)
    }
}

/// Spawn a media worker thread
///
/// # Arguments
/// * `worker` - MediaWorker instance to run
///
/// # Returns
/// JoinHandle for the spawned thread
pub fn spawn_worker(worker: MediaWorker) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        worker.run();
    })
}
