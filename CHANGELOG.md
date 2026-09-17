# Changelog

What changed in each release, for the person using Lazuli rather than the person building it.

Each version's section becomes the release notes verbatim, so write it before cutting the release — `scripts/release.mjs` refuses a version that has no section here, or one whose heading still says "unreleased". Headings are `## <version> — <date>`.

## unreleased

- **Take the picture with your camera, from inside the entry.** "Take a photo…" sits under the images in the entry editor and in the cover picker: press it and the preview takes the grid's place, press Take photo and the shot is saved into the entry and chosen. The preview is a mirror, like a mirror is, and the picture you keep is the one you saw. A machine with more than one camera gets a button to switch between them.
- **Your projects stay in the order you put them in, and you drag them into it.** The launch screen no longer reshuffles itself: opening a project leaves it exactly where it is, and dragging a row up or down the list is what moves it. The list is no longer capped at twelve either — nothing falls off the bottom because you added a thirteenth.
- **Tabs, for filing projects into groups of your own.** The list starts under one tab called Projects; hover the strip above it and a `+` appears to add more — Wip and Done, say, or one per part of your life. Drag a project onto a tab to file it there. Right-click a tab to rename or delete it, and drag it along the strip to reorder. Deleting a tab keeps every project in it, handing them back to the first tab, and Undo puts the tab back as it was. Nothing on disk moves: a tab is filing and nothing else. With a single tab the screen looks exactly as it did.

## 0.3.0 — 2026-09-16

- **Notes and project names take Markdown.** `**bold**`, `*italic*`, `` `code` ``, `~~struck~~`, `#` headings and `-` or `1.` lists, on the timeline and in the viewer. Nested lists, links, tables and quotes are not in: a day's note is a sentence and sometimes a short list.
- **Every field that holds Markdown shows it working as you type it** — the note, the project's name in its banner, and the name in the New project dialog. The `*` and the `#` stay where you put them, dimmed, and the text they mark is already bold, italic or a heading: the way a Markdown file looks in an editor rather than a preview beside one. A card and the viewer show it finished, markers gone. The name in the banner does both: formatted until you click into it, the Markdown itself while you are in it. Double-clicking a word selects the word and not the markers around it.
- **Which end of the timeline a project opens on is now the project's own.** Newest first or oldest first is stored beside the project rather than on the machine, so a hundred-day challenge can be read from day one while a work journal opens on what happened last, and each stays as you left it. Everything reads newest first to begin with, which is what every project showed before.
- **Ctrl+; shows the spelling suggestions for the word you are in**, without reaching for the right mouse button.
- **A project's folder is named after what its name reads as**, not after how it is written: a project called `**Test** Test` lives in `Test Test`. Renaming it follows the folder the same way, when the folder was named after the project to begin with. Folders you named yourself are still left where they are.
- **Clicking a card opens its editor. Clicking its picture opens the viewer.** Anywhere else on the card — the date's row, the sentence, the space around the picture — is the editor now, which is the half of a card you came back to change. The pencil is gone, and so are Ctrl+click and Shift+click, which were shortcuts to something a plain click does.
- **Pasting a copied path adds the image it points at.** Explorer's "Copy as path" puts text on the clipboard and no file, so it used to paste the path into the note.
- **The light theme's blue is deeper, and the accent is brighter.** If you have chosen your own background or accent, yours is kept.
- **The custom colour in Appearance stays yours.** It keeps what you mixed when you step over to a preset to compare the two, it comes back the next time you open the app, and clicking it both puts it back in use and opens the picker on it. One swatch is ringed at a time, even when what you mixed is a colour the palette also has.
- **Typing a project name survives the app saving.** A rescan landing mid-word used to take the field away, along with whatever was in it and any Windows emoji picker open over it.
- Going forward to a project you have since deleted does nothing, rather than reporting an error about a folder you threw away yourself.
- Leaving a project for the Recent list stays on the Recent list. Editing a file in that project's folder from outside the app used to put the project back on screen a moment later.
- **The video export is gone for now, and will come back.** It could not be reached from the UI and had never worked end to end; the version that returns will be rendered by a tool of its own. Nothing in your projects changes — an entry was always a date, a sentence and a picture, which is all a frame needs.
- **The title bar comes down whenever you reach the top edge.** On a window that opened maximised it often did not come at all, and where it did it stopped beside the scrollbar instead of passing over it. The strip that summons it is wider, so it no longer competes with the pixels Windows keeps for resizing, and the bar itself is the three buttons and nothing else — the name and the icon were repeating what the window already says.
- Bold is properly bold and italic is a real italic. The app bundled two weights of its typeface and no italic at all, so `**bold**` in a project's name — already a heading, already heavy — changed nothing, and `*italic*` was an upright letter the browser sheared. It now bundles the whole weight range, and the drawn italic alongside it.

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
