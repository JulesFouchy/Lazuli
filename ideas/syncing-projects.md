---
summary: Sync a project between devices and share it with other people, through Google Drive, with the merge done here
affects: [src-tauri/src/store.rs, src-tauri/src/model.rs, src-tauri/src/library.rs, src-tauri/src/trashcan.rs, src-tauri/src/commands.rs, src/api.ts]
---

# Syncing a project, and sharing it

Two wants, not one: one person with a laptop, a desktop and eventually a phone; and several people on one project, the stated case being Coollab's journal living where its collaborators can add to it.

The constraint that settles the architecture is that **it has to work for someone who is not a developer, on a phone**. That rules out git as the built-in mechanism, and it rules out leaning on Dropbox's or Syncthing's own folder sync, because on iOS neither exposes a folder to another app — iCloud is the only folder-level option there and it is Apple-only. So the app does its own syncing against a dumb remote, and the remote is not one we run.

## Most of this is already done by the format

- Entry folders are UUIDs, so two devices adding entries cannot collide.
- Images are append-only and the entry only *points* at a chosen one, so two people adding different pictures resolves to "both survive, one is chosen" — `wip.md`'s own reconciliation rule, falling out of `unique_path` and "nothing is thrown away".
- The watcher re-reads the whole folder and diffs structurally, so **anything that writes into the folder is picked up with no frontend integration at all.** The sync engine writes files and stops there.

What is left to conflict is small: the body, `date:` and `image:` of one entry edited in two places, and the fields of `lazuli.yaml`.

## Already built

- **The trash.** Deleting moves into `.lazuli-trash/` rather than the system bin, so a delete is a change that can travel and be undone anywhere.
- **Atomic writes.** Every persisted write goes through `atomic::write`, so a second writer cannot observe half a file.
- **Identity.** `id:` in `lazuli.yaml`, minted on open. `author:` on every new entry. A profile — author uuid, name and picture — in the app's own settings, edited from the round button in the corner and *published* into each project at `authors/<uuid>/` when the user writes there, picture and all, because a collaborator can read neither our settings nor our account. A card shows a name and picture only once a project has more than one author.
- **Migration.** Every new field defaulted; `lazuli.yaml` gains a line and no `entry.md` is touched. An absent `author:` reads as the project's owner rather than being backfilled.
- **Thumbnails**, in `.lazuli-thumbs/`, mirroring each picture's path with a `.jpg` name. They sync with the project rather than being rebuilt on each machine, which is what will make a shared project readable before its photographs have arrived, and a phone viable at all. Measured at 9% of the originals on a real project.
- **Conflicts**, as a card offering both versions. Git's markers and the sidecars Syncthing and Dropbox leave are both recognised — and the engine writes its own conflicts under the same name a folder syncer would, so it came through a door that was already open.
- **The sync base**, `.lazuli/sync-state.json`, per device and never synced.
- **The engine**, `sync.rs`: a `Backend` of four operations, and a decision table over local, remote and base. Tested against an in-memory remote, including two devices converging.
- **The Google Drive backend**, `drive.rs`, and the commands and UI around it.

Each stands on its own, and each was a prerequisite.

## Still to do

**The rest of identity**, once there is a second device to try it on: the `accounts:` list that lets one recognise its own author, and the per-project `display_name:` override, the Discord model.

**A members view**, listing who a shared project is shared with and inviting by email through `permissions.create`. Drive enforces reader, writer and owner server-side, so this is a screen over an API rather than an access-control system to build.

**The Picker**, for a project somebody else shared: `drive.file` cannot enumerate a Drive, so a shared folder is handed to the app once through Google's own chooser. Until then a shared project is added the way any folder is.

**Lazy materialisation**: text first so a project's timeline is complete within a round trip, then its pictures. Today a project syncs whole, which is right for a journal of a few hundred entries and wrong for one of several thousand.

**Everything here has now been run against a real Drive**, so what is left is features rather than doubt.

