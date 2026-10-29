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

Codec patents are separate from copyright licenses. H.264 export goes through GPU and OS encoders (NVENC, AMF, QSV,
VideoToolbox, Media Foundation), which are licensed by their vendor; FFmpeg only passes frames to them. ProRes,
FFV1, AAC and PCM use FFmpeg's own LGPL encoders. The release FFmpeg builds NVENC and AMF against the vendors'
MIT-licensed headers (their notices are in `FFMPEG-BUILD.txt`); no vendor library is linked or shipped, and the
encoders load the graphics driver at runtime. AMD's notice says it grants no license to codec patents and that any
royalties due for them are the user's, so whether H.264 export through GPU encoders needs a patent license of
RasterSong's own is to be checked before a paid release (see [Open Questions](decisions.md#open-questions)).

## Fonts

The app bundles **Inter** and **JetBrains Mono** (`crates/rastersong-gui/assets/fonts/`), both under the SIL Open
Font License 1.1. It allows bundling and embedding them in any software, paid or not. What it asks, and what we
do:

- Ship the copyright notice and license text with the fonts: `Inter-LICENSE.txt` and `JetBrainsMono-LICENSE.txt`
  sit beside the font files and `cargo xtask dist` copies them into every package.
- Don't sell the fonts on their own (we don't), and don't publish a modified font under its reserved name (we use
  them unmodified).

## Other Dependencies

All Rust dependencies must have permissive licenses (MIT, Apache-2.0, BSD, Zlib, MPL-2.0 or similar).
`cargo-deny` enforces this in CI and also checks security advisories.
