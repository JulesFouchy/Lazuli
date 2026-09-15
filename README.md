![](assets/banner.png)

# Lapis

**One picture a day, until the project is done.**

A local-first journal for a long project. One picture and one sentence for each day you worked on it, shown as a timeline where the gaps between entries are named as plainly as the entries themselves.

A one-second-per-entry summary video is written and working, but not reachable from the UI yet — see [ideas/ship-video-export.md](ideas/ship-video-export.md) for what it is waiting on.

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

## Development

```
cargo test -j 1                 # in src-tauri/
npx tsc --noEmit
node scripts/make-fixture.mjs   # a throwaway project, in the temp folder
node scripts/make-icon.mjs icon.png 1024 && npx tauri icon icon.png
node scripts/make-icon.mjs src-tauri/icons/256x256.png 256   # tauri icon skips this one
node scripts/make-banner.mjs assets/banner.png 1280 640
```

Always single-job: parallel builds exhaust the Windows page file. See [CLAUDE.md](CLAUDE.md) for the invariants worth knowing before changing anything.

## Why "Lapis"

- it's pretty (both the sonorities of the name, and the gem)
- short and memorable
- gives a good idea for the logo and overall theme / artistic direction
- reference to Obsidian, which is a software I really like, and we share some philosophy : local-first, "note taking" app
- minecraft origin: the idea for Lapis emerged when i was coding a minecraft mod, and I added the exact same timeline inside minecraft, and enjoyed it so much i wanted to make samilar timelines for all my projects not only my minecraft world, and so I made Lapis
- I love cakes, so I don't mind the Indonesian lapis cake

**It means layer**: in Indonesian, *lapis* is a layer; *kue lapis* is the layer cake. A journal is exactly that: days laid down one on top of the last, and readable afterwards precisely because none of them was flattened into the others. That was the idea the name was chosen for, and it was a small surprise to find it already inside the word.

**It started in Minecraft.** The first version of this timeline was a mod: a dated picture for each session in one world. It worked well enough that it became obvious every long project deserved the same thing, not just that world. Lapis lazuli is a Minecraft ore, so the name carries where it came from.

**It nods to [Obsidian](https://obsidian.md).** Another app named for a stone, and one this shares a position with: your notes are plain files in a folder you own, the app is a way of reading them, and it can be uninstalled without taking anything with it.
