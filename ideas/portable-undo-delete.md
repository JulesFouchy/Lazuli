---
summary: Undo-delete uses the system trash, which cannot be read back on macOS — a trash folder inside the project would work everywhere
affects: [src-tauri/src/commands.rs]
---

# Undo that works on every platform

Deleting sends the file to the system trash with `trash::delete`, and undoing reads it back with `trash::os_limited::list` + `restore_all`. That second half only exists on Windows and Linux. macOS has no API for reading its Trash back — Finder's "Put Back" works off a private file that nothing else is allowed to touch — so `restore` on macOS tells the user to do it themselves in Finder.

That is a real hole in a promise the README makes plainly: *"Deleting is always something you did, goes to the Recycle Bin, and Ctrl+Z takes it back."* On macOS the first two hold and the third does not.

## How

Stop using the system trash for anything inside a project. Move deleted entries and images to a `.lapis-trash/` folder inside the project instead, keeping the path they came from, and let undo move them back. One implementation, identical on all three platforms, and no crate feature that might not exist on the next one.

It also fixes two smaller things that only look separate:

- `trash::delete` fails while anything holds the file open, which is why `trash_with_retry` exists at all. A rename inside the same folder does not.
- A deleted entry currently leaves the project folder entirely, so a project that is a git repo records a deletion whose contents are gone. Under `.lapis-trash/` they are still in the tree until it is emptied.

What it costs: the trash stops being the OS's, so the app owns emptying it — some rule about age or size, and a way to empty it by hand. That is the part worth designing rather than guessing.

## Why not yet

It changes what deleting *means*, which is one of the app's stated invariants, and it wants the emptying rule thought through rather than bolted on. 0.1.0 ships with macOS undo telling the truth about what it cannot do.
