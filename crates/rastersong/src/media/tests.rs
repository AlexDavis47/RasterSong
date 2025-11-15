//! Integration tests for the media module

#[cfg(test)]
mod video_tests {
    use super::super::*;
    use std::path::Path;

    #[test]
    fn test_decode_video_frame_0() {
        // Initialize GStreamer
        init().unwrap();

        // Register test video
        let video_path = Path::new("../../test_assets/test.mp4");
        if !video_path.exists() {
            eprintln!("Test video not found, skipping test");
            return;
        }

        let id = register_video_file(video_path).unwrap();

        // Decode frame 0
        let sample = decode_video(id, 0).unwrap();

        // Verify we got a sample
        assert!(sample.buffer().is_some());
        assert!(sample.caps().is_some());

        // Verify buffer has data
        let buffer = sample.buffer().unwrap();
        assert!(buffer.size() > 0);

        // Verify caps contain video metadata
        let caps = sample.caps().unwrap();
        let structure = caps.structure(0).unwrap();
        assert!(structure.get::<i32>("width").is_ok());
        assert!(structure.get::<i32>("height").is_ok());
    }

    #[test]
    fn test_decode_video_frame_metadata() {
        init().unwrap();

        let video_path = Path::new("../../test_assets/test.mp4");
        if !video_path.exists() {
            eprintln!("Test video not found, skipping test");
            return;
        }

        let id = register_video_file(video_path).unwrap();
        let sample = decode_video(id, 0).unwrap();

        // Extract and verify metadata
        let caps = sample.caps().unwrap();
        let structure = caps.structure(0).unwrap();

        let width: i32 = structure.get("width").unwrap();
        let height: i32 = structure.get("height").unwrap();

        assert!(width > 0);
        assert!(height > 0);

        // Verify format is RGB
        let format: String = structure.get("format").unwrap();
        assert_eq!(format, "RGB");
    }

    #[test]
    fn test_decode_video_multiple_frames() {
        init().unwrap();

        let video_path = Path::new("../../test_assets/test.mp4");
        if !video_path.exists() {
            eprintln!("Test video not found, skipping test");
            return;
        }

        let id = register_video_file(video_path).unwrap();

        // Decode frame 0, 1, and 2
        let sample0 = decode_video(id, 0).unwrap();
        let sample1 = decode_video(id, 1).unwrap();
        let sample2 = decode_video(id, 2).unwrap();

        // All should have buffers
        assert!(sample0.buffer().is_some());
        assert!(sample1.buffer().is_some());
        assert!(sample2.buffer().is_some());

        // Buffers should have the same size (same resolution)
        let size0 = sample0.buffer().unwrap().size();
        let size1 = sample1.buffer().unwrap().size();
        let size2 = sample2.buffer().unwrap().size();

        assert_eq!(size0, size1);
        assert_eq!(size1, size2);
    }
}

#[cfg(test)]
mod audio_tests {
    use super::super::*;
    use std::path::Path;

    #[test]
    fn test_decode_audio_samples() {
        init().unwrap();

        let audio_path = Path::new("../../test_assets/test_modulator.wav");
        if !audio_path.exists() {
            eprintln!("Test audio not found, skipping test");
            return;
        }

        let id = register_audio_file(audio_path).unwrap();

        // Decode first 1000 samples (samples 0-1000)
        let sample = decode_audio(id, 0, 1000).unwrap();

        // Verify we got a sample
        assert!(sample.buffer().is_some());
        assert!(sample.caps().is_some());

        // Verify buffer has data
        let buffer = sample.buffer().unwrap();
        assert!(buffer.size() > 0);

        // Verify caps contain audio metadata
        let caps = sample.caps().unwrap();
        let structure = caps.structure(0).unwrap();
        assert!(structure.get::<i32>("rate").is_ok());
        assert!(structure.get::<i32>("channels").is_ok());
    }

    #[test]
    fn test_decode_audio_metadata() {
        init().unwrap();

        let audio_path = Path::new("../../test_assets/test_modulator.wav");
        if !audio_path.exists() {
            eprintln!("Test audio not found, skipping test");
            return;
        }

        let id = register_audio_file(audio_path).unwrap();
        let sample = decode_audio(id, 0, 1000).unwrap();

        // Extract and verify metadata
        let caps = sample.caps().unwrap();
        let structure = caps.structure(0).unwrap();

        let rate: i32 = structure.get("rate").unwrap();
        let channels: i32 = structure.get("channels").unwrap();
        let format: String = structure.get("format").unwrap();

        assert!(rate > 0);
        assert!(channels > 0);
        assert_eq!(format, "F32LE");
    }

    #[test]
    fn test_decode_audio_sample_range() {
        init().unwrap();

        let audio_path = Path::new("../../test_assets/test_modulator.wav");
        if !audio_path.exists() {
            eprintln!("Test audio not found, skipping test");
            return;
        }

        let id = register_audio_file(audio_path).unwrap();

        // Decode different sample ranges
        let sample1 = decode_audio(id, 0, 1000).unwrap();
        let sample2 = decode_audio(id, 1000, 2000).unwrap();
        let sample3 = decode_audio(id, 5000, 6000).unwrap();

        // All should have buffers
        assert!(sample1.buffer().is_some());
        assert!(sample2.buffer().is_some());
        assert!(sample3.buffer().is_some());

        // Verify buffers contain f32 samples (4 bytes per sample)
        let buffer1 = sample1.buffer().unwrap();
        let buffer2 = sample2.buffer().unwrap();
        let buffer3 = sample3.buffer().unwrap();

        // Each buffer should have at least some samples
        assert!(buffer1.size() >= 4); // At least 1 f32 sample
        assert!(buffer2.size() >= 4);
        assert!(buffer3.size() >= 4);

        // Verify we can read the samples as f32
        let map1 = buffer1.map_readable().unwrap();
        let data1 = map1.as_slice();
        assert!(
            data1.len() % 4 == 0,
            "Buffer size should be multiple of 4 (f32)"
        );
    }

    #[test]
    fn test_decode_audio_invalid_range() {
        init().unwrap();

        let audio_path = Path::new("../../test_assets/test_modulator.wav");
        if !audio_path.exists() {
            eprintln!("Test audio not found, skipping test");
            return;
        }

        let id = register_audio_file(audio_path).unwrap();

        // sample_start >= sample_end should fail
        let result = decode_audio(id, 1000, 500);
        assert!(result.is_err());
    }
}
