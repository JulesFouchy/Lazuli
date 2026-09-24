---
summary: On a phone a project has to live in a folder the user picks, not in the app's private storage, so that a syncer can reach it
affects: [src-tauri/src/store.rs, src-tauri/src/watch.rs, src-tauri/src/commands.rs]
---

# Where a project lives on a phone

Lazuli does not sync; whatever holds the folder does. On a desktop that is any folder at all. On a phone the app's own storage is private to it, and nothing else — no syncer, no Files app — can reach inside, so a project kept there is a project on that phone and nowhere else. The project has to live in a folder the **user picks**, somewhere a syncer also writes.

## iOS

Since iOS 13 an app can ask for a *folder* through the document picker and keep a security-scoped bookmark to it. That covers iCloud Drive, and Dropbox, Google Drive and OneDrive through their Files-app providers. Once access is started, the folder is a real path, so `std::fs` and most of the core work unchanged.

- iCloud Drive is the reliable one. Third-party providers can hand over placeholders for files not yet downloaded, which reads as an empty or missing picture until something asks for the contents; coordinated reads (`NSFileCoordinator`) are how a file is made to arrive.
- The watcher is the open question: whether `notify` sees changes a provider writes, or whether it has to be `NSFilePresenter` or polling.

## Android

Since Android 11 an app cannot read files that another app wrote into shared storage, except through the Storage Access Framework: the user picks a folder and the app gets a `content://` tree, **not a path**. `std::fs`, `atomic::write`'s rename and `notify` all assume a path, so the store would need a layer that can sit on a document tree, and the watcher would poll.

Syncers that fill such a folder exist (Syncthing-Fork, FolderSync, Autosync for Drive and Dropbox), but choosing and configuring one is a technical step for somebody who only wanted a journal.

## Tauri covers none of the picking

Checked against `tauri-plugin-dialog` 2.7.3: there is **no folder picker on either mobile platform**. No `ACTION_OPEN_DOCUMENT_TREE` anywhere in its Kotlin, no `directory` handling in `src/mobile.rs`, and the iOS picker copies what was picked into caches rather than handing back a scoped URL. So "pick the folder once and keep the grant" is a plugin to write, not a flag to pass.

On Android two third-party plugins already do it — `tauri-plugin-scoped-storage` and `tauri-plugin-android-fs`, both persisting the tree grant (512 of them on API 30+). Try those before writing one. For iOS security-scoped bookmarks nothing was found, so that is Swift of our own.

Which settles the other question: this is a storage abstraction, not a configuration job. It also inverts the platform order — iOS hands back a real path and leaves `std::fs`, `atomic::write`, `unique_path`, `trashcan` and `thumbs` working untouched, where Android hands back a document tree and leaves none of them working. Android is first because that is where the testers are, not because it fits.

## Why not yet

There is no mobile build.

What is worth knowing before committing to this is how expensive a scan is over SAF, because the app re-reads the folder on every change. A directory listing there is not a syscall but a round trip into another process, and if the folder is served by a cloud provider rather than local storage, potentially a network call. Measure a real project on a device — local storage and Google Drive's provider both — before deciding whether everything can live in the tree. The fallback if it cannot is to keep the text and thumbnails on a real path and leave only the originals in the tree, fetched as they are shown.

## How

Pick the folder once, at first launch, and keep the grant. Make that folder the phone's projects directory: new projects go in it, and every project already in it — arrived through the syncer — is on the launch screen without being added. Keep the thumbnails-first order the timeline already has, since a provider that downloads on demand makes a photograph the slow part.
