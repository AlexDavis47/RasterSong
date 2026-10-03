use std::process::Command;

#[test]
fn info_reports_lgpl_ffmpeg() {
    let output = Command::new(env!("CARGO_BIN_EXE_rastersong-cli"))
        .arg("info")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("LGPL"), "{stdout}");
    assert!(stdout.contains("avcodec"), "{stdout}");
}
