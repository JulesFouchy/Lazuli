---
summary: Rejected — Lazuli syncing and sharing projects itself, through Google Drive or any service of its own
decided: 2026-09-24
affects: [src-tauri/src/commands.rs, src-tauri/src/authors.rs, src/main.ts, src/profile.ts]
---

# Syncing through Google Drive, built into the app

**Rejected, after being built.** Lazuli had its own sync: a three-way reconciliation engine (`sync.rs`) behind a four-operation `Backend`, a Google Drive backend talking to the HTTP API (`drive.rs`), OAuth with PKCE, a members view over Drive's permissions, and Google's folder chooser for receiving a shared project. It worked between one person's own devices. It was removed on 2026-09-24; the commit that removed it is the place to recover any of it.

Projects are shared the way any folder is instead — Dropbox, OneDrive, iCloud, Drive for Desktop, Syncthing, git — and the app's part is to stay correct while something else moves its files. That part stayed: atomic writes, deletions as moves into `.lazuli-trash/`, the conflict card, thumbnails that travel with the project.

## Why not

**Receiving a project somebody else shared cannot work on the scope Google grants freely.** `drive.file` is per *file*, per *user*, per *app*. Measured on 2026-09-23: a folder handed over through the chooser comes back with `canListChildren: true`, then zero children to every query — a blanket listing, an explicit `'<id>' in parents`, with and without the shared-drive parameters. It did not matter whether the files were made by hand or by the other person's Lazuli. Google's documentation says nothing either way; it was assumed in the plan and was wrong.

Everything past that wall costs more than a small app should carry:

- **Google's `drive` scope** is *restricted*: OAuth verification (~6 weeks by Google's own estimate) and a CASA security assessment by an approved lab **every year** — $675 at the cheapest lab found, more elsewhere. The consent screen then reads "see, edit, create and delete all of your Google Drive files".
- **Dropbox** has no audit, but sharing forces full-Dropbox access, and a shared folder counts against **every member's** quota: a Basic account (2 GB) cannot even accept a folder larger than its free space, so a journal a year or two old shuts out anyone not paying Dropbox €120 a year.
- **A server of our own** (object storage plus a small service for accounts and members) is cheap in money — near zero at this scale — and a permanent job in everything else: holding people's photographs under GDPR, backups, uptime, abuse, and a service that must outlive the users who depend on it.

And a folder syncer already does the job on every desktop, with no account of ours.

## What was learned, worth keeping

- **The Picker has two flows.** The web widget needs an API key, an origin, the project number and the browser's Google cookies in a `docs.google.com` frame, and when any of that is wrong it takes the choice, spins and drops it with nothing in a console the app can read. The desktop flow is the ordinary sign-in redirect plus `trigger_onepick=true&allow_folder_selection=true`, answered on the loopback. Use the second.
- **Google's token endpoint refuses a desktop exchange without the client secret**, PKCE or not.
- **A consent screen left in Testing expires refresh tokens after seven days**, which looks exactly like the app losing its sign-in.
- **A `fields` parameter is a promise about the response shape**; asking for a subset means the full struct no longer parses.

## If this comes back

Any of these would change the answer:

- A backend where sharing is part of the service and costs the recipient nothing — a server of Lazuli's own is the one that fits, and the `Backend` trait in the removed `sync.rs` is the seam it would plug into.
- Paying the yearly assessment becoming reasonable, for example because the app earns enough to carry it.
- A phone build where no folder syncer can reach the app's folder — see [../mobile-storage.md](../mobile-storage.md). That is the case the built-in sync was chosen for in the first place.
