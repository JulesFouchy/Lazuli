# Changelog

What changed in each release, for the person using Lapis rather than the person building it.

Each version's section becomes the release notes verbatim, so write it before cutting the release — `scripts/release.mjs` refuses a version that has no section here, or one whose heading still says "unreleased". Headings are `## <version> — <date>`.

## 0.1.1 — 2026-09-16

- **F11** puts Lapis fullscreen, and takes it back out. It works from anywhere — the timeline, an open entry, the image viewer.

## 0.1.0 — 2026-09-16

First release.

- A project is a folder on disk: `lapis.yaml`, a `cover/` folder, and one folder per entry holding an `entry.md` and every image tried for it. Editing any of it by hand works, and the app follows along while you watch.
- A timeline of entries, where the gaps between them are named as plainly as the entries themselves. Click any date to flip the whole page between real dates and day numbers.
- A journal day runs 05:00 → 04:59, so an entry written after midnight belongs to the evening it came from.
- Nothing is deleted or overwritten on your behalf. Deletes go to the Recycle Bin and Ctrl+Z takes them back; a filename clash keeps both files.
- Light and dark, a choice of background, and a choice of accent.
- Lapis keeps itself up to date. It looks for a new version shortly after starting, downloads it quietly if there is one, and installs it as you close the app — so the next time you open Lapis, it is the new one. You are never asked and never interrupted, and nothing else ever leaves your machine.
