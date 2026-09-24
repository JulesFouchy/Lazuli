---
summary: Stream a project's entries in batches, newest first, so a huge project shows its first cards before the scan is done
affects: [src-tauri/src/store.rs, src-tauri/src/commands.rs, src/main.ts, src/timeline.ts]
---

# Progressive entry loading

`open_project` returns the whole project in one reply, and the timeline is built from it in one go. A project of thousands of entries would show nothing until the last `entry.md` was parsed and the last card built.

## Measured, on 2000 entries

The estimate here used to be that the cost becomes visible around 2000 entries. It was measured on exactly that, a fixture of 2000 entries each with a picture, and the number that matters turns out not to be the entry count:

| | |
| --- | --- |
| open, files read before | **~0.7s** |
| open, files never read on this boot | **~15s** |
| rescan, nothing changed | ~0.1s, no files read at all |

The same folder, the same build, minutes apart. What separates the two is whether the operating system has the files in hand — a first read pays the disk and whatever scans a newly written file on its way past, and there are three of them per entry.

So the trigger is not size, it is **the first open of a project whose files this machine has not read yet**: freshly cloned, freshly synced, or the first open after a boot. That is also exactly what a folder syncer produces, and it is the moment the user is most likely to be watching a spinner wondering whether anything happened.

Which reframes the value. Streaming does not make the 15 seconds shorter; it makes the first cards appear in a fraction of it and the rest fill in behind them. At 0.7s it would change nothing anybody notices.

## Why not yet

There is no project of this size yet — 2000 entries is five and a half years of daily journalling. The second event channel, the partial-project state and the ordering heuristic below are real complexity, and the scan already runs off the main thread so a slow open no longer freezes the page.

What would move this up: the phone. A project folder on Android is reached through the Storage Access Framework, where a directory listing is a round trip into another process rather than a syscall, and a first open pays `N` of them with nothing on screen — no cache to tier against and no recursive listing to ask for instead. Measure a real project on a device before building this, and size the batches against that number rather than against a desktop one. See [mobile-storage.md](mobile-storage.md).

## How

- `open_project` returns metadata and cover images at once, with an empty entry list and an open-generation id.
- A `project-entries` event then delivers batches of about 100 entries, tagged with that id so a batch from a superseded open is dropped.
- Read entry folders in **folder-mtime-descending** order as the cheap stand-in for newest-first: the day lives inside the file, but the stat is already paid in `list_entry_dirs`, and an entry folder is usually last touched when it was made. Not exact (adding an image to an old entry bumps it), and does not need to be.
- The frontend holds the partial project, re-renders per batch, and keeps the spinner up until the final batch says it is complete.
- The watcher's diff baseline (`OpenProject::snapshot`) is set only once the full scan is in, so a rescan racing the stream cannot emit a half-read project as a change.

## When

A real project whose first open is slow enough to wonder about — on a phone, most likely, or on a desktop project that has grown past a couple of thousand entries. `node scripts/make-fixture.mjs <folder> 2000` builds something the right size to test against, and **be warned it is 4.5 GB**; delete it afterwards.
