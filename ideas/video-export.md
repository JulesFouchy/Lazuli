---
summary: The summary video comes back, rendered by a tool of the owner's own rather than by a canvas and ffmpeg
---

# Video export

One frame per entry, held a second each: the project as a short film. This is a wanted feature and not a dropped one.

## Why there is no code for it

There was, and it has been deleted — `src/video.ts`, `src/export-dialog.ts`, `src-tauri/src/video.rs`, and the five `export_*` commands. It drew each frame on a `<canvas>` and piped PNGs into an ffmpeg found on `PATH`.

Both halves of that are being replaced rather than fixed:

- **The renderer.** The owner is building a custom rendering tool, and the frames will come from that. A canvas has no layout engine, which is what kept the entry card and the frame from being able to show the same things — the moment a card gained Markdown, a heading or a list could be shown on the page and never in the export.
- **The encoder.** Shipping ffmpeg was never answered: bundling it adds ~80 MB to a ~10 MB installer and drags in its licence terms, and downloading it on first use needs a trusted URL, a hash check and a story for offline users.

Keeping a hidden, unreachable implementation of both in the tree cost more than it saved. It was never run end to end — the one time it was, it turned out every frame would have failed on a tainted canvas — and it constrained what a card was allowed to contain in exchange for nothing a user could see.

## What the app still owes it

Nothing in the entry format: an entry is a date, a sentence and a picture, which is everything a frame needs. Two things to know when it returns:

- **`accentOnDark` in `theme.ts` was written for this** — the accent lightened until it reads on a dark scrim — and is now unused. It is left in place because it is the answer to a question the new renderer will ask again.
- **The day number is what carries the gaps.** See `rejected/video-gap-cards.md`: entries run back to back with nothing between them, and `Day 12` following `Day 4` shows the pause on its own.
