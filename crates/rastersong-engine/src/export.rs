//! Export: a project rendered from the start into a file, with progress and cancel. The CLI
//! and the GUI's export dialog both go through here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

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

    /// The render settings for `bus` at `size` (the project's for `None`), stopping after
    /// `frames` if given.
    pub fn render_settings(
        &self,
        bus: Bus,
        size: Option<(u32, u32)>,
        frames: Option<usize>,
    ) -> RenderSettings {
        RenderSettings {
            timebase: self.timeline.timebase,
            size,
            frames,
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
    /// Export only the first `frames` frames.
    pub frames: Option<usize>,
    /// The bus whose sound is written.
    pub bus: Bus,
}

/// Renders `job` into `path`. `progress` hears the frames done and the total after each frame,
/// and stops the export by returning `false`. A failed or cancelled export removes the file.
pub fn export(
    backend: &dyn MediaBackend,
    cache: Option<&AudioCache>,
    job: &ExportJob,
    settings: &ExportSettings,
    path: &Path,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<RenderInfo, EngineError> {
    let result = (|| {
        let (tracks, sources) = job.load(backend, cache)?;
        let render_settings =
            job.render_settings(settings.bus.clone(), settings.size, settings.frames);
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
            &mut sink,
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
    /// The track mix, when the graph renders no sound, and the sample frames mixed so far.
    mix: Option<(Mixer, usize)>,
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
                self.mix = Some((bus_mixer(&self.tracks, &self.bus), 0));
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
            let end = ((frame.index + 1) as f64 / self.frame_rate * f64::from(self.audio_rate))
                .round() as usize;
            let block = mix_block(mixer, self.bus.channels, self.audio_rate, *mixed, end);
            writer.push_audio(&block);
            *mixed = end;
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
    fn a_project_without_buses_exports_the_main_bus() {
        let job = ExportJob::from_project(&Project::new(
            GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap(),
        ));
        assert_eq!(job.buses(), vec![Bus::main()]);
        assert_eq!(job.bus("Main"), Some(Bus::main()));
        assert_eq!(job.bus("Nope"), None);
    }
}
