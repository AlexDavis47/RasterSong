A test build of RasterSong for early testers. The builds aren't signed, so each system asks you to
confirm the first time you open the app.

## Opening the app the first time

**Windows (64-bit):** unzip `RasterSong-…-windows-x64.zip` and run `rastersong.exe`. If Windows says
"Windows protected your PC", click **More info**, then **Run anyway**.

**macOS 11 or later (Apple Silicon and Intel):** unzip `RasterSong-…-macos-universal.zip` and move
`RasterSong.app` to Applications. The first time you open it, macOS says it can't verify the app:
click **Done**, open **System Settings → Privacy & Security**, scroll down and click **Open Anyway**.
Alternatively, run `xattr -dr com.apple.quarantine /Applications/RasterSong.app` in Terminal once.

**Linux (x86_64):** make the AppImage executable (`chmod +x RasterSong-*.AppImage`) and run it. If it
reports that FUSE is missing, run it with `--appimage-extract-and-run`.

## Licenses

RasterSong is source-available; see `LICENSE.txt` in the package.

This software uses libraries from the FFmpeg project under the LGPLv2.1. Its source code is attached
to this release (`ffmpeg-*.tar.xz`), and `FFMPEG-BUILD.txt` in each package says how it was built.
The licenses of the other libraries RasterSong uses are in `THIRD-PARTY-NOTICES.html`.
