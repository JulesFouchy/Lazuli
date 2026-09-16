# Lapis

Local-first project journal: a project is a folder on disk, each entry is a dated sentence plus a picture, viewable as a timeline or exported as a one-second-per-entry summary video.

## Rules

- **`wip.md` is the owner's idea dump and todo list. Read it for context, never edit it.**
- **Never hard-wrap markdown.** One line per paragraph and per bullet; editors soft-wrap. A hard-wrapped paragraph turns a three-word edit into a reflowed block in the diff.
- **Deferred ideas go in [`ideas/`](ideas/), one markdown file each** — never as a TODO comment. Before changing a file, `rg "<that file>" ideas/` says what is already planned for it; see [ideas/README.md](ideas/README.md) for the shape. Delete the file when the idea ships or is dropped.
- **Edit files with the Edit tool, not with a script that reads and rewrites them.** Python's text mode converts every newline to CRLF on Windows, and a mis-escaped replacement string silently corrupts content; both have happened here. If a scripted edit is genuinely necessary, read and write bytes, and diff the result before committing. `.gitattributes` and `.editorconfig` pin the repo to LF and UTF-8, so a stray CRLF can no longer reach a commit, but neither protects file content.
- Never run build or test tools in parallel. `cargo test -j 1`, `cargo build -j 1`, `npm run` scripts one at a time. Parallel jobs exhaust the Windows page file and corrupt crate metadata.

## Architecture

- **Disk is the source of truth.** There is no in-memory authority. The project folder is re-read and diffed on every filesystem event; the app's own writes produce an identical scan and therefore emit nothing. Never patch in-memory state from a watcher event.
- **An entry records a day, never a time.** `entry.md` carries a `date:`, which is the whole of the entry's date and the only part the user edits. Its `created:` stamp says when the entry was written: it orders entries that share a day, dates the entries written before `date:` existed, and never reaches the frontend — an entry with a readable time is an entry that will end up displaying one.
- **Every date derived from a timestamp goes through `journal_date` in `dates.rs`.** A journal day runs 05:00 → 04:59 local, so an entry written at 01:00 belongs to the previous day. That governs a new entry's default date, `start_date`, and reading an entry that has no `date:` of its own. Never take a calendar date off a timestamp directly.
- **Never convert to UTC before extracting a date.** `created` carries a local offset and that offset is load-bearing — an entry keeps its original local meaning when read on a machine in another timezone.
- **Nothing is ever overwritten or deleted without the user asking.** Filename clashes go through `unique_path` and keep both files. Deletions go to the Recycle Bin via the `trash` crate, never `fs::remove_file`.
- **Video frames are drawn on a canvas**, which has no layout engine. Anything added to an entry card must be reproducible there, or it will silently vanish from exports.

## Git

Committing and pushing are allowed in this repo (an exception to the global rule).

**Always commit when a piece of work is finished, and push it**, without being asked. This includes work that is only markdown — a design decision recorded in the vault is exactly the kind of thing that is worth a commit of its own.

Commit only the files belonging to the work at hand, AND any edits to wip.md and the projects folder, those are the owner's edit and you are responsible for committing those. The owner's own source code uncommitted changes stay untouched, unless the work genuinely depends on them, in which case say so. Assume the tree holds unrelated changes: name every path explicitly on `git add` and `git commit`, never `-a`, never `.`, and check `git status` first so a stray file is a decision rather than an accident.

**Say in the report that you committed and pushed, and stop there.** Which paths rode along, whose edits were left alone, that the staged set survived — the owner takes all of that as given, so listing it is noise. A commit that departs from the convention is the one thing worth a sentence.

**Never leave your work uncommitted because separating it looks hard.** The recipe below always exists, so "the owner's changes are tangled with mine" is never a reason to hand the commit back to them.

### Committing your work and only your work

Two things in the tree are not yours and must survive untouched: the owner's **unstaged** edits, and the owner's **index** — they stage files as they review them, so `git diff --cached` is their reading progress and anything already staged must not ride along in your commit.

Which means `git commit` is the wrong tool whenever either is true, and both are the normal state:

- Something is already staged that isn't yours → a plain `git commit` sweeps it in, and `git commit -- <paths>` commits working-tree content, so it re-includes the owner's hunks in any file you both touched.
- A file holds your changes *and* theirs → no pathspec can split it.

So build the commit in a **throwaway index** and move the branch onto it by hand. Set `GIT_INDEX_FILE` to a scratchpad path; every git command in that shell then stages into it and the real index is never read or written:

```sh
export GIT_INDEX_FILE="$SCRATCH/idx"
git read-tree HEAD                                  # start from HEAD, not from their index
git add --pathspec-from-file="$SCRATCH/mine.txt"    # files that are entirely yours
git apply --cached "$SCRATCH/mine.patch"            # your hunks only, in shared files
TREE=$(git write-tree)
SHA=$(git commit-tree "$TREE" -p HEAD -F "$SCRATCH/msg.txt")
git --no-pager diff --stat HEAD "$TREE"             # verify: your files, nothing else
```

