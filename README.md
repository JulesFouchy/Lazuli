# Journaley

A local-first journal for a long project. One picture and one sentence for each day you worked on it, shown as a timeline where the gaps between entries are named as plainly as the entries themselves — and exportable as a one-second-per-entry summary video.

## Local-first

A project is a folder of plain files. No database, no app-owned store, nothing to export from.

```
<project>/
  journaley.yaml            name, start date, chosen cover
  cover/                    every cover image ever added
  entries/
    <uuid>/
      entry.md              YAML frontmatter + the sentence
      IMG_4821.jpg          every image tried for this entry
      IMG_4822.jpg
```

Open the folder in an editor, a git repo, or Explorer and it still makes sense. Edit an `entry.md` by hand and the app picks the change up while you watch.

Nothing is thrown away on your behalf: every image you tried for an entry stays in its folder, the app just records which one is chosen. Deleting is always something you did, goes to the Recycle Bin, and Ctrl+Z takes it back.

## The 5am rule

A journal day runs from **05:00 to 04:59 the next morning**. Add an entry on July 11 at 01:00 and it is dated July 10 — you were still up working on the 10th. Sleep, wake, add another, and that one is July 11.

The rule decides what a new entry is dated, and what the project's own start date is. After that the date is the entry's own: it sits in `entry.md` as a plain day, and you can change it to any other day. An entry has no time — `created:` records when you wrote it, only so that two entries on the same day keep the order you wrote them in.

Clicking any date flips every date on the page between `Sep 15, 2026` and `Day 39`.

## Running it

```
npm install
npm run tauri dev
```

`journaley <folder>` opens straight into a project. New projects default into [`projects/`](projects/).

Video export needs ffmpeg: drop an `ffmpeg.exe` into `src-tauri/binaries/` to bundle it, or have one on `PATH`.

## Development

```
cargo test -j 1                 # in src-tauri/
npx tsc --noEmit
node scripts/make-fixture.mjs   # a throwaway project, in the temp folder
```

Always single-job: parallel builds exhaust the Windows page file. See [CLAUDE.md](CLAUDE.md) for the invariants worth knowing before changing anything.
