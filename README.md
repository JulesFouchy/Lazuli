![](assets/banner.png)

# Lazuli

**One picture a day, until the project is done.**

A local-first journal for a long project. One picture and one sentence for each day you worked on it, shown as a timeline where the gaps between entries are named as plainly as the entries themselves.

A one-second-per-entry summary video is planned and not yet built — see [ideas/video-export.md](ideas/video-export.md) for what it is waiting on.

## Local-first

A project is a folder of plain files. No database, no app-owned store, nothing to export from.

```
<project>/
  lazuli.yaml            id, name, start date, chosen cover
  cover/                    every cover image ever added
  entries/
    <uuid>/
      entry.md              YAML frontmatter + the sentence
      IMG_4821.jpg          every image tried for this entry
      IMG_4822.jpg
  authors/
    <uuid>/
      profile.yaml          a name and picture for whoever has written here
      face.jpg
  .lazuli-thumbs/           small copies of every picture, for the timeline
  .lazuli-trash/            what you deleted, for thirty days
  .lazuli/                  what *this machine* knows about syncing it
```

Open the folder in an editor, a git repo, or Explorer and it still makes sense. Edit an `entry.md` by hand and the app picks the change up while you watch.

An entry that comes back from a merge in two versions says so on its card, with both versions and a button under each — nothing is merged for you, and the version you do not keep goes to the trash. An entry records who wrote it, and a card shows that name and picture only once a project has more than one author — put a project somewhere two people can both write to and the timeline says who did what; keep it to yourself and nothing changes. Your own name and picture are set on the round button in the corner, and are copied into each project you write in so that whoever opens it can see them.

Nothing is thrown away on your behalf: every image you tried for an entry stays in its folder, the app just records which one is chosen. Deleting is always something you did, and Ctrl+Z takes it back — what you delete moves into `.lazuli-trash/` inside the project, where it stays for thirty days before being passed on to the Recycle Bin. It is in the folder the whole time, so you can take it back by hand, and so can the other machines the folder reaches.

## Syncing, and sharing

A project can be kept on your Google Drive, which is how it reaches your other devices and how somebody else gets to write in it. It is per project and off until you ask: **Sync…** in a project's toolbar, once an account is connected from the round button in the corner.

Lazuli talks to Drive's HTTP API. There is no Drive client to install, which is the point — no such client exists on a phone, and relying on a folder something else keeps in step is what would rule that out. Editing works offline and reconciles when there is a connection again: the folder *is* the outbox, so there is no queue to fall out of step.

Sharing is Drive's own: the Syncing dialog lists who a project is shared with, invites somebody by email as a reader or a writer, and removes them again — Google sends the invitation and enforces the roles. There is no Lazuli account and no Lazuli server.

You can be called something different in one project, the way you can on one Discord server: set it in the Syncing dialog and it is used there alone. And your devices know each other — an author record carries the accounts it signs in with, so an entry written on a phone is by the same person as one written on a laptop.

**Receiving** a shared project does not work yet. The `drive.file` scope cannot see a folder it did not create, so the person you share with has to be handed it through Google's own file chooser, which is not built — see [ideas/syncing-projects.md](ideas/syncing-projects.md).

Two devices adding entries never collide, because an entry folder is a UUID. Two people editing the same sentence is the one real conflict, and it is not merged for you — both versions land on the card and you keep one.

### The Google client id

Syncing identifies Lazuli to Google with an OAuth client id, which is in [`src-tauri/src/drive.rs`](src-tauri/src/drive.rs). A desktop client id is public by design: it is baked into every copy of the app, it says *which app is asking*, and it grants nothing on its own. What proves a sign-in genuine is the PKCE exchange, which is why a desktop app needs no client secret and this one uses none.

It is made once, for the app, not once per user. Every copy carries the same id; what belongs to each person is the token it gets back, which stays on their machine and never reaches us. Their Drive traffic does count against this project's quota, which is the one thing that is genuinely shared.

The app asks for the `drive.file` scope and nothing else. That scope is classed non-sensitive, so there is no verification, no security assessment and no "Google hasn't verified this app" screen in front of anyone. The narrowness is the trade: it can see the files Lazuli made and nothing else of yours, which is also why a project somebody *else* shared has to be handed over through the Google Picker.

Two things to get right in the Cloud console, both easy to forget:

- **The consent screen has to be in Production.** Left in Testing it works only for accounts listed there by hand, capped at a hundred — which looks exactly like "sync is broken for everyone but me".
- **One id does not cover every platform.** An Android build needs its own OAuth client, pinned to its package name and signing certificate, and iOS a third. They belong in the *same* Cloud project, so they share the consent screen, the quota and the user's approval: somebody who connected on their laptop is not asked again on their phone. The sign-in differs too — the loopback listener here is a desktop mechanism, and a phone takes the redirect through a custom URI scheme instead.

