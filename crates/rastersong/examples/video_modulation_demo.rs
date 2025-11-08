/// Demo: Video Signal Modulation
///
/// Shows how to:
/// 1. Encode video frames to audio signal
/// 2. Load a modulator audio track
/// 3. Apply audio effects (AM, FM, Ring Mod, etc.)
/// 4. Decode back to video frames
use rastersong::{EffectSettings, MappingMode, VideoSignalProcessor};

fn main() {
    println!("=== RasterSong Video Modulation Demo ===\n");

    // Create a simple test frame (8x8 gradient)
    println!("1. Creating test frame (8x8 gradient)...");
    let width = 8;
    let height = 8;
    let mut test_frame = Vec::new();

    for y in 0..height {
        for x in 0..width {
            let brightness = ((x + y) * 255 / (width + height)) as u8;
            test_frame.push(brightness); // R
            test_frame.push(brightness); // G
            test_frame.push(brightness); // B
            test_frame.push(255); // A
        }
    }

    println!(
        "   Frame size: {}x{} = {} pixels ({} bytes)",
        width,
        height,
        width * height,
        test_frame.len()
    );

    // Create processor
    println!("\n2. Creating video signal processor...");
    let mut processor = VideoSignalProcessor::new(
        width as u32,
        height as u32,
        30.0,
        MappingMode::Bugged, // Use "bugged" mode for glitchy effects
    );

    println!("   Mode: Bugged (glitchy)");
    println!("   Sample rate: {} Hz", processor.metadata().sample_rate);

    // Create a simple modulator (sine-wave-like pattern)
    println!("\n3. Creating modulator signal (simple wave)...");
    let modulator_samples = 200;
    let modulator: Vec<f32> = (0..modulator_samples)
        .map(|i| {
            let phase = (i as f32 / modulator_samples as f32) * std::f32::consts::PI * 2.0;
            phase.sin() // Normalized float (-1.0 to +1.0)
        })
        .collect();

    processor.load_modulator(modulator);
    println!("   Modulator length: {} samples", modulator_samples);

    // Test different effects
    println!("\n4. Testing different effects:\n");

    // Original (no effects)
    println!("   a) Original (no effects)");
    let effects_none = EffectSettings::default();
    let result_none = processor
        .process_frame(&test_frame, &effects_none)
        .expect("Failed to process frame");
    let avg_brightness_none = calculate_average_brightness(&result_none);
    println!("      Average brightness: {:.1}", avg_brightness_none);

    // Amplitude Modulation
    println!("\n   b) Amplitude Modulation (AM)");
    let mut effects_am = EffectSettings::default();
    effects_am.am_enabled = true;
    effects_am.am_depth = 0.8;
    let result_am = processor
        .process_frame(&test_frame, &effects_am)
        .expect("Failed to process frame");
    let avg_brightness_am = calculate_average_brightness(&result_am);
    println!("      Depth: {}", effects_am.am_depth);
    println!(
        "      Average brightness: {:.1} (Δ{:+.1})",
        avg_brightness_am,
        avg_brightness_am - avg_brightness_none
    );

    // Ring Modulation
    println!("\n   c) Ring Modulation");
    let mut effects_ring = EffectSettings::default();
    effects_ring.ring_mod_enabled = true;
    effects_ring.ring_mod_mix = 0.7;
    let result_ring = processor
        .process_frame(&test_frame, &effects_ring)
        .expect("Failed to process frame");
    let avg_brightness_ring = calculate_average_brightness(&result_ring);
    println!("      Mix: {}", effects_ring.ring_mod_mix);
    println!(
        "      Average brightness: {:.1} (Δ{:+.1})",
        avg_brightness_ring,
        avg_brightness_ring - avg_brightness_none
    );

    // Frequency Modulation
    println!("\n   d) Frequency Modulation (FM)");
    let mut effects_fm = EffectSettings::default();
    effects_fm.fm_enabled = true;
    effects_fm.fm_depth = 15.0;
    let result_fm = processor
        .process_frame(&test_frame, &effects_fm)
        .expect("Failed to process frame");
    let avg_brightness_fm = calculate_average_brightness(&result_fm);
    println!("      Depth: {}", effects_fm.fm_depth);
    println!(
        "      Average brightness: {:.1} (Δ{:+.1})",
        avg_brightness_fm,
        avg_brightness_fm - avg_brightness_none
    );

    // Combined effects
    println!("\n   e) Combined (AM + Ring Mod + Distortion)");
    let mut effects_combo = EffectSettings::default();
    effects_combo.am_enabled = true;
    effects_combo.am_depth = 0.5;
    effects_combo.ring_mod_enabled = true;
    effects_combo.ring_mod_mix = 0.3;
    effects_combo.distortion_enabled = true;
    effects_combo.distortion_amount = 0.4;
    let result_combo = processor
        .process_frame(&test_frame, &effects_combo)
        .expect("Failed to process frame");
    let avg_brightness_combo = calculate_average_brightness(&result_combo);
    println!(
        "      Average brightness: {:.1} (Δ{:+.1})",
        avg_brightness_combo,
        avg_brightness_combo - avg_brightness_none
    );

    println!("\n=== Demo Complete! ===");
    println!("\nNext steps:");
    println!("  • Load real video frames from VideoDecoder");
    println!("  • Load real audio from files");
    println!("  • Render modified frames back to video");
    println!("  • Create real-time preview in GUI");
}

fn calculate_average_brightness(frame_rgba: &[u8]) -> f32 {
    let mut sum = 0u32;
    let mut count = 0u32;

    // Average RGB values (skip alpha)
    for chunk in frame_rgba.chunks_exact(4) {
        sum += chunk[0] as u32 + chunk[1] as u32 + chunk[2] as u32;
        count += 3;
    }

    if count > 0 {
        sum as f32 / count as f32
    } else {
        0.0
    }
}
