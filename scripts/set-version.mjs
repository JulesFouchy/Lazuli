// Sets the app's version in the three files that each hold their own copy.
//
//   node scripts/set-version.mjs 0.2.0
//   node scripts/set-version.mjs            # prints the current three
//
// `tauri.conf.json` is the one that decides what the app reports and what the
// updater compares against; `package.json` and `Cargo.toml` are not read by
// anything at runtime. They are kept in step anyway, because a `Cargo.toml`
// that disagrees is what makes someone believe the wrong thing while reading
// the tree, and because the release workflow refuses a tag that does not match
// all three — an update whose version is not what the endpoint advertised is
// downloaded by every client and then declined.

import { readFileSync, writeFileSync } from "node:fs";

/** Each file, and the one line in it that carries the version. */
const FILES = [
  { path: "package.json", pattern: /^(\s*"version":\s*")([^"]+)(")/m },
  { path: "src-tauri/tauri.conf.json", pattern: /^(\s*"version":\s*")([^"]+)(")/m },
  // Anchored to the `[package]` block's own `version`, which is the first one
  // in the file; a dependency's `version = "2"` further down must not match.
  { path: "src-tauri/Cargo.toml", pattern: /^(version = ")([^"]+)(")/m },
  // Cargo would rewrite this on its next run anyway, but "its next run" was
  // after the release commit, so the committed lockfile lagged the version by
  // one release. Anchored on the package name so no dependency can match.
  { path: "src-tauri/Cargo.lock", pattern: /^(name = "lazuli"\s+version = ")([^"]+)(")/m },
];

const next = process.argv[2];

if (next && !/^\d+\.\d+\.\d+$/.test(next)) {
  console.error(`not a version: ${next} — expected three numbers, like 0.2.0`);
  process.exit(1);
}

let failed = false;

for (const { path, pattern } of FILES) {
  // Read and write as UTF-8 text with no newline translation: the repo is LF
  // and `writeFileSync` does not rewrite line endings, unlike Python's text
  // mode. Only the matched span changes, so nothing else can be disturbed.
  const before = readFileSync(path, "utf8");
  const found = before.match(pattern);
  if (!found) {
    console.error(`${path}: no version line found`);
    failed = true;
    continue;
  }
  if (!next) {
    console.log(`${found[2].padEnd(12)} ${path}`);
    continue;
  }
  writeFileSync(path, before.replace(pattern, `$1${next}$3`));
  console.log(`${found[2]} → ${next}   ${path}`);
}

if (failed) process.exit(1);

if (next) {
  console.log(`\nNext: commit, then  git tag v${next} && git push origin v${next}`);
}