To make a fresh one, for a fork: a project at [console.cloud.google.com](https://console.cloud.google.com), the Google Drive API enabled, then Credentials → Create credentials → OAuth client ID → **Desktop app**, and the id into `DESKTOP_CLIENT_ID`.

## The 5am rule

A journal day runs from **05:00 to 04:59 the next morning**. Add an entry on July 11 at 01:00 and it is dated July 10 — you were still up working on the 10th. Sleep, wake, add another, and that one is July 11.

The rule decides what a new entry is dated, and what the project's own start date is. After that the date is the entry's own: it sits in `entry.md` as a plain day, and you can change it to any other day. An entry has no time — `created:` records when you wrote it, only so that two entries on the same day keep the order you wrote them in.

Clicking any date flips every date on the page between `Sep 15, 2026` and `Day 39`.

## Running it

```
npm install
npm run tauri dev
```

`lazuli <folder>` opens straight into a project. New projects default into [`projects/`](projects/).

## Development

```
cargo test -j 1                              # in src-tauri/
cargo clippy -j 1 --all-targets -- -D warnings
npx tsc --noEmit
node scripts/make-fixture.mjs   # a throwaway project, in the temp folder
node scripts/make-icon.mjs icon.png 1024 && npx tauri icon icon.png
node scripts/make-icon.mjs src-tauri/icons/256x256.png 256   # tauri icon skips this one
node scripts/make-banner.mjs assets/banner.png 1280 640
```

Always single-job: parallel builds exhaust the Windows page file. See [CLAUDE.md](CLAUDE.md) for the invariants worth knowing before changing anything.

**Nothing runs in CI on a push.** Those three commands are the whole check, and running them here costs nothing where a private repository's Actions minutes do not. A release runs them once on Linux before it fans out to four runners.

### Keeping releases from starting cold

Actions caches are readable only from the ref that wrote them and from the default branch. The release workflow runs on tags, so each release writes its caches onto its own tag where the next release cannot reach them. [`warm-cache.yml`](.github/workflows/warm-cache.yml) puts the same caches on `main`, where every tag run can read them — run it by hand from the Actions tab, **with `main` selected**.

Worth running after anything that changes `Cargo.lock`, when `rustc` goes up a stable release, and if more than a week has passed since the last release — GitHub deletes a cache that has not been read for seven days, and each release that restores one resets that clock.

## Releasing

Write the `## 0.2.0` section in [CHANGELOG.md](CHANGELOG.md) first — it becomes the release notes. Then:

```
node scripts/release.mjs 0.2.0
```

That is the whole thing. It refuses to start if you are not on `main`, if any of the files that hold the version has uncommitted changes, if the tag exists, or if the CHANGELOG has nothing to say; otherwise it sets the version in all three manifests, commits exactly those files, tags and pushes. Everything else uncommitted in the tree is left alone and stays out of the release. `--dry-run` prints what it would do.

Everything after that is unattended. [`.github/workflows/release.yml`](.github/workflows/release.yml) builds for Windows, macOS (Apple Silicon and Intel) and Linux, signs each installer with the updater key, uploads them to the public [`lazuli-releases`](https://github.com/JulesFouchy/lazuli-releases) repo as a draft, and publishes that draft once all four have landed. About fifteen minutes.

The draft matters: the updater reads `latest.json` from the *latest published* release, so publishing early would offer everyone an update while three of the four installers were still building.

Once it publishes, every install picks the new version up on its own: downloaded quietly in the background during a session, applied between sessions, never with a prompt. On Windows it is applied at the start of the next launch, before the window appears, because a running executable cannot be replaced and doing it at close raced with the user reopening the app; on macOS and Linux the files are swapped in place as the window closes. Either way the version changes only between sessions.

### What the repository needs, once

Three secrets on this repo, under Settings → Secrets and variables → Actions:

| Secret | What |
| --- | --- |
| `RELEASES_TOKEN` | A fine-grained PAT scoped to `lazuli-releases` with **Contents: read and write**. The workflow's own token cannot write to another repository. |
| `TAURI_SIGNING_PRIVATE_KEY` | The contents of `~/.tauri/lazuli.key`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Empty, as the key was generated without one. The secret still has to exist. |

**Back up `~/.tauri/lazuli.key` somewhere that is not this machine.** It is the only thing that can sign an update Lazuli will accept. Lose it and every existing install is stranded on its current version for good — there is no recovery, only asking each user to download and reinstall by hand. The matching public key is in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json), baked into every build, and changing it is exactly the break just described.

## Why "Lazuli"

- it's pretty (both the sonorities of the name, and the gem)
- short and memorable
- gives a good idea for the logo and overall theme / artistic direction
- reference to Obsidian, which is a software I really like, and we share some philosophy : local-first, "note taking" app
- minecraft origin: the idea for Lazuli emerged when i was coding a minecraft mod, and I added the exact same timeline inside minecraft, and enjoyed it so much i wanted to make samilar timelines for all my projects not only my minecraft world, and so I made Lazuli
- I love cakes, so I don't mind the Indonesian lapis cake

**It means layer.** In Indonesian, *lapis* is a layer; *kue lapis* is the layer cake. A journal is exactly that: days laid down one on top of the last, and readable afterwards precisely because none of them was flattened into the others. That was the idea the name was chosen for, and it was a small surprise to find it already inside the word.

**It was Lapis first.** Until enough French speakers pointed out that *lapis* lands on *la pisse*. Lazuli is the half that survives being said aloud — and it is the same stone either way, so the reasons above are the reasons this is called Lazuli. The layer is still in there: lapis lazuli keeps the whole name, and only the app dropped half of it.

**It started in Minecraft.** The first version of this timeline was a mod: a dated picture for each session in one world. It worked well enough that it became obvious every long project deserved the same thing, not just that world. Lapis lazuli is a Minecraft ore, so the name carries where it came from.

**It nods to [Obsidian](https://obsidian.md).** Another app named for a stone, and one this shares a position with: your notes are plain files in a folder you own, the app is a way of reading them, and it can be uninstalled without taking anything with it.
