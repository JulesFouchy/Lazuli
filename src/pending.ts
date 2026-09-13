// Things the user has asked to delete, that the filesystem has not caught up
// on yet.
//
// Moving something to the Recycle Bin goes through the Windows shell and can
// take a second or two, and a folder that something else still holds a handle
// on takes longer still. Waiting for that before the row disappears makes the
// click feel broken.
//
// This does not make the app believe anything about the project: disk is still
// the only authority, and `state.project` is still whatever the last scan
// found. It is a view filter and nothing more — these keys hide rows that have
// been asked for, and are dropped again the moment the call returns, whether it
// worked or not. A failed delete therefore puts the row back.

const pending = new Set<string>();

export const entryKey = (id: string) => `entry:${id}`;

export const imageKey = (directory: string, filename: string) =>
  `image:${directory}/${filename}`;

export const projectKey = (path: string) => `project:${path}`;

export function markDeleting(key: string): void {
  pending.add(key);
}

export function unmarkDeleting(key: string): void {
  pending.delete(key);
}

export function isDeleting(key: string): boolean {
  return pending.has(key);
}
