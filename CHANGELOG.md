# Changelog

What changed in each release, for the person using Lapis rather than the person building it. The release notes on each published version are this file's entry for it.

## 0.1.0 — unreleased

First release.

- A project is a folder on disk: `lapis.yaml`, a `cover/` folder, and one folder per entry holding an `entry.md` and every image tried for it. Editing any of it by hand works, and the app follows along while you watch.
- A timeline of entries, where the gaps between them are named as plainly as the entries themselves. Click any date to flip the whole page between real dates and day numbers.
- A journal day runs 05:00 → 04:59, so an entry written after midnight belongs to the evening it came from.
- Nothing is deleted or overwritten on your behalf. Deletes go to the Recycle Bin and Ctrl+Z takes them back; a filename clash keeps both files.
- Light and dark, a choice of background, and a choice of accent.
- Updates: Lapis checks once at startup whether a newer version exists, and offers it. Nothing else leaves the machine.
