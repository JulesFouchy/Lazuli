# Journaley

Local-first project journal: a project is a folder on disk, each entry is a dated sentence plus a picture, viewable as a timeline or exported as a one-second-per-entry summary video.

## Rules

- **`wip.md` is the owner's idea dump and todo list. Read it for context, never edit it.**
- **Deferred ideas go in [`ideas/`](ideas/), one markdown file each** — never as a TODO comment. Before changing a file, `rg "<that file>" ideas/` says what is already planned for it; see [ideas/README.md](ideas/README.md) for the shape. Delete the file when the idea ships or is dropped.
- When editing files with a script, write bytes or pass `newline=""` / `encoding="utf-8"`. Python's text mode silently turns every `
` into `
` on Windows, which rewrites the whole file and buries a one-line change in a full-file diff. `.gitattributes` normalises the repo to LF.
- Never run build or test tools in parallel. `cargo test -j 1`, `cargo build -j 1`, `npm run` scripts one at a time. Parallel jobs exhaust the Windows page file and corrupt crate metadata.

## Architecture

- **Disk is the source of truth.** There is no in-memory authority. The project folder is re-read and diffed on every filesystem event; the app's own writes produce an identical scan and therefore emit nothing. Never patch in-memory state from a watcher event.
- **Every date goes through `journal_date` in `dates.rs`.** A journal day runs 05:00 → 04:59 local, so an entry written at 01:00 belongs to the previous day. Labels, gap connectors, same-day grouping, new-entry defaults and `start_date` all use it. Never take a calendar date off a timestamp directly.
- **Never convert to UTC before extracting a date.** `created` carries a local offset and that offset is load-bearing — an entry keeps its original local meaning when read on a machine in another timezone.
- **Nothing is ever overwritten or deleted without the user asking.** Filename clashes go through `unique_path` and keep both files. Deletions go to the Recycle Bin via the `trash` crate, never `fs::remove_file`.
- **Video frames are drawn on a canvas**, which has no layout engine. Anything added to an entry card must be reproducible there, or it will silently vanish from exports.
