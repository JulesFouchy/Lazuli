# Lapis

**One picture a day, until the project is done.**

A local-first journal for a long project. One picture and one sentence for each day you worked on it, shown as a timeline where the gaps between entries are named as plainly as the entries themselves — and exportable as a one-second-per-entry summary video.

## Local-first

A project is a folder of plain files. No database, no app-owned store, nothing to export from.

```
<project>/
  lapis.yaml            name, start date, chosen cover
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

`lapis <folder>` opens straight into a project. New projects default into [`projects/`](projects/).

Video export needs ffmpeg: have one on `PATH`, or put an `ffmpeg.exe` next to the installed app.

## Development

```
cargo test -j 1                 # in src-tauri/
npx tsc --noEmit
node scripts/make-fixture.mjs   # a throwaway project, in the temp folder
node scripts/make-icon.mjs icon.png 1024 && npx tauri icon icon.png
```

Always single-job: parallel builds exhaust the Windows page file. See [CLAUDE.md](CLAUDE.md) for the invariants worth knowing before changing anything.

## Why "Lapis"

Because the word turned out to mean everything the app is already doing.

**It is a stone, and stones keep a record.** *Lapis* is Latin for stone. Lapis lazuli is the deep blue one, shot through with flecks of gold — which is where the app's colours come from, ground and accent, and where its icon comes from too.

**It also means layer.** In Indonesian, *lapis* is a layer; *kue lapis* is the layer cake. A journal is exactly that: days laid down one on top of the last, and readable afterwards precisely because none of them was flattened into the others. That was the idea the name was chosen for, and it was a small surprise to find it already inside the word.

**Ground lapis is ultramarine** — the blue of illuminated manuscripts, once the most expensive pigment there was, saved for the page you wanted looked at. An app for arranging pictures so they tell a story has no business being named after anything else.

**It started in Minecraft.** The first version of this timeline was a mod: a dated picture for each session in one world. It worked well enough that it became obvious every long project deserved the same thing, not just that world. Lapis lazuli is a Minecraft ore, so the name carries where it came from.

**It nods to [Obsidian](https://obsidian.md).** Another app named for a stone, and one this shares a position with: your notes are plain files in a folder you own, the app is a way of reading them, and it can be uninstalled without taking anything with it.

And it is two syllables that sound the same in French and in English, which the earlier candidates — *Rupestre*, *Fresque* — were not.
