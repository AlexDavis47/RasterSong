//! Export: a project rendered from the start into a file, with progress, cancel and the latest
//! frame for a preview. The CLI and the GUI's export dialog both go through here.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rastersong_graph::{GraphDesc, Registry, Tempo, render_form};
use rastersong_media::{AudioCache, ExportFormat, MediaBackend, MediaWriter};

use crate::playback::{MixTrack, Mixer};
use crate::sources::Modulator;
use crate::timeline::{Bus, Timeline, TrackKind};
use crate::{
    AudioSink, Composition, EngineError, FrameSink, Project, RenderInfo, RenderSettings,
    RenderTrack, RenderedFrame, TrackMedia,
};

/// What to render: a project, as the render needs it.
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub timeline: Timeline,
    /// The graph rendered over the whole timeline, or, with `composition`, the description of
    /// the open graph among the project's graphs. A project without graph items renders its mix.
    pub graph: GraphDesc,
    /// The project's composition; `None` renders `graph` over the whole timeline.
    pub composition: Option<Composition>,
    pub tempo: Tempo,
    pub audio_rate: u32,
}

impl ExportJob {
    pub fn from_project(project: &Project) -> Self {
        Self {
            timeline: project.timeline(),
            // The open graph is applied only where a graph item uses it; without one the mix
            // is rendered.
            graph: render_form(&project.graph, Registry::shared()),
            composition: Some(project.composition()),
            tempo: project.tempo,
            audio_rate: project.audio_rate,
        }
    }

    /// The buses the project can export: its own, or the stereo main bus when it lists none.
    pub fn buses(&self) -> Vec<Bus> {
        if self.timeline.buses.is_empty() {
            vec![Bus::main()]
        } else {
            self.timeline.buses.clone()
        }
    }

    /// The bus named `name`.
    pub fn bus(&self, name: &str) -> Option<Bus> {
        self.buses().into_iter().find(|b| b.name == name)
    }

    /// Decodes every audio track. Returns the tracks to render and the audio tracks the sound
    /// can be mixed from.
    pub fn load(
        &self,
        backend: &dyn MediaBackend,
        cache: Option<&AudioCache>,
    ) -> Result<(Vec<RenderTrack>, Vec<SourceTrack>), EngineError> {
        let mut tracks = Vec::new();
        let mut sources = Vec::new();
        for spec in &self.timeline.tracks {
            let media = match spec.kind {
                TrackKind::Video => TrackMedia::Video {
                    path: spec.path.clone(),
                    stream: spec.stream,
                },
                TrackKind::Audio => {
                    let clip = match cache {
                        Some(cache) => cache.load(backend, &spec.path, spec.audio_options()),
                        None => backend.load_audio(&spec.path, spec.audio_options()),
                    }
                    .map_err(|error| EngineError::AudioTrack {
                        name: spec.name.clone(),
                        error,
                    })?;
                    let modulator = Arc::new(Modulator::new(&clip));
                    sources.push(SourceTrack {
                        bus: spec.bus.clone(),
                        track: MixTrack {
                            clip: Arc::new(clip),
                            items: spec.items.clone(),
                            gain: spec.gain,
                        },
                    });
                    TrackMedia::Audio(modulator)
                }
            };
            tracks.push(RenderTrack {
                name: spec.name.clone(),
                media,
                items: spec.items.clone(),
                in_mix: spec.gain > 0.0,
            });
        }
        Ok((tracks, sources))
    }

    /// The render settings for `bus` at `size` (the project's for `None`), writing `range` (every
    /// frame for `None`).
    pub fn render_settings(
        &self,
        bus: Bus,
        size: Option<(u32, u32)>,
        range: Option<Range<usize>>,
    ) -> RenderSettings {
        RenderSettings {
            timebase: self.timeline.timebase,
            size,
            range,
            tempo: self.tempo,
            audio_rate: Some(self.audio_rate),
            bus,
            composition: self.composition.clone(),
        }
    }
}

