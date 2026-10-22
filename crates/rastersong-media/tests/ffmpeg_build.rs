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
fn the_license_link_names_the_loaded_lgpl_version() {
    assert_eq!(
        rastersong_media::lgpl_url("LGPL version 2.1 or later"),
        "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html"
    );
    assert_eq!(
        rastersong_media::lgpl_url("LGPL version 3 or later"),
        "https://www.gnu.org/licenses/lgpl-3.0.html"
    );
    let info = rastersong_media::backend_info();
    let expected = if info.configuration.contains("--enable-version3") {
        "https://www.gnu.org/licenses/lgpl-3.0.html"
    } else {
        "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html"
    };
    assert_eq!(info.license_url(), expected, "{}", info.license());
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
