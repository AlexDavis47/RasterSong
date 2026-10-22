# Licensing

> This document reflects our understanding of the licenses involved and is not legal advice.
> Have the RasterSong license and FFmpeg compliance reviewed before any paid release.

## RasterSong License

RasterSong is **source-available**, not open source. The code is public, so it can be read, studied, and built for
personal use, and the project stays attributable to its author. Redistribution and commercial use are not permitted
without permission. This keeps the option of selling prebuilt binaries later (similar to Aseprite's model).
The terms are in [LICENSE](../LICENSE). It is a custom license and should be reviewed (see
[open questions](decisions.md#open-questions)).

Because the code isn't open source, outside contributions will require a contributor license agreement (CLA).

## FFmpeg

- Use an **LGPL** build of FFmpeg only. Never configure with `--enable-gpl` (this rules out x264/x265) or `--enable-nonfree`.
- **Dynamically link** FFmpeg and ship its shared libraries alongside the app. Static linking under the LGPL would require providing relinkable object files.
- LGPL obligations we meet:
  - Ship FFmpeg as separate, replaceable shared libraries
  - Credit FFmpeg (the About dialog) and ship the LGPL license text with the libraries (`FFmpeg-LICENSE.txt` in
    every package, and in the installer once there is one)
  - Publish the exact FFmpeg source version and configure line used for each release
  - Don't restrict users from replacing the FFmpeg libraries or reverse engineering for that purpose
- Users never download or install FFmpeg themselves. Downloading FFmpeg at runtime was considered and rejected: it doesn't help with licensing and hurts reliability (offline use, firewalls, antivirus, version drift).

## Codec Patents

Codec patents are separate from copyright licenses. Today the only encoder used is FFmpeg's own lossless FFV1 (with
PCM audio), in the CLI. H.264 export is planned to go through OS and hardware encoders, which are licensed by the
platform vendor, and ProRes through FFmpeg's own LGPL encoder.

## Other Dependencies

All Rust dependencies must have permissive licenses (MIT, Apache-2.0, BSD, Zlib, MPL-2.0 or similar).
`cargo-deny` enforces this in CI and also checks security advisories.
