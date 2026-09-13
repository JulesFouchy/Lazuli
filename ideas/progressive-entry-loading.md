---
summary: Stream a project's entries in batches, newest first, so a huge project shows its first cards before the scan is done
affects: [src-tauri/src/store.rs, src-tauri/src/commands.rs, src/main.ts, src/timeline.ts]
---

# Progressive entry loading

`open_project` returns the whole project in one reply, and the timeline is built from it in one go. A project of thousands of entries would show nothing until the last `entry.md` was parsed and the last card built.

## Why not yet

At 81 entries the full scan plus the DOM build land in one frame; streaming would change nothing visible. The cost is estimated (not measured) to become visible around 2000 entries in a debug build. Until a project that size exists, the second event channel, the partial-project state, and the ordering heuristic below are complexity with no payoff. The scan already runs off the main thread, so a slow open no longer freezes the page; the spinner covers the wait.

## How

- `open_project` returns metadata and cover images at once, with an empty entry list and an open-generation id.
- A `project-entries` event then delivers batches of about 100 entries, tagged with that id so a batch from a superseded open is dropped.
- Read entry folders in **folder-mtime-descending** order as the cheap stand-in for newest-first: the day lives inside the file, but the stat is already paid in `list_entry_dirs`, and an entry folder is usually last touched when it was made. Not exact (adding an image to an old entry bumps it), and does not need to be.
- The frontend holds the partial project, re-renders per batch, and keeps the spinner up until the final batch says it is complete.
- The watcher's diff baseline (`OpenProject::snapshot`) is set only once the full scan is in, so a rescan racing the stream cannot emit a half-read project as a change.

## When

Opening a fixture from `node scripts/make-fixture.mjs <folder> 2000` in a debug build visibly waits with the spinner up.