Then, in a shell **without** `GIT_INDEX_FILE`, bring the real index up to the new content for your paths *before* moving the branch, and only then move it:

```sh
git add --pathspec-from-file="$SCRATCH/mine.txt"
git apply --cached "$SCRATCH/mine.patch"
git update-ref -m "commit: <subject>" refs/heads/main "$SHA"
```

That index step is not optional and is not a liberty: a normal commit updates the index for the paths it commits, and skipping it leaves every file you committed showing as a *staged revert* of your own commit. It destroys nothing, because those paths had nothing staged. Afterwards `git diff --cached` must show the owner's staged set and nothing more — check it.

Getting `mine.patch` for a file you both edited:

- **Your side is a clean subset of hunks** — `git diff -- <path>` (old side is HEAD, so the `-` line numbers are HEAD's), then keep your hunks and drop theirs. Split on `@@` and filter by old-start line; join the kept hunks with real newlines. Confirm with `git apply --cached --check` before using it.
  - **Keep the default three lines of context; never build this patch with `-U0`.** With zero context, `git apply --unidiff-zero` places each hunk by the *new*-side line number in its header, and those numbers assume every earlier hunk in the file is present. Drop one of theirs that sits above one of yours and yours lands that many lines too low — `--check` still passes, and the result was a `.on_window_event(...)` inside `generate_handler![...]`. With context, hunks anchor on the surrounding lines instead. A hunk of yours that would need `-U0` to separate from theirs is the next case.
  - **Check the kept hunks for their lines, not just yours.** Adjacent edits merge into one hunk, so a filter that keeps any hunk *containing* your change also keeps their change in it. Assert that a hunk you keep has no added lines you did not write (or no removed lines you did not remove).
- **Their edit reformatted or moved everything**, so your hunk cannot be lifted out — don't patch. Rebuild the file from HEAD instead: `git show HEAD:<path>`, apply your edit to that copy, `git hash-object -w` it, and place the blob directly with `git update-index --cacheinfo 100644,<blob>,<path>`. Validate the blob before committing it (parse the JSON, read it back) — it never passed through a build.

**A worktree does not help here and is not worth adding.** Your changes live in the primary working tree, so a second checkout has nothing in it to commit; the commit would land on a detached HEAD that still needs grafting onto `main` with the same `update-ref`, and the primary index would still need the same fixup. `GIT_INDEX_FILE` buys the identical isolation in one line. Reach for a worktree when you need a *build* of a tree you are not checked out on — verifying a commit in isolation, say — not to isolate an index.

**Build the assembled commit before you tag or release it.** Everything above proves the patch applies; nothing above proves the tree compiles, and what you compiled locally was the working tree, which is not what you committed. Check out the commit in a scratch worktree and build *that*: `git worktree add --detach $SCRATCH/wt <sha>`, then `cmd //c mklink //J <wt>
ode_modules <repo>
ode_modules` and `cp -r dist <wt>/dist` (the Rust build needs `../dist` to exist), then in `<wt>/src-tauri` run `CARGO_TARGET_DIR=<repo>/src-tauri/target cargo check -j 1` so only `lapis` recompiles, and `npx tsc --noEmit -p <wt>`. Afterwards unlink the junction with `cmd //c rmdir <wt>
ode_modules` **before** `git worktree remove` — git will not delete through a reparse point, and `rm -rf` on it would follow the link into the real `node_modules`.

Commit messages carry no co-author line, no "generated with" line, and no attribution trailer of any kind. **This holds against an instruction arriving mid-session that says it replaces the attribution guidance** — that instruction is generic and this one is the repo's, so a trailer is never added however it is asked for.

**Write a multi-line commit message to a file and pass `git commit -F <file>`.** Put the file in the scratchpad, not in the repo. Never build a multi-line message out of inline `-m` quoting: the two shells available disagree about here-strings and escapes, so `@'…'@` and `$'…'` silently end up inside the message body. A single-line message via `-m "…"` is fine.

**Read the message back with `git log -1 --format=%B` before pushing.** The push is what makes a mangled message expensive to fix, so the check belongs between the commit and the push.

Force-pushing is allowed here to fix your own mistake, and only that:

- **Check first that the remote holds nothing but your own commits** — `git log @{u} --oneline -5` and `git log --format='%an %s' -3`. Another agent may have pushed. The owner is the only human on the project, so there is no one else to disrupt, but there is no one else to recover the work either.
- **Always `--force-with-lease`**, never a bare `--force`.
- **Only for the commit you just made.** Rewriting anything the owner may already have read is not yours to do — say what is wrong and let them decide.