// Cuts a release: sets the version, commits, tags, pushes, and hands over to
// the workflow that does everything else.
//
//   node scripts/release.mjs 0.2.0
//   node scripts/release.mjs 0.2.0 --dry-run     # say what would happen
//   node scripts/release.mjs 0.2.0 --no-checks   # skip clippy and the tests
//
// Write the CHANGELOG.md section for the version first — this refuses to
// release without one, because that section becomes the release notes.
//
// Clippy, the tests and the frontend build all run here, before the tag is cut.
// Nothing after the tag re-checks them, so this is the only chance to fail
// without having to delete a tag.
//
// Everything after `git push --tags` is unattended: four platforms build, the
// installers are signed and uploaded to the public lazuli-releases repo, and the
// release publishes itself once all four have landed. Publishing is the moment
// existing installs start picking the new version up, silently, on next close.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const version = process.argv[2];
const dryRun = process.argv.includes("--dry-run");

if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("usage: node scripts/release.mjs <version> [--dry-run]");
  console.error("       version is three numbers, like 0.2.0");
  process.exit(1);
}

const run = (cmd, args) =>
  execFileSync(cmd, args, { encoding: "utf8", stdio: "pipe" }).trim();

const step = (what, cmd, args) => {
  if (dryRun) {
    console.log(`would run: ${cmd} ${args.join(" ")}`);
    return "";
  }
  console.log(what);
  return run(cmd, args);
};

function fail(message) {
  console.error(`\n${message}`);
  process.exit(1);
}

// --- everything that can be wrong, before anything is changed ---------------

if (run("git", ["rev-parse", "--abbrev-ref", "HEAD"]) !== "main") {
  fail("not on main. Releases are cut from main.");
}

// The files the bump rewrites, and the lockfile that follows Cargo.toml. The
// version commit names exactly these, so anything else uncommitted in the tree
// — the normal state here — stays out of the release and is left alone. What
// must not happen is an unrelated edit to one of *these* being swept in, so
// those four have to be clean before the bump.
const VERSION_FILES = [
  "package.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];
const dirty = run("git", ["status", "--porcelain", "--", ...VERSION_FILES]);
if (dirty) {
  fail(`these hold the version and have uncommitted changes; commit them first:
${dirty}`);
}

// The section that becomes the release notes. Checked here as well as in CI,
// because finding out after the tag is pushed means deleting a tag.
const changelog = readFileSync("CHANGELOG.md", "utf8");
const section = changelog
  .split(/^## /m)
  .find((block) => block.startsWith(version));
if (!section || !section.slice(version.length).replace(/[\s—-]/g, "")) {
  fail(
    `CHANGELOG.md has no "## ${version}" section with anything in it.\n` +
      "That section is the release notes, so write it first.",
  );
}
if (/unreleased/i.test(section.split("\n")[0])) {
  fail(`the "## ${version}" heading still says "unreleased". Take it off.`);
}

if (run("git", ["tag", "-l", `v${version}`])) {
  fail(`v${version} already exists as a tag. Pick the next version.`);
}

// Clippy and the tests, here rather than in the workflow. The workflow can only
// run them once the tag is pushed, and a tag that has to be deleted is the one
// expensive way for a release to fail; this machine is warm and incremental, so
// the same check costs a fraction of what a cold runner charges for it.
//
// `--no-checks` skips this, for re-cutting a release when only the CHANGELOG
// or the version was wrong and the code has not moved.
if (!process.argv.includes("--no-checks")) {
  const check = (what, cmd, args, cwd) => {
    if (dryRun) {
      console.log(`would run: ${cmd} ${args.join(" ")}`);
      return;
    }
    process.stdout.write(`  ${what}… `);
    try {
      // `npm` is a .cmd on Windows, which execFile cannot start on its own.
      execFileSync(cmd, args, { cwd, stdio: "pipe", shell: cmd === "npm" });
    } catch (e) {
      console.error("failed\n");
      process.stderr.write(e.stdout?.toString() ?? "");
      process.stderr.write(e.stderr?.toString() ?? "");
      fail(`${what} failed. Fix it before releasing.`);
    }
    console.log("ok");
  };
  console.log("Checking\n");
  // `tsc --noEmit` and the frontend build, which the Rust build also needs.
  check("frontend", "npm", ["run", "build"]);
  // Single-job everywhere: parallel cargo exhausts the page file on this machine.
  check("clippy", "cargo", ["clippy", "-j", "1", "--all-targets", "--", "-D", "warnings"], "src-tauri");
  check("tests", "cargo", ["test", "-j", "1"], "src-tauri");
  console.log();
}

console.log(`Releasing ${version}\n`);

// --- the release ------------------------------------------------------------

step(`  version → ${version}`, "node", ["scripts/set-version.mjs", version]);

// Normally the line above just changed three files. On a first release, or on
// a re-run after the bump was committed by hand, it changed nothing and there
// is no commit to make — which is fine, as long as the tag still gets cut.
if (dryRun || run("git", ["status", "--porcelain", "--", ...VERSION_FILES])) {
  step("  commit", "git", ["commit", "-m", `Lazuli ${version}`, "--", ...VERSION_FILES]);
} else {
  console.log(`  commit — nothing to commit, HEAD is already ${version}`);
}
step("  tag", "git", ["tag", `v${version}`]);
step("  push", "git", ["push", "origin", "main"]);
step("  push tag", "git", ["push", "origin", `v${version}`]);

if (dryRun) process.exit(0);

console.log(`
Pushed. The rest happens on its own — four platforms build, the installers are
signed and uploaded, and the release publishes itself when all four have landed.
Roughly fifteen minutes.

  watch it:   gh run watch --repo JulesFouchy/Lazuli
  the result: https://github.com/JulesFouchy/lazuli-releases/releases

Once it is published, every existing install picks it up silently: downloaded
during a session, applied between sessions.`);
