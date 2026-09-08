---
summary: Cache downscaled images so a long timeline does not decode full-size photos
affects: [src/timeline.ts, src/image-picker.ts, src-tauri/src/store.rs]
---

# Thumbnail cache

Timeline cards and picker thumbnails point straight at the original files. A card is at most ~440px tall and a picker thumbnail ~104px square, but the browser decodes a full-resolution photo for each. This is the likeliest thing to hurt first at scale — before the DOM count does.

`loading="lazy"` already stops offscreen images being decoded, so the cost only shows up while scrolling. That is exactly when it is most noticeable.

## How

Generate downscaled copies into a `.journaley-cache/` folder inside the project, keyed by source path and mtime. Gitignored, safe to delete at any time, and regenerated on demand — which keeps it consistent with everything being files on disk. Serve those to the timeline and the picker; the entry editor's chosen image and the video export keep using the originals.

Purely additive: nothing else has to change, and deleting the folder just makes it slow again rather than losing anything.

## When

When scrolling a project of real photos stutters. The fixture generates small synthetic gradients, so it will not show this — test with a folder of actual camera images.
