---
summary: Opening a project leaves an unreleased handle on its folder, so it cannot be moved to the Recycle Bin until the app restarts
affects: [src-tauri/src/commands.rs, src-tauri/src/store.rs, src-tauri/src/watch.rs]
---

# The project folder handle leak

Once the app has opened a project, that project's folder can no longer be moved — not by the app, not by Explorer, not by `Rename-Item`. Windows answers `Access is denied`, and `trash::delete` reports it as the unhelpful `Unknown { description: "Some operations were aborted" }`. Restarting the app clears it.

This is why **Delete on a project you have opened in this session fails**, and why it looks intermittent: delete one you have not opened and it works.

## Reproducing it

```
open_project(P)      // through the UI or __TAURI_INTERNALS__.invoke
close_project()
```
then, from PowerShell, `Rename-Item <P> <P>_x` — denied. It stays denied for as long as the process lives; a 60-second poll never saw it released.

A project with **no entries** is unaffected: created, opened, closed and trashed in about 1.2s every time. Every project with at least one entry that has been opened is affected.

## What it is not

Each of these was ruled out by disabling it and reproducing anyway:

- **The filesystem watcher.** Built with `JOURNALEY_NO_WATCH` skipping `watch_project` entirely: still leaks. Worth knowing anyway — `Debouncer`'s `Drop` only raises a stop flag and never joins its thread, so `close` now calls `unwatch` and `stop` explicitly. That is a real fix for a different, smaller problem.
- **The asset protocol scope.** Built with the `allow_directory` call skipped: still leaks.
- **A failed `trash::delete` leaving handles of its own.** Opening and closing without ever attempting a delete leaves the folder just as stuck.

That leaves `ProjectStore::scan`, which is the only remaining thing `open_at` does — though every `read_dir` in it is collected into a `Vec` and every file read goes through `fs::read_to_string`, so nothing there obviously holds a directory open.

## Where to look next

- Which handle it actually is. `handle64.exe -p journaley.exe` (Sysinternals) or Process Explorer's Find Handle would name it in a minute and turn this from a hunt into a fix. Not installed on this machine, which is why the search above was by elimination.
- Whether it is the *root* that is held or a descendant: renaming `entries/` and `cover/` inside a stuck project both succeed, which points at the root, but renaming a directory does not prove much about its children.
- `is_project` is the only other thing touching the root, and it is one `is_file()` call.

## Why it is not just worked around

`trash_with_retry` already retries for two seconds, which covers a folder something is briefly holding. It cannot help here: the handle is never released. The error message now says the folder is open in another program and to close it, which is true and actionable — but the program is us.
