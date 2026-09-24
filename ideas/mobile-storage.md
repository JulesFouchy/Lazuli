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

## Why not yet

There is no mobile build. And it is unchecked how much of the folder picking, the bookmarks and the `content://` access Tauri 2's mobile plugins already cover — that is the first thing to find out, and it decides whether this is a configuration job or a storage abstraction.

## How

Pick the folder once, at first launch, and keep the grant. Make that folder the phone's projects directory: new projects go in it, and every project already in it — arrived through the syncer — is on the launch screen without being added. Keep the thumbnails-first order the timeline already has, since a provider that downloads on demand makes a photograph the slow part.
