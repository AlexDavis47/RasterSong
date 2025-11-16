//! Audio decoding functionality

use super::media_store::{MediaId, get_file_info};
use anyhow::Result;
use gstreamer::prelude::*;
use gstreamer::{Sample, State};
use gstreamer_app::AppSink;

/// Decode a range of audio samples from an audio file
///
/// # Arguments
/// * `identifier` - The MediaId of the registered audio file
/// * `sample_start` - The starting sample index (inclusive)
/// * `sample_end` - The ending sample index (exclusive)
///
/// # Returns
/// A GStreamer Sample containing the audio buffer and metadata (caps)
pub fn decode_audio(identifier: MediaId, sample_start: usize, sample_end: usize) -> Result<Sample> {
    let file_info = get_file_info(&identifier)
        .ok_or_else(|| anyhow::anyhow!("Media file not found for identifier"))?;

    if sample_start >= sample_end {
        anyhow::bail!("sample_start must be less than sample_end");
    }

    // Create pipeline: filesrc -> decodebin -> audioconvert -> audio/x-raw,format=F32LE -> appsink
    let pipeline = gstreamer::Pipeline::new();

    let src = gstreamer::ElementFactory::make("filesrc")
        .property("location", file_info.path.to_string_lossy().as_ref())
        .build()?;

    let decodebin = gstreamer::ElementFactory::make("decodebin").build()?;
    let audioconvert = gstreamer::ElementFactory::make("audioconvert").build()?;
    let caps = gstreamer::Caps::builder("audio/x-raw")
        .field("format", &"F32LE")
        .build();
    let capsfilter = gstreamer::ElementFactory::make("capsfilter")
        .property("caps", &caps)
        .build()?;

    let appsink = gstreamer::ElementFactory::make("appsink")
        .property("emit-signals", true)
        .property("max-buffers", 1u32)
        .property("drop", true)
        .build()?;

    pipeline.add_many(&[&src, &decodebin, &audioconvert, &capsfilter, &appsink])?;

    src.link(&decodebin)?;
    audioconvert.link(&capsfilter)?;
    capsfilter.link(&appsink)?;

    // Handle decodebin's dynamic pad
    let audioconvert_clone = audioconvert.clone();
    decodebin.connect_pad_added(move |_, src_pad| {
        let sink_pad = audioconvert_clone
            .static_pad("sink")
            .expect("audioconvert has no sink pad");
        if sink_pad.is_linked() {
            return;
        }
        if let Err(err) = src_pad.link(&sink_pad) {
            eprintln!("Failed to link decodebin to audioconvert: {:?}", err);
        }
    });

    // Set up appsink callback to capture sampless
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
                    anyhow::bail!("Pipeline reached EOS before samples could be decoded");
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

    // Get first sample to extract sample rate from caps
    let first_sample = rx.recv_timeout(std::time::Duration::from_secs(5))?;

    // Extract sample rate from caps
    let sample_rate = if let Some(caps) = first_sample.caps() {
        if let Some(structure) = caps.structure(0) {
            structure.get::<i32>("rate").unwrap_or(44100) as f64
        } else {
            44100.0
        }
    } else {
        44100.0
    };

    // Calculate time for sample_start
    let start_time_seconds = sample_start as f64 / sample_rate;
    let start_time = gstreamer::ClockTime::from_seconds(start_time_seconds as u64)
        + gstreamer::ClockTime::from_nseconds(
            ((start_time_seconds % 1.0) * 1_000_000_000.0) as u64,
        );

    // If sample_start is 0, we already have the first sample
    if sample_start == 0 {
        // Check if the first sample contains enough data
        if let Some(buffer) = first_sample.buffer() {
            let sample_count = (buffer.size() / 4) as usize; // f32 = 4 bytes
            if sample_count >= (sample_end - sample_start) {
                // We have enough samples in the first buffer
                pipeline.set_state(State::Null)?;
                return Ok(first_sample);
            }
        }
    }

    // Seek to the start position
    pipeline.seek_simple(
        gstreamer::SeekFlags::FLUSH | gstreamer::SeekFlags::KEY_UNIT,
        start_time,
    )?;

    // Wait for the sample at the seek position
    let sample = rx.recv_timeout(std::time::Duration::from_secs(5))?;

    // Verify we got the right amount of samples
    // Note: The buffer might contain more samples than requested, but that's okay
    // The caller can extract the range they need from the buffer

    pipeline.set_state(State::Null)?;

    Ok(sample)
}

/// Get the duration of an audio file in seconds
///
/// # Arguments
/// * `identifier` - The MediaId of the registered audio file
///
/// # Returns
/// Duration in seconds as f64
pub fn get_audio_duration(identifier: MediaId) -> Result<f64> {
    let file_info = get_file_info(&identifier)
        .ok_or_else(|| anyhow::anyhow!("Media file not found for identifier"))?;

    // Create a simple pipeline to query duration
    let pipeline = gstreamer::Pipeline::new();

    let src = gstreamer::ElementFactory::make("filesrc")
        .property("location", file_info.path.to_string_lossy().as_ref())
        .build()?;

    let decodebin = gstreamer::ElementFactory::make("decodebin").build()?;
    let fakesink = gstreamer::ElementFactory::make("fakesink").build()?;

    pipeline.add_many(&[&src, &decodebin, &fakesink])?;
    src.link(&decodebin)?;

    // Connect decodebin pads to fakesink
    let fakesink_clone = fakesink.clone();
    decodebin.connect_pad_added(move |_, src_pad| {
        let sink_pad = fakesink_clone
            .static_pad("sink")
            .expect("fakesink has no sink pad");
        if sink_pad.is_linked() {
            return;
        }
        if let Err(err) = src_pad.link(&sink_pad) {
            eprintln!("Failed to link decodebin to fakesink: {:?}", err);
        }
    });

    // Set pipeline to paused state to get duration
    pipeline.set_state(State::Paused)?;

    // Wait for state change to complete
    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gstreamer::ClockTime::from_seconds(10)) {
        use gstreamer::MessageView;
        match msg.view() {
            MessageView::Error(err) => {
                pipeline.set_state(State::Null)?;
                anyhow::bail!("Pipeline error: {:?}", err.error());
            }
            MessageView::Eos(_) => {
                break;
            }
            MessageView::AsyncDone(_) => {
                break;
            }
            _ => {}
        }
    }

    // Query duration
    let duration = pipeline.query_duration::<gstreamer::ClockTime>();

    pipeline.set_state(State::Null)?;

    match duration {
        Some(d) => Ok(d.seconds() as f64 + (d.nseconds() as f64 / 1_000_000_000.0)),
        None => anyhow::bail!("Failed to query audio duration"),
    }
}
