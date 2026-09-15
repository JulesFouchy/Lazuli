---
summary: Video export is built but hidden, because there is no decision yet on how ffmpeg reaches the user
affects: [src/main.ts, src/export-dialog.ts, src/video.ts, src-tauri/src/video.rs, src-tauri/tauri.conf.json]
---

# Ship the video export

The whole export path works: `export-dialog.ts` collects the options, `video.ts` draws each entry on a canvas and streams PNGs at `video.rs`, which pipes them into an ffmpeg it finds on `PATH` or beside the executable. What is missing is any answer to *where that ffmpeg comes from on a machine that has never heard of ffmpeg*, and that is a question about the installer, not about the code.

So the "Export video…" button is not rendered. Everything behind it is still compiled and still registered as commands — only the way in is gone.

## Why not yet

The feature has not been tested end to end, and shipping it as-is means most users click a button that tells them to go install something. Either it works out of the box or it should not be visible.

## The options, none chosen

- **Bundle ffmpeg as a Tauri sidecar.** Works offline and always the right build, but adds ~80 MB to an installer that is otherwise ~10 MB, and drags in ffmpeg's licence terms — a GPL build makes the whole distribution GPL, so it would have to be an LGPL build with the source offer that implies.
- **Download it on first export.** Keeps the installer small and the licensing at arm's length, but needs a trusted URL, a hash check, somewhere to put it, and a story for offline users.
- **Encode in-process instead.** No ffmpeg at all — `WebCodecs` can produce an H.264 stream from the same canvas frames, and WebView2 has it. Removes the whole problem, at the cost of rewriting the encoder and trusting the container muxing to a library.

## Re-enabling

Put the toolbar button back in `timelineSection` in `src/main.ts` (the comment there marks the spot); the `export` route, the dialog and the commands are all still wired.