**A way in on Android**, when there is an Android build. It needs its own OAuth client — Google pins a mobile one to the package name and the signing certificate, where a desktop one is anonymous and proves itself with PKCE alone — and its own redirect: `Redirect` in `drive.rs` runs a loopback web server, which a phone has neither the means nor the business to do. A custom URI scheme the OS routes back to the app is the shape. Both clients live in one Cloud project, so the consent screen, the quota and the user's approval are shared, and `DESKTOP_CLIENT_ID` gains a sibling rather than a replacement.

## The remote

**Google Drive, via its HTTP API** — no desktop client involved, which is the point, since none exists on iOS. `drive.file` scope, which needs no OAuth verification and no security assessment: the app sees what it created, plus whatever the user hands it through the Google Picker. Own projects therefore need nothing; a project someone shared is added through the Picker once, and selecting a folder grants its contents recursively.

Sharing costs nothing to build — Drive does the invitations, the accounts and the permissions, so reader/writer/owner are enforced server-side and there is no Lazuli account and no Lazuli server.

Dropbox is the natural second backend behind the same trait, and was not first: 2 GB free against Drive's 15 GB is about a year of one project at ~1.5 MB a photo, its free plan caps at three devices, and app-folder scope cannot receive a shared folder at all, so sharing would force full-account access and the review that comes with it.

Syncing is per project and opt-in; the setting is per device, in `.lazuli/sync.json`. Opening a project that is not on this machine materialises it lazily — all the text first, so the timeline is complete within a round trip, then the images, then a background pass for whatever was never scrolled to, so it ends up **fully local**. Offline is the normal case: the folder is the outbox, so there is no queue to fall out of step.

## What was turned down

- **GitHub as the remote.** Free private repos and sharing handled by the vendor, and it would give a true common ancestor. It fails on size: ~550 MB a year of photos, past GitHub's recommended 1 GB in two years, and git never forgets, so deleting old photos reclaims nothing. A cold clone on a phone downloads all of it.
- **Git built into the app.** `.lazuli-trash/` and the merge work make a project folder safe to commit, pull and push with an ordinary client, which covers the Coollab case with no git dependency and no second transport. Document it; write no code for it.
- **iroh peer-to-peer.** Closest to `wip.md`'s "décentralisée sans serveur", needs no accounts at all, and shipped 1.0 in June 2026. But both devices must be online at once, and `iroh-docs`/`iroh-blobs` are pre-1.0. The backend trait leaves room.
- **Enforcing "may edit only their own entries".** A dumb remote cannot enforce a rule about file *contents* — a modified Lazuli, or a text editor, writes what it likes. Reader, writer and owner are real because Drive enforces them; the fourth is a UI convention, and `author:` makes a breach visible afterwards. Signing would buy tamper-evidence, not prevention, since nothing but storage ACLs stops a writer deleting files. Only a server could, and that is the trade to revisit if it ever becomes a hard requirement.
- **Reading and writing straight to the remote** instead of keeping a local folder. Remote-first is not local-first: unavailable offline, a fetch per card, and it breaks "disk is the source of truth" — the store, the watcher and the diff all assume a folder.

## What is left, and why

Most of it now is. What is left is listed above, and the largest of it — the members view, the Picker, lazy materialisation — is better designed against a Drive that has actually been talked to than ahead of one. The sync base in particular is deliberately last of the file-format work: nothing reads or writes it until there is an engine, and a format in the tree that nothing uses is the thing `video-export.md` says cost more than it saved.

## A browser version, later

Talking to a storage API rather than a synced folder is what would make one possible at all: sign in and go, no install. The obstacle is that the core is Rust behind IPC, and reimplementing `dates.rs` in TypeScript is the wrong answer — a second `journal_date` will drift and misfile entries by a day. Compile the core to WASM instead; `model.rs`, `dates.rs`, `paths.rs` and most of `store.rs` are pure logic over bytes. `api.ts` is already the seam. Switching the UI to Dioxus would not help: a browser build still needs OPFS and the Drive API whatever draws it, and the desktop target is a webview too.

Two things cost nothing now and keep it open: keep the core free of Tauri and OS types, and `.lazuli-trash/`, which is already in.
