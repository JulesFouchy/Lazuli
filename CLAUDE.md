# Journaley

Local-first project journal: a project is a folder on disk, each entry is a dated sentence plus a picture, viewable as a timeline or exported as a one-second-per-entry summary video.

## Rules

- **`wip.md` is the owner's idea dump and todo list. Read it for context, never edit it.**
- **Never hard-wrap markdown.** One line per paragraph and per bullet; editors soft-wrap. A hard-wrapped paragraph turns a three-word edit into a reflowed block in the diff.
- **Deferred ideas go in [`ideas/`](ideas/), one markdown file each** — never as a TODO comment. Before changing a file, `rg "<that file>" ideas/` says what is already planned for it; see [ideas/README.md](ideas/README.md) for the shape. Delete the file when the idea ships or is dropped.
- **Edit files with the Edit tool, not with a script that reads and rewrites them.** Python's text mode converts every newline to CRLF on Windows, and a mis-escaped replacement string silently corrupts content; both have happened here. If a scripted edit is genuinely necessary, read and write bytes, and diff the result before committing. `.gitattributes` and `.editorconfig` pin the repo to LF and UTF-8, so a stray CRLF can no longer reach a commit, but neither protects file content.
- Never run build or test tools in parallel. `cargo test -j 1`, `cargo build -j 1`, `npm run` scripts one at a time. Parallel jobs exhaust the Windows page file and corrupt crate metadata.

## Architecture

- **Disk is the source of truth.** There is no in-memory authority. The project folder is re-read and diffed on every filesystem event; the app's own writes produce an identical scan and therefore emit nothing. Never patch in-memory state from a watcher event.
- **An entry records a day, never a time.** `entry.md` carries a `date:`, which is the whole of the entry's date and the only part the user edits. Its `created:` stamp says when the entry was written: it orders entries that share a day, dates the entries written before `date:` existed, and never reaches the frontend — an entry with a readable time is an entry that will end up displaying one.
- **Every date derived from a timestamp goes through `journal_date` in `dates.rs`.** A journal day runs 05:00 → 04:59 local, so an entry written at 01:00 belongs to the previous day. That governs a new entry's default date, `start_date`, and reading an entry that has no `date:` of its own. Never take a calendar date off a timestamp directly.
- **Never convert to UTC before extracting a date.** `created` carries a local offset and that offset is load-bearing — an entry keeps its original local meaning when read on a machine in another timezone.
- **Nothing is ever overwritten or deleted without the user asking.** Filename clashes go through `unique_path` and keep both files. Deletions go to the Recycle Bin via the `trash` crate, never `fs::remove_file`.
- **Video frames are drawn on a canvas**, which has no layout engine. Anything added to an entry card must be reproducible there, or it will silently vanish from exports.
