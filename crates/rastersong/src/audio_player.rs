use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;

pub struct AudioPlayer {
    pipeline: gst::Pipeline,
}

impl AudioPlayer {
    pub fn new(audio_path: &str) -> Result<Self> {
        // Create pipeline: filesrc ! decodebin ! audioconvert ! audioresample ! autoaudiosink
        let pipeline = gst::Pipeline::new();

        let filesrc = gst::ElementFactory::make("filesrc")
            .property("location", audio_path)
            .build()
            .context("Failed to create filesrc")?;

        let decodebin = gst::ElementFactory::make("decodebin")
            .build()
            .context("Failed to create decodebin")?;

        let audioconvert = gst::ElementFactory::make("audioconvert")
            .build()
            .context("Failed to create audioconvert")?;

        let audioresample = gst::ElementFactory::make("audioresample")
            .build()
            .context("Failed to create audioresample")?;

        let autoaudiosink = gst::ElementFactory::make("autoaudiosink")
            .build()
            .context("Failed to create autoaudiosink")?;

        pipeline.add_many([
            &filesrc,
            &decodebin,
            &audioconvert,
            &audioresample,
            &autoaudiosink,
        ])?;

        gst::Element::link_many([&filesrc, &decodebin])?;
        gst::Element::link_many([&audioconvert, &audioresample, &autoaudiosink])?;

        // Connect decodebin's pad-added signal for audio
        let audioconvert_weak = audioconvert.downgrade();
        decodebin.connect_pad_added(move |_, pad| {
            let caps = match pad.current_caps() {
                Some(caps) => caps,
                None => return,
            };

            let structure = match caps.structure(0) {
                Some(structure) => structure,
                None => return,
            };

            // Only link audio pads
            if structure.name().starts_with("audio/") {
                if let Some(audioconvert) = audioconvert_weak.upgrade() {
                    let sink_pad = audioconvert
                        .static_pad("sink")
                        .expect("Failed to get audio sink pad");

                    if !sink_pad.is_linked() {
                        let _ = pad.link(&sink_pad);
                    }
                }
            }
        });

        // Set to paused state initially
        pipeline
            .set_state(gst::State::Paused)
            .context("Failed to set pipeline to paused")?;

        // Wait for state change
        let _ = pipeline.state(gst::ClockTime::from_seconds(5));

        Ok(Self { pipeline })
    }

    pub fn play(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Playing)
            .context("Failed to set audio pipeline to playing")?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Paused)
            .context("Failed to set audio pipeline to paused")?;
        Ok(())
    }

    pub fn seek(&self, position_ns: u64) -> Result<()> {
        self.pipeline.seek_simple(
            gst::SeekFlags::FLUSH,
            gst::ClockTime::from_nseconds(position_ns),
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_position(&self) -> Option<u64> {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|pos| pos.nseconds())
    }

    #[allow(dead_code)]
    pub fn get_duration(&self) -> Option<u64> {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|dur| dur.nseconds())
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
