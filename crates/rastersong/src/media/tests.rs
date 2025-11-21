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
    let media_store = MediaStore::new();
    let media_list = media_store.list_media();
    // Just verify this doesn't panic
    println!("Current media files: {:?}", media_list);
    assert!(media_list.is_empty(), "New MediaStore should be empty");
}

// Note: Actual video loading tests would require test video files
// These would be integration tests that require test assets
#[test]
fn test_load_video() {
    // Initialize FFmpeg first
    init().unwrap();

    // This test requires a test video file at this path
    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        let mut media_store = MediaStore::new();

        // Try to load the video
        let result = media_store.load_media(std::path::Path::new(test_video));
        assert!(result.is_ok(), "Failed to load test video");

        let media_id = result.unwrap();

        // Check that we can get info about it
        let media_file = media_store.get_media(&media_id);
        assert!(media_file.is_some(), "Should be able to get media file");

        let media_file = media_file.unwrap();
        let has_video = media_file.has_video();
        let has_audio = media_file.has_audio();
        let duration = media_file.duration();

        println!(
            "Video info - has_video: {}, has_audio: {}, duration: {}s",
            has_video, has_audio, duration
        );

        // Try to get video info
        if has_video {
            let video_info = media_file.video_info();
            assert!(video_info.is_some(), "Should have video info");
            let (width, height, fps) = video_info.unwrap();
            println!("Video: {}x{} @ {} fps", width, height, fps);
        }

        // Clean up
        media_store.remove_media(&media_id);
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
        let mut media_store = MediaStore::new();

        // Load the video
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();

        // Try to decode frames - now returns VideoFrame wrappers
        let result = media_store
            .get_media_mut(&media_id)
            .unwrap()
            .decode_frames(0.0, 0.1);

        match result {
            Ok(frames) => {
                assert!(!frames.is_empty(), "Should decode at least one frame");

                // Test the VideoFrame wrapper functionality
                let first_frame = &frames[0];
                let (width, height) = first_frame.dimensions();
                println!(
                    "Decoded frame: {}x{} at {:.3}s",
                    width,
                    height,
                    first_frame.timestamp()
                );
                println!("Frame data size: {} bytes", first_frame.data_size());

                // Verify we can convert to base64
                let base64 = first_frame.to_base64();
                assert!(!base64.is_empty(), "Base64 encoding should not be empty");

                // Verify serializable conversion
                let serializable = first_frame.to_serializable();
                assert_eq!(serializable.width, width);
                assert_eq!(serializable.height, height);
            }
            Err(e) => {
                println!("Failed to decode frames: {}", e);
            }
        }

        // Clean up
        media_store.remove_media(&media_id);
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
        let mut media_store = MediaStore::new();

        // Load the video
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();

        // Get video info to know the duration
        let media_file = media_store
            .get_media(&media_id)
            .expect("Should have media file");
        let (width, height, fps) = media_file.video_info().expect("Should have video info");
        let duration = media_file.duration();

        println!(
            "Video: {}x{} @ {} fps, duration: {:.2}s",
            width, height, fps, duration
        );

        // Generate 15 random timestamps between 0 and 120 seconds
        use std::collections::HashSet;
        let mut timestamps = Vec::new();
        let mut seen = HashSet::new();

        // Use a simple seeded approach for reproducibility
        let mut seed: u64 = 42;
        for _ in 0..15 {
            // Simple LCG for pseudo-randomness
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let random_float = (seed % 1000000) as f64 / 1000000.0;
            let timestamp = random_float * 120.0;

            // Ensure we don't request beyond video duration
            let timestamp = timestamp.min(duration - 0.1);

            // Avoid duplicates
            let rounded = (timestamp * 1000.0) as u64;
            if seen.insert(rounded) {
                timestamps.push(timestamp);
            }
        }

        // Sort for easier debugging
        timestamps.sort_by(|a, b| a.partial_cmp(b).unwrap());

        println!("\nTesting {} random access points:", timestamps.len());
        for (i, &ts) in timestamps.iter().enumerate() {
            println!("  {}. Requesting frame at {:.3}s", i + 1, ts);
        }

        // Decode frames at each random timestamp
        let mut decoded_count = 0;
        let mut failed_count = 0;
        let mut total_decode_time = std::time::Duration::ZERO;
        let mut decode_times = Vec::new();

        for (i, &requested_timestamp) in timestamps.iter().enumerate() {
            // Decode a small window around the requested timestamp
            let frame_duration = 1.0 / fps;
            let decode_start = (requested_timestamp - frame_duration).max(0.0);
            let decode_end = decode_start + frame_duration;

            // Time the decode operation
            let start_time = std::time::Instant::now();
            let result = media_store
                .get_media_mut(&media_id)
                .unwrap()
                .decode_frames(decode_start, decode_end);
            let decode_duration = start_time.elapsed();
            total_decode_time += decode_duration;
            decode_times.push((requested_timestamp, decode_duration));

            match result {
                Ok(frames) => {
                    if frames.is_empty() {
                        println!(
                            "  ❌ Frame {}: No frames decoded at {:.3}s",
                            i + 1,
                            requested_timestamp
                        );
                        failed_count += 1;
                        continue;
                    }

                    // Find the frame closest to requested timestamp
                    let closest_frame = frames
                        .iter()
                        .min_by(|a, b| {
                            let a_diff = (a.timestamp() - requested_timestamp).abs();
                            let b_diff = (b.timestamp() - requested_timestamp).abs();
                            a_diff.partial_cmp(&b_diff).unwrap()
                        })
                        .unwrap();

                    let actual_timestamp = closest_frame.timestamp();
                    let time_diff = (actual_timestamp - requested_timestamp).abs();

                    // Allow up to 1 frame duration of difference
                    if time_diff <= frame_duration * 1.5 {
                        println!(
                            "  ✅ Frame {}: Requested {:.3}s, got {:.3}s (diff: {:.3}s) - {}x{} - Decode time: {:.2}ms - Number of frames decoded: {}",
                            i + 1,
                            requested_timestamp,
                            actual_timestamp,
                            time_diff,
                            closest_frame.width(),
                            closest_frame.height(),
                            decode_duration.as_secs_f64() * 1000.0,
                            frames.len()
                        );
                        decoded_count += 1;
                    } else {
                        println!(
                            "  ⚠️  Frame {}: Requested {:.3}s, got {:.3}s (diff: {:.3}s) - TOO FAR - Decode time: {:.2}ms",
                            i + 1,
                            requested_timestamp,
                            actual_timestamp,
                            time_diff,
                            decode_duration.as_secs_f64() * 1000.0
                        );
                        failed_count += 1;
                    }
                }
                Err(e) => {
                    println!(
                        "  ❌ Frame {}: Failed to decode at {:.3}s - {} - Decode time: {:.2}ms",
                        i + 1,
                        requested_timestamp,
                        e,
                        decode_duration.as_secs_f64() * 1000.0
                    );
                    failed_count += 1;
                }
            }
        }

        // Calculate timing statistics
        let avg_decode_time = total_decode_time.as_secs_f64() / timestamps.len() as f64;
        let min_decode_time = decode_times
            .iter()
            .map(|(_, d)| d.as_secs_f64() * 1000.0)
            .fold(f64::INFINITY, f64::min);
        let max_decode_time = decode_times
            .iter()
            .map(|(_, d)| d.as_secs_f64() * 1000.0)
            .fold(0.0, f64::max);

        println!(
            "\nResults: {} decoded successfully, {} failed out of {} total",
            decoded_count,
            failed_count,
            timestamps.len()
        );
        println!(
            "Timing Statistics:\n  Total: {:.2}ms\n  Average: {:.2}ms\n  Min: {:.2}ms\n  Max: {:.2}ms",
            total_decode_time.as_secs_f64() * 1000.0,
            avg_decode_time * 1000.0,
            min_decode_time,
            max_decode_time
        );

        // Show timing breakdown by video position
        println!("\nTiming by video position:");
        println!("  Early (0-40s):");
        let early_times: Vec<_> = decode_times
            .iter()
            .filter(|(ts, _)| *ts < 40.0)
            .map(|(_, d)| d.as_secs_f64() * 1000.0)
            .collect();
        if !early_times.is_empty() {
            let early_avg = early_times.iter().sum::<f64>() / early_times.len() as f64;
            println!("    Count: {}, Avg: {:.2}ms", early_times.len(), early_avg);
        }

        println!("  Mid (40-80s):");
        let mid_times: Vec<_> = decode_times
            .iter()
            .filter(|(ts, _)| *ts >= 40.0 && *ts < 80.0)
            .map(|(_, d)| d.as_secs_f64() * 1000.0)
            .collect();
        if !mid_times.is_empty() {
            let mid_avg = mid_times.iter().sum::<f64>() / mid_times.len() as f64;
            println!("    Count: {}, Avg: {:.2}ms", mid_times.len(), mid_avg);
        }

        println!("  Late (80-120s):");
        let late_times: Vec<_> = decode_times
            .iter()
            .filter(|(ts, _)| *ts >= 80.0)
            .map(|(_, d)| d.as_secs_f64() * 1000.0)
            .collect();
        if !late_times.is_empty() {
            let late_avg = late_times.iter().sum::<f64>() / late_times.len() as f64;
            println!("    Count: {}, Avg: {:.2}ms", late_times.len(), late_avg);
        }

        // All frames should decode successfully
        assert_eq!(
            failed_count,
            0,
            "All random access frames should decode successfully, but {} failed out of {}.",
            failed_count,
            timestamps.len()
        );

        // Clean up
        media_store.remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_get_frame_boundaries() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        let mut media_store = MediaStore::new();

        // Load the video
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();

        // Get video info to know the FPS
        let media_file = media_store
            .get_media(&media_id)
            .expect("Should have media file");
        if let Some((width, height, fps)) = media_file.video_info() {
            println!("Testing with video: {}x{} @ {} fps", width, height, fps);

            // Test various timestamps
            let test_cases = vec![
                0.0,   // First frame
                0.5,   // Mid-video
                1.0,   // 1 second in
                1.234, // Arbitrary timestamp
            ];

            for timestamp in test_cases {
                let result = media_file.frame_boundaries(timestamp);
                assert!(result.is_some(), "Should get frame boundaries");

                let (start, end) = result.unwrap();
                println!(
                    "Timestamp {:.3}s -> Frame [{:.6}s, {:.6}s)",
                    timestamp, start, end
                );

                // Verify the frame boundaries are correct
                let frame_duration = 1.0 / fps;
                assert!(start <= timestamp, "Frame start should be <= timestamp");
                assert!(
                    end > timestamp,
                    "Frame end should be > timestamp (unless at exact boundary)"
                );
                assert!(
                    (end - start - frame_duration).abs() < 0.0001,
                    "Frame duration should match 1/fps"
                );

                // Verify start is at a frame boundary
                let frame_number = (start * fps).round();
                let expected_start = frame_number / fps;
                assert!(
                    (start - expected_start).abs() < 0.0001,
                    "Frame start should be at a frame boundary"
                );
            }
        } else {
            println!("No video stream found in test file");
        }

        // Clean up
        media_store.remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_metadata_cache_building() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        println!("\n=== Testing Metadata Cache Building ===");

        let mut media_store = MediaStore::new();

        // Load the video - this should automatically build the metadata cache
        let start_time = std::time::Instant::now();
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();
        let load_time = start_time.elapsed();

        println!(
            "Video loaded and metadata scanned in {:.2}ms",
            load_time.as_secs_f64() * 1000.0
        );

        // Get video info
        let media_file = media_store
            .get_media(&media_id)
            .expect("Should have media file");
        if let Some((width, height, fps)) = media_file.video_info() {
            let duration = media_file.duration();

            let expected_frames = (duration * fps) as usize;

            println!("Video: {}x{} @ {} fps", width, height, fps);
            println!("Duration: {:.2}s", duration);
            println!("Expected frames: ~{}", expected_frames);

            // Test that we can decode frames quickly now (cache should help)
            let decode_start = std::time::Instant::now();
            let result = media_store
                .get_media_mut(&media_id)
                .unwrap()
                .decode_frames(0.0, 0.1);
            let decode_time = decode_start.elapsed();

            assert!(result.is_ok(), "Should decode frames successfully");
            println!(
                "First decode (0.0-0.1s) took {:.2}ms",
                decode_time.as_secs_f64() * 1000.0
            );

            // Second decode in same area should potentially be faster (from cache)
            let decode_start = std::time::Instant::now();
            let result2 = media_store
                .get_media_mut(&media_id)
                .unwrap()
                .decode_frames(0.0, 0.1);
            let decode_time2 = decode_start.elapsed();

            assert!(result2.is_ok(), "Second decode should also succeed");
            println!(
                "Second decode (0.0-0.1s) took {:.2}ms",
                decode_time2.as_secs_f64() * 1000.0
            );

            if decode_time2 < decode_time {
                println!(
                    "✅ Cache improved performance: {:.1}x faster",
                    decode_time.as_secs_f64() / decode_time2.as_secs_f64()
                );
            } else {
                println!("⚠️  Cache didn't improve this decode (may still be beneficial overall)");
            }
        }

        // Clean up
        media_store.remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_gop_based_decoding() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        println!("\n=== Testing GOP-Based Decoding ===");

        let mut media_store = MediaStore::new();
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();

        // Get video info
        let media_file = media_store
            .get_media(&media_id)
            .expect("Should have media file");
        if let Some((width, height, fps)) = media_file.video_info() {
            println!("Video: {}x{} @ {} fps", width, height, fps);

            // Decode multiple ranges that might span different GOPs
            let test_ranges = vec![
                (0.0, 0.5),   // Beginning
                (5.0, 5.5),   // Middle
                (10.0, 10.5), // Later
            ];

            println!("\nDecoding {} different time ranges:", test_ranges.len());
            for (i, (start, end)) in test_ranges.iter().enumerate() {
                let decode_start = std::time::Instant::now();
                let result = media_store
                    .get_media_mut(&media_id)
                    .unwrap()
                    .decode_frames(*start, *end);
                let decode_time = decode_start.elapsed();

                match result {
                    Ok(frames) => {
                        println!(
                            "  Range {} ({:.1}s-{:.1}s): Decoded {} frames in {:.2}ms",
                            i + 1,
                            start,
                            end,
                            frames.len(),
                            decode_time.as_secs_f64() * 1000.0
                        );

                        // Verify frames are in the correct range
                        for frame in &frames {
                            let ts = frame.timestamp();
                            assert!(
                                ts >= *start - 0.1 && ts <= *end + 0.1,
                                "Frame timestamp {:.3}s should be within range [{:.1}s, {:.1}s]",
                                ts,
                                start,
                                end
                            );
                        }
                    }
                    Err(e) => {
                        println!(
                            "  Range {} ({:.1}s-{:.1}s): Failed - {}",
                            i + 1,
                            start,
                            end,
                            e
                        );
                    }
                }
            }

            // Test caching by decoding the same range again
            println!("\nTesting cache effectiveness:");
            let (start, end) = test_ranges[0];

            let decode_start = std::time::Instant::now();
            let result1 = media_store
                .get_media_mut(&media_id)
                .unwrap()
                .decode_frames(start, end);
            let time1 = decode_start.elapsed();

            let decode_start = std::time::Instant::now();
            let result2 = media_store
                .get_media_mut(&media_id)
                .unwrap()
                .decode_frames(start, end);
            let time2 = decode_start.elapsed();

            if let (Ok(frames1), Ok(frames2)) = (result1, result2) {
                println!(
                    "  First decode: {} frames in {:.2}ms",
                    frames1.len(),
                    time1.as_secs_f64() * 1000.0
                );
                println!(
                    "  Second decode: {} frames in {:.2}ms",
                    frames2.len(),
                    time2.as_secs_f64() * 1000.0
                );

                if time2 < time1 {
                    println!(
                        "  ✅ Cache speedup: {:.1}x faster",
                        time1.as_secs_f64() / time2.as_secs_f64()
                    );
                }

                assert_eq!(
                    frames1.len(),
                    frames2.len(),
                    "Both decodes should return same number of frames"
                );
            }
        }

        // Clean up
        media_store.remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_cache_performance() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        println!("\n=== Testing Cache Performance ===");

        let mut media_store = MediaStore::new();
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();

        // Decode the same frame multiple times to test cache effectiveness
        let timestamp = 5.0;
        let frame_duration = 0.033; // Assume ~30fps

        println!("Decoding frame at {:.1}s multiple times:", timestamp);

        let mut times = Vec::new();
        for i in 0..5 {
            let decode_start = std::time::Instant::now();
            let result = media_store
                .get_media_mut(&media_id)
                .unwrap()
                .decode_frames(timestamp, timestamp + frame_duration);
            let decode_time = decode_start.elapsed();
            times.push(decode_time);

            if let Ok(frames) = result {
                println!(
                    "  Decode {}: {:.2}ms ({} frames)",
                    i + 1,
                    decode_time.as_secs_f64() * 1000.0,
                    frames.len()
                );
            } else {
                println!("  Decode {}: Failed", i + 1);
            }
        }

        // First decode may be slow, subsequent should be faster
        if times.len() >= 2 {
            let first = times[0].as_secs_f64() * 1000.0;
            let avg_rest = times[1..]
                .iter()
                .map(|t| t.as_secs_f64() * 1000.0)
                .sum::<f64>()
                / (times.len() - 1) as f64;

            println!("\nSummary:");
            println!("  First decode: {:.2}ms", first);
            println!("  Avg cached decodes: {:.2}ms", avg_rest);

            if avg_rest < first {
                println!(
                    "  ✅ Cache working: {:.1}x faster on average",
                    first / avg_rest
                );
            }
        }

        // Clean up
        media_store.remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}

