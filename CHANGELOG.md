# Changelog

What changed in each release, for the person using Lazuli rather than the person building it.

Each version's section becomes the release notes verbatim, so write it before cutting the release — `scripts/release.mjs` refuses a version that has no section here, or one whose heading still says "unreleased". Headings are `## <version> — <date>`.

## 0.3.0 — unreleased

- **Notes and project names take Markdown.** `**bold**`, `*italic*`, `` `code` `` and `~~struck~~`, on the timeline, in the viewer and in the exported video. Nothing else: a frame of the video is drawn on a canvas with no layout engine, so headings and lists could be shown on the page but never in the export, and the export is meant to be what you saw. The project name shows its formatting until you click into it, and the Markdown itself while you are editing it.
- **Clicking a card's sentence opens the editor**, where clicking its picture still opens the viewer. The pencil and Ctrl+click are unchanged.
- **Pasting a copied path adds the image it points at.** Explorer's "Copy as path" puts text on the clipboard and no file, so it used to paste the path into the note.
- **The light theme's blue is deeper, and the accent is brighter.** If you have chosen your own background or accent, yours is kept.
- **The colour well in Appearance keeps what you mixed** when you step over to a preset to compare the two.
- **Typing a project name survives the app saving.** A rescan landing mid-word used to take the field away, along with whatever was in it and any Windows emoji picker open over it.
- Going forward to a project you have since deleted does nothing, rather than reporting an error about a folder you threw away yourself.

## 0.2.0 — 2026-09-16

**Lapis is now Lazuli.** The same app and the same stone — *lapis* just sounds like something unfortunate in French.

Three things to know, all one-time:

- **Uninstall the old Lapis yourself.** Windows treats Lazuli as a separate app, so it installs alongside rather than over, and you will see both in Add/Remove Programs. Removing Lapis there takes nothing with it — your projects are folders on your own disk and were never inside the app.
- **Your theme, accent and background go back to their defaults**, because the window's stored settings move with the app's name. Set them again in Appearance.
- **Each project's `lapis.yaml` becomes `lazuli.yaml`** the first time you open it. Folders from even older versions still work too; nothing is lost, and nothing needs doing.

## 0.1.2 — 2026-09-16

- Updates no longer close Lapis. Before, a new version was installed as you closed the app, and if you opened Lapis again in the few seconds that took, the installer shut it straight back down. Now the download waits on disk and is applied at the start of your next launch, before the window appears — that one launch takes a moment longer, and nothing else changes.
- Lapis draws its own title bar. It stays hidden until the pointer reaches the top edge, then slides down over the page — so at rest it costs no height, and going straight to the top-right corner still lands on Close.
- Pasting an image file copied from Explorer imports it. It kept failing because the paste listener sat on an element that never had focus.
- The colour wells in Appearance no longer close the OS picker the moment you start dragging in it.
- Error messages stay on screen until you click them, instead of vanishing while you read.
- "Open folder…" is now "Add project…": it puts the folder at the top of Recent and leaves you on the launch screen, rather than opening it.

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
