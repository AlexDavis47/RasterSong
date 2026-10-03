//! Guards on the FFmpeg build we link against. These protect the licensing model: RasterSong may
//! only ever ship an LGPL FFmpeg.

#[test]
fn initializes() {
    rastersong_media::init().unwrap();
    rastersong_media::init().unwrap();
}

#[test]
fn loaded_ffmpeg_is_lgpl() {
    let info = rastersong_media::backend_info();
    for lib in &info.libraries {
        assert!(
            lib.license.starts_with("LGPL"),
            "{} is licensed as {:?}",
            lib.name,
            lib.license
        );
    }
    assert!(
        !info.configuration.contains("--enable-gpl"),
        "GPL build: {}",
        info.configuration
    );
    assert!(
        !info.configuration.contains("--enable-nonfree"),
        "non-free build: {}",
        info.configuration
    );
    assert!(info.is_lgpl());
}

#[test]
fn loaded_libraries_match_bindings() {
    for lib in rastersong_media::backend_info().libraries {
        assert_eq!(
            lib.version.major, lib.compiled_major,
            "{} {} was loaded but the bindings were built for major version {}",
            lib.name, lib.version, lib.compiled_major
        );
    }
}
