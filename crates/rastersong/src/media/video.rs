//! Video decoding functionality

use super::media_store::{MediaId, get_file_info};
use anyhow::Result;
use gstreamer::prelude::*;
use gstreamer::{Sample, State};
use gstreamer_app::AppSink;

/// Decode a specific frame from a video file
///
/// # Arguments
/// * `identifier` - The MediaId of the registered video file
/// * `frame_index` - The zero-based index of the frame to decode
///
/// # Returns
/// A GStreamer Sample containing the frame buffer and metadata (caps)
pub fn decode_video(identifier: MediaId, frame_index: usize) -> Result<Sample> {
    let file_info = get_file_info(&identifier)
        .ok_or_else(|| anyhow::anyhow!("Media file not found for identifier"))?;

    // Create pipeline: filesrc -> decodebin -> videoconvert -> video/x-raw,format=RGB -> appsink
    let pipeline = gstreamer::Pipeline::new();

    let src = gstreamer::ElementFactory::make("filesrc")
        .property("location", file_info.path.to_string_lossy().as_ref())
        .build()?;

    let decodebin = gstreamer::ElementFactory::make("decodebin").build()?;
    let videoconvert = gstreamer::ElementFactory::make("videoconvert").build()?;
    let caps = gstreamer::Caps::builder("video/x-raw")
        .field("format", &gstreamer_video::VideoFormat::Rgb.to_str())
        .build();
    let capsfilter = gstreamer::ElementFactory::make("capsfilter")
        .property("caps", &caps)
        .build()?;

    let appsink = gstreamer::ElementFactory::make("appsink")
        .property("emit-signals", true)
        .property("max-buffers", 1u32)
        .property("drop", true)
        .build()?;

    pipeline.add_many(&[&src, &decodebin, &videoconvert, &capsfilter, &appsink])?;

    src.link(&decodebin)?;
    videoconvert.link(&capsfilter)?;
    capsfilter.link(&appsink)?;

    // Handle decodebin's dynamic pad
    let videoconvert_clone = videoconvert.clone();
    decodebin.connect_pad_added(move |_, src_pad| {
        let sink_pad = videoconvert_clone
            .static_pad("sink")
            .expect("videoconvert has no sink pad");
        if sink_pad.is_linked() {
            return;
        }
        if let Err(err) = src_pad.link(&sink_pad) {
            eprintln!("Failed to link decodebin to videoconvert: {:?}", err);
        }
    });

    // Set up appsink callback to capture frame
    let appsink = appsink.downcast::<AppSink>().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();

    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let sample = appsink
                    .pull_sample()
                    .map_err(|_| gstreamer::FlowError::Eos)?;
                let _ = tx.send(sample.clone());
                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );

    // Start pipeline
    pipeline.set_state(State::Playing)?;

    // Wait for pipeline to be ready
    let bus = pipeline.bus().unwrap();
    loop {
        let msg = bus.timed_pop_filtered(
            gstreamer::ClockTime::NONE,
            &[
                gstreamer::MessageType::Error,
                gstreamer::MessageType::Eos,
                gstreamer::MessageType::StateChanged,
            ],
        );

        if let Some(msg) = msg {
            use gstreamer::MessageView;
            match msg.view() {
                MessageView::Error(err) => {
                    anyhow::bail!("Pipeline error: {:?}", err.error());
                }
                MessageView::Eos(_) => {
                    anyhow::bail!("Pipeline reached EOS before frame could be decoded");
                }
                MessageView::StateChanged(state_changed) => {
                    if state_changed
                        .src()
                        .map(|s| s == pipeline.as_ref() as &gstreamer::Object)
                        .unwrap_or(false)
                    {
                        let new_state = state_changed.current();
                        if new_state == State::Playing {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Get first frame to extract framerate from caps
    let first_sample = rx.recv_timeout(std::time::Duration::from_secs(5))?;

    // Extract framerate from caps
    let fps = if let Some(caps) = first_sample.caps() {
        if let Some(structure) = caps.structure(0) {
            if let Ok(framerate) = structure.get::<gstreamer::Fraction>("framerate") {
                framerate.numer() as f64 / framerate.denom() as f64
            } else {
                30.0 // Default to 30fps if we can't get framerate
            }
        } else {
            30.0
        }
    } else {
        30.0
    };

    // If frame_index is 0, return the first frame we already got
    if frame_index == 0 {
        pipeline.set_state(State::Null)?;
        return Ok(first_sample);
    }

    // Calculate time for desired frame
    let frame_time_seconds = frame_index as f64 / fps;
    let frame_time = gstreamer::ClockTime::from_seconds(frame_time_seconds as u64)
        + gstreamer::ClockTime::from_nseconds(
            ((frame_time_seconds % 1.0) * 1_000_000_000.0) as u64,
        );

    // Seek to the desired frame time
    pipeline.seek_simple(
        gstreamer::SeekFlags::FLUSH | gstreamer::SeekFlags::KEY_UNIT,
        frame_time,
    )?;

    // Wait for the frame at the seek position
    let sample = rx.recv_timeout(std::time::Duration::from_secs(5))?;

    pipeline.set_state(State::Null)?;

    Ok(sample)
}
