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
- **The Google Drive backend**, `drive.rs`, and the commands and UI around it. Run against a real account: sign-in, folders, listing, uploading, downloading, trashing, and two folders reconciling until they held the same bytes.
- **Identity across devices.** An author record carries the accounts its owner signs in with, so a second device looks itself up and joins the author it already is rather than minting a rival. Per-project `display_name`, the Discord model: your own name everywhere until you decide otherwise, and then only there.
- **Sharing**, as a members view over Drive's own permissions — invite by email, see who has what, take it away. The roles are real because Google enforces them, which is why there is no access control here to write.
- **Words before pictures.** A pass carries text first, so a timeline is complete within a round trip and the photographs fill in behind it.
- **The Picker**, through Google's *desktop* flow: the sign-in redirect with `trigger_onepick=true` and `allow_folder_selection=true`, answered on the loopback the app already listens on. It works — the choice comes back with a folder id and fresh tokens — and it is not enough; see "The scope is the wall" below. The web flow was built first and removed: it needs an API key, a fixed origin, the project number and the browser's Google cookies in a `docs.google.com` frame, and when any of that is missing it takes the choice, spins, and silently drops it.
- **Telling one shared folder from another**, which is why project folders are named `Lazuli | <project>`. A folder shared with you is in no folder of yours — it stays in the sharer's Drive and reaches you through "Shared with me", a view rather than a place — and no chooser can look inside one to recognise a `lazuli.yaml`. The name is all there is, so the name is made to carry it and the sharer's dialog hands them the exact string.

Each stands on its own, and each was a prerequisite.

## Still to do

**A way in on Android**, when there is an Android build. Its own OAuth client, pinned to the package name and signing certificate, and its own redirect: `Redirect` in `drive.rs` runs a loopback web server, which a phone has neither the means nor the business to do. A custom URI scheme the OS routes back to the app is the shape. Both clients live in one Cloud project, so the consent screen, the quota and the user's approval are shared, and `DESKTOP_CLIENT_ID` gains a sibling rather than a replacement.

**Dropbox**, behind the same trait, if a second backend is ever wanted.

## The remote

**Google Drive, via its HTTP API** — no desktop client involved, which is the point, since none exists on iOS. `drive.file` scope, which needs no OAuth verification and no security assessment: the app sees what it created, plus whatever the user hands it through the Google Picker, **one file at a time**. Own projects therefore need nothing. A project someone shared is a different matter — the paragraph that used to end here said a picked folder grants its contents recursively, and that was wrong.

## The scope is the wall

Measured on 2026-09-23, twice: a folder handed over through the Picker comes back with `canListChildren: true` and **zero children to every query** — a blanket listing, an explicit `'<id>' in parents`, with and without the shared-drive parameters. Every file inside fails `drive.file`'s per-file, per-user check. It makes no difference whether the files were made by hand or by another person's Lazuli: the grant is to *(this app, this user, this file)*, and "this user" is the part a shared project can never satisfy from the other side. Symmetric, too — what the receiver's Lazuli would write into the folder (`canAddChildren` is true) the owner's Lazuli cannot read back.

So under `drive.file` a live shared project is impossible, not merely hard: it would need every participant to re-pick every new file. The one scope that lets an app read what another person's copy of it wrote is a scope that reads more than the app's own files, and Google classes every one of those *restricted* — `drive`, `drive.readonly`, `drive.metadata.readonly` alike. Restricted means OAuth verification (a privacy policy, a homepage, a demo video, a brand review) and a **CASA Tier 2 security assessment, repeated annually**; in 2026 Tier 2 can be met with a self-scan by an approved tool, so the cost is hours a year rather than money, but it is a recurring obligation on a one-person app.

What that scope would buy is exactly what was asked for and cannot otherwise be had: an invitation by email, and the project **appearing on the receiver's launch screen by itself** — `files.list` with `sharedWithMe = true and name = 'lazuli.yaml'` finds every project anyone has shared, no chooser, nothing to paste. The trade the user sees is the consent screen: "see, edit, create and delete all of your Google Drive files" instead of "the files you use with this app".

The alternatives that stay non-restricted are the ones the plan already holds: a project kept in a **git repository**, which `.lazuli-trash/`, the merge work and the conflict cards were built to make safe, and which is the right answer for the Coollab case anyway; or a second backend whose full-access review is a form rather than an audit (Dropbox), at the price of a second account for every collaborator. **Which way to go is the owner's call and is open.** Until it is made, *Add shared…* and *Invite* are in the app and do not deliver a project.

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

Most of it now is. What is left is listed above, and the largest of it — the members view, the Picker, lazy materialisation — was better designed against a Drive that has actually been talked to than ahead of one. The sync base in particular is deliberately last of the file-format work: nothing reads or writes it until there is an engine, and a format in the tree that nothing uses is the thing `video-export.md` says cost more than it saved.

## A browser version, later

Talking to a storage API rather than a synced folder is what would make one possible at all: sign in and go, no install. The obstacle is that the core is Rust behind IPC, and reimplementing `dates.rs` in TypeScript is the wrong answer — a second `journal_date` will drift and misfile entries by a day. Compile the core to WASM instead; `model.rs`, `dates.rs`, `paths.rs` and most of `store.rs` are pure logic over bytes. `api.ts` is already the seam. Switching the UI to Dioxus would not help: a browser build still needs OPFS and the Drive API whatever draws it, and the desktop target is a webview too.

Two things cost nothing now and keep it open: keep the core free of Tauri and OS types, and `.lazuli-trash/`, which is already in.