/// How to export a job into a file.
#[derive(Debug, Clone)]
pub struct ExportSettings {
    pub format: ExportFormat,
    /// Output size; `None` is the project's.
    pub size: Option<(u32, u32)>,
    /// The frames to export, `start..end`; `None` is the whole project.
    pub range: Option<Range<usize>>,
    /// The bus whose sound is written.
    pub bus: Bus,
}

/// The widest an export keeps its latest frame for a preview.
pub const PREVIEW_WIDTH: usize = 480;

/// A frame an export wrote, scaled down to at most [`PREVIEW_WIDTH`] wide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFrame {
    pub index: usize,
    pub width: usize,
    pub height: usize,
    /// Three bytes a pixel.
    pub rgb: Vec<u8>,
}

/// The latest frame an export wrote, for a preview while it runs. A frame is copied in only once
/// the previous one has been taken, so a preview that looks a few times a second costs little.
#[derive(Debug, Default)]
pub struct LatestFrame(Mutex<Option<PreviewFrame>>);

impl LatestFrame {
    /// The frame written since the last take, if any.
    pub fn take(&self) -> Option<PreviewFrame> {
        self.0.lock().ok()?.take()
    }

    /// Keeps frame `index` (`width` by `height`, packed RGB), scaled down, unless the last one
    /// is still waiting to be taken.
    fn offer(&self, index: usize, width: usize, height: usize, rgb: &[u8]) {
        let Ok(mut slot) = self.0.lock() else {
            return;
        };
        if slot.is_none() && width > 0 && height > 0 && rgb.len() == width * height * 3 {
            *slot = Some(scaled(index, width, height, rgb));
        }
    }
}

/// Frame `index` scaled to at most [`PREVIEW_WIDTH`] wide, nearest pixel, keeping its shape.
fn scaled(index: usize, width: usize, height: usize, rgb: &[u8]) -> PreviewFrame {
    let to_width = width.min(PREVIEW_WIDTH);
    let to_height = (height * to_width / width).max(1);
    let mut out = Vec::with_capacity(to_width * to_height * 3);
    for y in 0..to_height {
        let row = y * height / to_height * width;
        for x in 0..to_width {
            let at = (row + x * width / to_width) * 3;
            out.extend_from_slice(&rgb[at..at + 3]);
        }
    }
    PreviewFrame {
        index,
        width: to_width,
        height: to_height,
        rgb: out,
    }
}

/// Passes frames on to `sink`, keeping each in `preview` when it is wanted.
struct Previewed<'a> {
    sink: &'a mut dyn FrameSink,
    preview: Option<&'a LatestFrame>,
    size: (usize, usize),
}

impl FrameSink for Previewed<'_> {
    fn start(&mut self, info: &RenderInfo, audio: &AudioSink) -> Result<(), EngineError> {
        self.size = (info.width as usize, info.height as usize);
        self.sink.start(info, audio)
    }

    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError> {
        if let Some(preview) = self.preview {
            preview.offer(frame.index, self.size.0, self.size.1, frame.rgb);
        }
        self.sink.frame(frame)
    }
}

