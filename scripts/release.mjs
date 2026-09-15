// Cuts a release: sets the version, commits, tags, pushes, and hands over to
// the workflow that does everything else.
//
//   node scripts/release.mjs 0.2.0
//   node scripts/release.mjs 0.2.0 --dry-run     # say what would happen
//
// Write the CHANGELOG.md section for the version first — this refuses to
// release without one, because that section becomes the release notes.
//
// Everything after `git push --tags` is unattended: four platforms build, the
// installers are signed and uploaded to the public lapis-releases repo, and the
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

// A dirty tree would ride along in the version commit below, which names no
// paths — this is the one place in this repo where that is the right shape,
// because a release must be a commit of exactly the version bump.
if (run("git", ["status", "--porcelain"])) {
  fail("the working tree has changes. Commit or stash them first:\n" +
    run("git", ["status", "--short"]));
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

console.log(`Releasing ${version}\n`);

// --- the release ------------------------------------------------------------

step(`  version → ${version}`, "node", ["scripts/set-version.mjs", version]);
step("  commit", "git", ["commit", "-am", `Lapis ${version}`]);
step("  tag", "git", ["tag", `v${version}`]);
step("  push", "git", ["push", "origin", "main"]);
step("  push tag", "git", ["push", "origin", `v${version}`]);

if (dryRun) process.exit(0);

console.log(`
Pushed. The rest happens on its own — four platforms build, the installers are
signed and uploaded, and the release publishes itself when all four have landed.
Roughly fifteen minutes.

  watch it:   gh run watch --repo JulesFouchy/Lapis
  the result: https://github.com/JulesFouchy/lapis-releases/releases

Once it is published, every existing install picks it up silently the next time
it is closed.`);
