use anyhow::Result;
use crate::audio_player::AudioPlayer;
use crate::video_decoder::{VideoDecoder, VideoFrame};

/// Coordinates synchronized playback of video and audio
pub struct MediaPlayer {
    video: VideoDecoder,
    audio: AudioPlayer,
}

impl MediaPlayer {
    pub fn new(media_path: &str) -> Result<Self> {
        // For now, use the same file for both video and audio
        // In the future, you could support separate files or apply effects
        let video = VideoDecoder::new(media_path)?;
        let audio = AudioPlayer::new(media_path)?;
        
        Ok(Self { video, audio })
    }
    
    pub fn play(&self) -> Result<()> {
        self.video.play()?;
        self.audio.play()?;
        Ok(())
    }
    
    pub fn pause(&self) -> Result<()> {
        self.video.pause()?;
        self.audio.pause()?;
        Ok(())
    }
    
    pub fn seek(&self, position_ns: u64) -> Result<()> {
        self.video.seek(position_ns)?;
        self.audio.seek(position_ns)?;
        Ok(())
    }
    
    pub fn get_position(&self) -> Option<u64> {
        // Use video position as the primary timeline
        self.video.get_position()
    }
    
    pub fn get_duration(&self) -> Option<u64> {
        // Use video duration as the primary duration
        self.video.get_duration()
    }
    
    pub fn get_current_frame(&self) -> Option<VideoFrame> {
        self.video.get_current_frame()
    }
}

