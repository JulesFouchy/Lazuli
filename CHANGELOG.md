# Changelog

What changed in each release, for the person using Lazuli rather than the person building it.

Each version's section becomes the release notes verbatim, so write it before cutting the release — `scripts/release.mjs` refuses a version that has no section here, or one whose heading still says "unreleased". Headings are `## <version> — <date>`.

## 0.5.0 — unreleased

- **What you delete stays in the project, and Undo works on a Mac.** Deleting an entry, an image or a whole project used to hand it to the system Recycle Bin, which macOS will not let anything read back — so on a Mac the app could only tell you to go and find it in the Trash yourself. Deletions now move into a `.lazuli-trash/` folder instead: inside the project for an entry or an image, and beside your projects for a whole one. Undo works the same way on all three platforms, what you deleted is sitting in the folder where you can see it, and after thirty days it is passed on to the Recycle Bin — Lazuli still throws nothing away itself.
- Deleting no longer fails because something else has the file open. The move is a rename now, which nothing can refuse the way the Recycle Bin could.
- **Entries record who wrote them, and say so once more than one person has.** A project kept somewhere two people can both write to — a shared folder, or a repository — now shows a name and a small round picture beside the date on each entry. A journal you keep by yourself looks exactly as it did: they appear only when a project has more than one author. Entries written before this are left as they are rather than being claimed for anyone.
- **A round button in the corner is you.** It shows your picture, or a drawn figure until you choose one, and opens a small dialog where you set your name and picture. Both are yours rather than any project's — they are copied into each project you write in, so whoever else opens it sees who wrote what, and nothing of yours has to leave your machine for them to. Your name starts as your account name here.
- Every project is given an id of its own the first time you open it, so that the same journal on two machines is recognisably one project rather than two. Your entries are not touched; only `lazuli.yaml` gains a line.
- **A project can be kept on your Google Drive**, which is how it reaches your other devices and how somebody else gets to write in it. Per project and off until you ask for it: **Sync…** in a project's toolbar connects the account and turns the project on, and the account is in the round button in the corner too, since it is yours rather than any one project's. There is no Lazuli account and no Lazuli server — sharing is Drive's own, so Google does the invitation and the permissions. Editing works offline and reconciles when there is a connection again. Two devices adding entries never collide; two people editing the same sentence is not merged for you, and both versions land on the card for you to keep one.
- **Invite people to a synced project, and see who is in it.** The Syncing dialog lists everyone the project's folder is shared with and what they may do, takes an email address to invite somebody as a reader or a writer, and removes them again. They need a Google account and nothing of Lazuli's. Google enforces the roles, which is why they are worth trusting.
- **A project somebody shared with you opens in Lazuli.** **Add shared…** on the launch screen opens Google's own folder chooser in your browser; choose the folder and it is an ordinary project from then on, entries they add later included. A project Lazuli syncs is kept in a `Lazuli` folder in your Drive and named `Lazuli | <project>` — tidier, and it is how you tell it apart from everything else anyone has ever shared with you. The owner's Syncing dialog has that name ready to copy, to send along with the invitation.
- **Your Google sign-in, name and picture no longer vanish together.** When `settings.json` could not be read for a moment — another copy of Lazuli writing it, a save mid-rename — the app took that for a blank file, and the next thing it saved wrote the blankness back: sign-in gone, name and picture gone, and a new identity minted on top so your own entries stopped being yours. Settings that cannot be read are now left exactly as they are; at worst the one change you were making is not saved, and you make it again.
- **Your name can differ in one project.** Set it in the Syncing dialog and it is used there and nowhere else; leave it empty and your own name shows, and renaming yourself later reaches every project you have not overridden.
- **Your other devices know you are you.** An entry you write on a laptop and one you write on a phone are by the same person, rather than by two people who happen to share a name.
- A project that is still arriving shows its timeline before its photographs: the words come first, and the pictures fill in behind them.
- **An entry that came back from a merge in two versions says so, instead of disappearing.** Conflict markers in an `entry.md` made the file unreadable, and Lazuli quietly left that entry off the timeline — nothing was lost on disk, and nothing told you. Such an entry now sits on its own day with both versions side by side and a button under each; the one you do not keep goes to the project's trash rather than away. The second copy a folder syncer leaves beside an entry, rather than touching it, is offered the same way. Nothing is ever merged for you: two versions of a sentence is a question only you can answer.
- **A long timeline of real photographs scrolls properly.** Cards and the image picker were decoding full-resolution pictures to draw them at a fraction of the size. Lazuli now keeps a small copy of each picture beside the project, in `.lazuli-thumbs/`, and shows that instead — about a tenth of the bytes on a real project. They are built in the background the first time you open a project and as each new picture is added, and a card falls back to the full-size picture until its copy is there. The viewer and the entry editor still show the original, because that is the one you opened them to look at.

## 0.4.0 — 2026-09-22

- **Take the picture with your camera, from inside the entry.** "Take a photo…" sits under the images in the entry editor and in the cover picker: press it and the preview takes the grid's place, press Take photo and the shot is saved into the entry and chosen. The preview is a mirror, like a mirror is, and the picture you keep is the one you saw. A machine with more than one camera gets a button to switch between them.
- **Your projects stay in the order you put them in, and you drag them into it.** The launch screen no longer reshuffles itself: opening a project leaves it exactly where it is, and dragging a row up or down the list is what moves it. The list is no longer capped at twelve either — nothing falls off the bottom because you added a thirteenth.
- **Tabs, for filing projects into groups of your own.** The list starts under one tab called Projects; hover the strip above it and a `+` appears to add more — Wip and Done, say, or one per part of your life. Drag a project onto a tab to file it there. Right-click a tab to rename or delete it, and drag it along the strip to reorder. Deleting a tab keeps every project in it, handing them back to the first tab, and Undo puts the tab back as it was. Nothing on disk moves: a tab is filing and nothing else. With a single tab the screen looks exactly as it did.
- A picture with transparency in it no longer shows the timeline through itself in the viewer. It is laid on black now, and only where the picture is — the rest of the window stays the dimmed page it was.

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