#[test]
fn test_seeking_accuracy() {
    // Initialize FFmpeg first
    init().unwrap();

    let test_video = "C:\\Users\\boobo\\RustroverProjects\\RasterSong\\test_assets\\test.mp4";

    if std::path::Path::new(test_video).exists() {
        println!("\n=== Testing Seeking Accuracy ===");

        let mut media_store = MediaStore::new();
        let media_id = media_store
            .load_media(std::path::Path::new(test_video))
            .unwrap();

        let media_file = media_store
            .get_media(&media_id)
            .expect("Should have media file");
        if let Some((_, _, fps)) = media_file.video_info() {
            let frame_duration = 1.0 / fps;

            // Test seeking to various timestamps
            let test_timestamps = vec![0.0, 1.0, 5.0, 10.0, 15.0, 20.0];

            println!(
                "Testing seeking accuracy (frame duration: {:.4}s):",
                frame_duration
            );

            for &ts in &test_timestamps {
                let result = media_store
                    .get_media_mut(&media_id)
                    .unwrap()
                    .decode_frames(ts, ts + frame_duration * 0.5);

                match result {
                    Ok(frames) => {
                        if let Some(frame) = frames.first() {
                            let actual_ts = frame.timestamp();
                            let error = (actual_ts - ts).abs();
                            let error_frames = error / frame_duration;

                            println!(
                                "  Seek to {:.2}s: Got {:.4}s (error: {:.4}s = {:.2} frames)",
                                ts, actual_ts, error, error_frames
                            );

                            // Should be within 1 frame
                            assert!(
                                error_frames < 1.5,
                                "Seek error should be less than 1.5 frames, got {:.2} frames",
                                error_frames
                            );
                        } else {
                            println!("  Seek to {:.2}s: No frames decoded", ts);
                        }
                    }
                    Err(e) => {
                        println!("  Seek to {:.2}s: Failed - {}", ts, e);
                    }
                }
            }
        }

        // Clean up
        media_store.remove_media(&media_id);
    } else {
        println!("Skipping test - no test video file at {}", test_video);
    }
}
