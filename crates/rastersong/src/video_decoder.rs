use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // RGBA format
}

pub struct VideoDecoder {
    pipeline: gst::Pipeline,
    #[allow(dead_code)]
    appsink: gst_app::AppSink,
    current_frame: Arc<Mutex<Option<VideoFrame>>>,
}

impl VideoDecoder {
    pub fn new(video_path: &str) -> Result<Self> {
        // Create pipeline: filesrc ! decodebin ! videoconvert ! appsink
        let pipeline = gst::Pipeline::new();

        let filesrc = gst::ElementFactory::make("filesrc")
            .property("location", video_path)
            .build()
            .context("Failed to create filesrc")?;

        let decodebin = gst::ElementFactory::make("decodebin")
            .build()
            .context("Failed to create decodebin")?;

        let videoconvert = gst::ElementFactory::make("videoconvert")
            .build()
            .context("Failed to create videoconvert")?;

        let appsink = gst_app::AppSink::builder()
            .caps(
                &gst_video::VideoCapsBuilder::new()
                    .format(gst_video::VideoFormat::Rgba)
                    .build(),
            )
            .build();

        pipeline.add_many([&filesrc, &decodebin, &videoconvert, appsink.upcast_ref()])?;

        gst::Element::link_many([&filesrc, &decodebin])?;
        gst::Element::link_many([&videoconvert, appsink.upcast_ref()])?;

        // Connect decodebin's pad-added signal for video
        let videoconvert_weak = videoconvert.downgrade();
        decodebin.connect_pad_added(move |_, pad| {
            let caps = match pad.current_caps() {
                Some(caps) => caps,
                None => return,
            };

            let structure = match caps.structure(0) {
                Some(structure) => structure,
                None => return,
            };

            // Only link video pads
            if structure.name().starts_with("video/") {
                if let Some(videoconvert) = videoconvert_weak.upgrade() {
                    let sink_pad = videoconvert
                        .static_pad("sink")
                        .expect("Failed to get video sink pad");

                    if !sink_pad.is_linked() {
                        let _ = pad.link(&sink_pad);
                    }
                }
            }
        });

        let current_frame = Arc::new(Mutex::new(None));
        let current_frame_clone = current_frame.clone();

        // Set up callback to receive frames
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let caps = sample.caps().ok_or(gst::FlowError::Error)?;

                    let video_info =
                        gst_video::VideoInfo::from_caps(caps).map_err(|_| gst::FlowError::Error)?;

                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                    let frame = VideoFrame {
                        width: video_info.width(),
                        height: video_info.height(),
                        data: map.as_slice().to_vec(),
                    };

                    *current_frame_clone.lock().unwrap() = Some(frame);

                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        // Set to paused state to trigger preroll
        pipeline
            .set_state(gst::State::Paused)
            .context("Failed to set pipeline to paused")?;

        // Wait for state change to complete (with timeout)
        let _ = pipeline.state(gst::ClockTime::from_seconds(5));

        Ok(Self {
            pipeline,
            appsink,
            current_frame,
        })
    }

    pub fn play(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Playing)
            .context("Failed to set pipeline to playing")?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Paused)
            .context("Failed to set pipeline to paused")?;
        Ok(())
    }

    pub fn seek(&self, position_ns: u64) -> Result<()> {
        self.pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::ClockTime::from_nseconds(position_ns),
        )?;
        Ok(())
    }

    pub fn get_position(&self) -> Option<u64> {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|pos| pos.nseconds())
    }

    pub fn get_duration(&self) -> Option<u64> {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|dur| dur.nseconds())
    }

    pub fn get_current_frame(&self) -> Option<VideoFrame> {
        self.current_frame.lock().unwrap().clone()
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