/// Renders `job` into `path`. `progress` hears the frames done and the total after each frame,
/// and stops the export by returning `false`; `preview`, if given, keeps the latest frame
/// written. A failed or cancelled export removes the file.
pub fn export(
    backend: &dyn MediaBackend,
    cache: Option<&AudioCache>,
    job: &ExportJob,
    settings: &ExportSettings,
    path: &Path,
    preview: Option<&LatestFrame>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<RenderInfo, EngineError> {
    let _span = tracing::trace_span!("export").entered();
    let result = (|| {
        let (tracks, sources) = {
            let _span = tracing::trace_span!("load").entered();
            job.load(backend, cache)?
        };
        let render_settings =
            job.render_settings(settings.bus.clone(), settings.size, settings.range.clone());
        let mut sink = FileSink::new(
            path,
            settings.format,
            sources,
            job.audio_rate,
            settings.bus.clone(),
        );
        let info = crate::render(
            backend,
            &tracks,
            &job.graph,
            &render_settings,
            &mut Previewed {
                sink: &mut sink,
                preview,
                size: (0, 0),
            },
            progress,
        )?;
        sink.finish()?;
        Ok(info)
    })();
    if result.is_err() {
        // Nothing to remove when the file was never created.
        let _ = std::fs::remove_file(path);
    }
    result
}

/// An audio track the exported sound can be made of, when the graph renders none.
#[derive(Debug, Clone)]
pub struct SourceTrack {
    /// The bus it is summed into in the track mix.
    pub bus: String,
    pub track: MixTrack,
}

/// Writes rendered frames into a file in an [`ExportFormat`], with the sound of a bus: the
/// graph's, or else the mix of the tracks routed to it, mixed frame by frame as the render goes.
#[derive(Debug)]
pub struct FileSink {
    path: PathBuf,
    format: ExportFormat,
    /// The audio tracks, mixed into the sound when the graph doesn't render its own.
    tracks: Vec<SourceTrack>,
    /// The rate the mix is written at.
    audio_rate: u32,
    /// The bus written: the track mix takes its tracks and its channels.
    bus: Bus,
    writer: Option<MediaWriter>,
    /// The track mix, when the graph renders no sound, and the sample frame mixed up to; it
    /// starts at the first frame written.
    mix: Option<(Mixer, Option<usize>)>,
    frame_rate: f64,
}

impl FileSink {
    pub fn new(
        path: &Path,
        format: ExportFormat,
        tracks: Vec<SourceTrack>,
        audio_rate: u32,
        bus: Bus,
    ) -> Self {
        Self {
            path: path.to_owned(),
            format,
            tracks,
            audio_rate,
            bus,
            writer: None,
            mix: None,
            frame_rate: 0.0,
        }
    }

    /// Finishes the file. Does nothing if no frame was rendered.
    pub fn finish(self) -> Result<(), EngineError> {
        match self.writer {
            Some(writer) => Ok(writer.finish()?),
            None => Ok(()),
        }
    }
}

impl FrameSink for FileSink {
    fn start(&mut self, info: &RenderInfo, sink: &AudioSink) -> Result<(), EngineError> {
        let (sample_rate, channels) = match sink {
            // The graph's own sound arrives with each frame.
            AudioSink::Rendered {
                sample_rate,
                channels,
            } => (*sample_rate, *channels),
            // The tracks as placed on the timeline: the bus's audio mix.
            AudioSink::TrackMix => {
                self.mix = Some((bus_mixer(&self.tracks, &self.bus), None));
                (self.audio_rate, self.bus.channels)
            }
        };
        self.frame_rate = info.frame_rate.as_f64();
        self.writer = Some(MediaWriter::create_streaming(
            &self.path,
            self.format,
            info.width,
            info.height,
            info.frame_rate,
            sample_rate,
            channels,
        )?);
        Ok(())
    }

    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError> {
        let writer = self.writer.as_mut().expect("started");
        if let Some(audio) = frame.audio {
            writer.push_audio(&audio.samples);
        }
        if let Some((mixer, mixed)) = &mut self.mix {
            let _span = tracing::trace_span!("mix_audio").entered();
            let sample = |frame: usize| {
                (frame as f64 / self.frame_rate * f64::from(self.audio_rate)).round() as usize
            };
            let from = mixed.unwrap_or_else(|| sample(frame.index));
            let end = sample(frame.index + 1);
            let block = mix_block(mixer, self.bus.channels, self.audio_rate, from, end);
            writer.push_audio(&block);
            *mixed = Some(end);
        }
        Ok(writer.write_frame(frame.rgb)?)
    }
}

/// Mixes the `tracks` routed to `bus`.
fn bus_mixer(tracks: &[SourceTrack], bus: &Bus) -> Mixer {
    Mixer::new(
        tracks
            .iter()
            .filter(|t| t.bus == bus.name)
            .map(|t| t.track.clone())
            .collect(),
    )
}

/// Sample frames `from..to` of the mix at `rate`, in `channels` interleaved.
fn mix_block(mixer: &Mixer, channels: u32, rate: u32, from: usize, to: usize) -> Vec<f32> {
    let channels = channels as usize;
    let mut samples = vec![0.0; to.saturating_sub(from) * channels];
    let rate = f64::from(rate);
    mixer.render_channels(from as f64 / rate, rate, channels, &mut samples);
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Item;
    use rastersong_media::AudioClip;

    #[test]
    fn the_mix_follows_the_timeline_and_its_bus() {
        let clip = |value| {
            Arc::new(AudioClip {
                sample_rate: 4,
                channels: 1,
                samples: vec![value; 4].into(),
            })
        };
        let tracks = vec![
            SourceTrack {
                bus: "Main".into(),
                track: MixTrack {
                    clip: clip(0.5),
                    items: vec![Item::whole(0.0)],
                    gain: 1.0,
                },
            },
            SourceTrack {
                bus: "Stems".into(),
                track: MixTrack {
                    clip: clip(0.25),
                    items: vec![Item::whole(1.0)],
                    gain: 0.5,
                },
            },
        ];
        // 2 s at 2 Hz on the stereo Main bus, mixed in two blocks: only track `a` is routed
        // to it. Each item's first sample falls on its edge, where its fade in starts from
        // silence ...
        let main = Bus::main();
        let mixer = bus_mixer(&tracks, &main);
        let mixed = [mix_block(&mixer, 2, 2, 0, 1), mix_block(&mixer, 2, 2, 1, 4)].concat();
        assert_eq!(mixed, [0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        // ... and `b` plays on its own mono bus at its volume, from 1 s.
        let stems = Bus {
            name: "Stems".into(),
            channels: 1,
        };
        let stem = mix_block(&bus_mixer(&tracks, &stems), 1, 2, 0, 4);
        assert_eq!(stem, [0.0, 0.0, 0.0, 0.125]);
    }

    #[test]
    fn the_latest_frame_is_kept_scaled_down_once_taken() {
        struct Count(usize);
        impl FrameSink for Count {
            fn start(&mut self, _: &RenderInfo, _: &AudioSink) -> Result<(), EngineError> {
                Ok(())
            }
            fn frame(&mut self, _: RenderedFrame) -> Result<(), EngineError> {
                self.0 += 1;
                Ok(())
            }
        }
        let (width, height) = (960usize, 4usize);
        let info = RenderInfo {
            width: width as u32,
            height: height as u32,
            frame_rate: rastersong_media::Rational::new(30, 1),
            frames: 3,
            timebase: crate::Timebase::DEFAULT,
        };
        // Each pixel's red is its column halved, so the scaled frame's red counts up by one.
        let rgb: Vec<u8> = (0..width * height)
            .flat_map(|i| [(i % width / 2) as u8, 0, 0])
            .collect();
        let latest = LatestFrame::default();
        let mut count = Count(0);
        let mut sink = Previewed {
            sink: &mut count,
            preview: Some(&latest),
            size: (0, 0),
        };
        sink.start(&info, &AudioSink::TrackMix).unwrap();
        for index in 0..2 {
            let frame = RenderedFrame {
                index,
                rgb: &rgb,
                audio: None,
            };
            sink.frame(frame).unwrap();
        }
        // Every frame is written; the first is kept until taken, half as wide and as tall.
        assert_eq!(count.0, 2);
        let frame = latest.take().unwrap();
        assert_eq!((frame.index, frame.width, frame.height), (0, 480, 2));
        assert_eq!(&frame.rgb[..9], &[0, 0, 0, 1, 0, 0, 2, 0, 0]);
        assert_eq!(latest.take(), None);
        let mut sink = Previewed {
            sink: &mut count,
            preview: Some(&latest),
            size: (width, height),
        };
        let frame = RenderedFrame {
            index: 2,
            rgb: &rgb,
            audio: None,
        };
        sink.frame(frame).unwrap();
        assert_eq!(latest.take().map(|f| f.index), Some(2));
    }

    #[test]
    fn a_project_without_buses_exports_the_main_bus() {
        let job = ExportJob::from_project(&Project::new(
            GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap(),
        ));
        assert_eq!(job.buses(), vec![Bus::main()]);
        assert_eq!(job.bus("Main"), Some(Bus::main()));
        assert_eq!(job.bus("Nope"), None);
    }
}
