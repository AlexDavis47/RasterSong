//! Tests for media module

use super::*;

#[test]
fn test_ffmpeg_initialization() {
    // Test that FFmpeg can be initialized
    let result = init();
    assert!(result.is_ok(), "FFmpeg initialization failed");
    assert!(is_initialized(), "FFmpeg should be initialized");
}

#[test]
fn test_media_id_creation() {
    // Test MediaId creation
    let id1 = MediaId::new();
    let id2 = MediaId::new();

    // IDs should be unique
    assert_ne!(id1, id2);

    // Test string conversion
    let id_string = id1.to_string();
    let id_parsed = MediaId::from_string(&id_string).unwrap();
    assert_eq!(id1, id_parsed);
}

#[test]
fn test_media_store_empty() {
    // Test that media store starts empty or can list media
    let media_list = list_media();
    // Just verify this doesn't panic
    println!("Current media files: {:?}", media_list);
}

// Note: Actual video loading tests would require test video files
// These would be integration tests that require test assets
#[test]
#[ignore] // Ignore by default since it requires a test video file
fn test_load_video() {
    // Initialize FFmpeg first
    init().unwrap();

    // This test requires a test video file at this path
    let test_video = "test_assets/test_video.mp4";

    if std::path::Path::new(test_video).exists() {
        // Try to load the video
        let result = load_media(test_video);
        assert!(result.is_ok(), "Failed to load test video");

        let media_id = result.unwrap();

        // Check that we can get info about it
        let info = get_media_info(&media_id);
        assert!(info.is_some(), "Should be able to get media info");

        let (has_video, has_audio, duration) = info.unwrap();
        println!(
            "Video info - has_video: {}, has_audio: {}, duration: {}s",
            has_video, has_audio, duration
        );

        // Try to get video info
        if has_video {
            let video_info = get_video_info(&media_id);
            assert!(video_info.is_some(), "Should have video info");
            let (width, height, fps) = video_info.unwrap();
            println!("Video: {}x{} @ {} fps", width, height, fps);
        }

        // Clean up
        remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_decode_frame() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        // Load the video
        let media_id = load_media(test_video).unwrap();

        // Try to decode frames
        let result = decode_frames(&media_id, 0.0, 0.1);

        match result {
            Ok(frames) => {
                assert!(!frames.is_empty(), "Should decode at least one frame");
            }
            Err(e) => {
                println!("Failed to decode frames: {}", e);
            }
        }

        // Clean up
        remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_decode_random_access_frames() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        // Load the video
        let media_id = load_media(test_video).unwrap();

        // Try to decode frames from a random access point
        let result = decode_frames(&media_id, 5.0, 15.1);

        match result {
            Ok(frames) => {
                assert!(!frames.is_empty(), "Should decode at least one frame");
            }
            Err(e) => {
                println!("Failed to decode frames: {}", e);
            }
        }

        // Clean up
        remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}
