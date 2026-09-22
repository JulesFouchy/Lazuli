//! Two versions of one entry, and telling them apart.
//!
//! Syncing a project by hand — a git merge, a folder two machines both write
//! to — can leave an entry in two minds. Until now that was the worst kind of
//! failure the app had: an `entry.md` with conflict markers in it does not
//! parse, `scan` skipped it with a line on stderr, and **the entry disappeared
//! from the timeline**. Nothing was lost on disk and nothing said so.
//!
//! Two shapes arrive, and neither is ours to choose between:
//!
//! - **Markers inside the file**, which is what git leaves.
//! - **A second file beside it**, which is what a folder syncer leaves rather
//!   than touching the contents — Syncthing's `entry.sync-conflict-….md` and
//!   Dropbox's `entry (Someone's conflicted copy ….md)`.
//!
//! Both reduce to the same thing: more than one version of an entry, offered
//! for the user to pick from. Prose is never merged automatically, which is the
//! rule `wip.md` set out — the app keeps both and asks.

use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::EntryFrontmatter;
use crate::store::ENTRY_FILE;

/// The line a git merge opens a conflict with.
const OURS: &str = "<<<<<<<";
/// The base section of a `diff3`-style conflict, which is neither side.
const BASE: &str = "|||||||";
/// The line between the two sides.
const SPLIT: &str = "=======";
/// The line a conflict closes with.
const THEIRS: &str = ">>>>>>>";

/// One version of an entry, as the card offers it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Version {
    /// Where this one came from, in words the user can act on.
    pub label: String,
    pub text: String,
    /// The day it claims, when it has readable frontmatter.
    pub date: Option<chrono::NaiveDate>,
    /// The picture it chose, when it has readable frontmatter.
    pub image: Option<String>,
}

/// An entry that arrived in more than one version.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    /// What left it this way, so the card can say so.
    pub kind: Kind,
    /// The versions to choose between, the file's own first.
    pub versions: Vec<Version>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Conflict markers inside `entry.md`, left by a merge.
    Markers,
    /// A second file beside `entry.md`, left by a folder syncer.
    Sidecar,
}

/// The conflicting versions of the entry in `dir`, if it is in more than one.
///
/// `contents` is `entry.md` as it stands, already read. Returns `None` for the
/// ordinary case, which is nearly always.
pub fn of_entry(dir: &Path, contents: &str) -> Option<Conflict> {
    if let Some(versions) = split_markers(contents) {
        return Some(Conflict {
            kind: Kind::Markers,
            versions,
        });
    }

    let sidecars = sidecars(dir);
    if sidecars.is_empty() {
        return None;
    }

    let mut versions = vec![version("This copy", contents)];
    for path in sidecars {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("The other copy")
            .to_owned();
        versions.push(version(&label, &text));
    }
    // One version is not a conflict: every sidecar failed to be read.
    (versions.len() > 1).then_some(Conflict {
        kind: Kind::Sidecar,
        versions,
    })
}

/// Files a folder syncer left beside `entry.md` because it would not choose.
///
/// Matched by the names those tools actually use rather than by "any other
/// `.md`": an entry folder is the user's, and a `notes.md` they put there
/// themselves is not a conflict.
pub fn sidecars(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_sidecar_name)
        })
        .collect();
    // Stable, so the card offers them in the same order every scan and the
    // project does not appear to change when nothing has.
    found.sort();
    found
}

fn is_sidecar_name(name: &str) -> bool {
    if name == ENTRY_FILE || !name.ends_with(".md") {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    // Syncthing, then Dropbox.
    lower.contains(".sync-conflict-") || lower.contains("conflicted copy")
}

/// Split a file holding git conflict markers into the two sides.
///
/// `None` when there are no markers, which is the ordinary case and the only
/// one worth being fast about.
///
/// An unclosed conflict — a file cut off mid-merge — still yields what it has,
/// because a half-written side the user can read and copy from beats an entry
/// that vanishes.
fn split_markers(contents: &str) -> Option<Vec<Version>> {
    if !contents.lines().any(|line| line.starts_with(OURS)) {
        return None;
    }

    let mut ours = String::new();
    let mut theirs = String::new();
    let mut side = Side::Both;

    for line in contents.lines() {
        if line.starts_with(OURS) {
            side = Side::Ours;
        } else if line.starts_with(BASE) {
            side = Side::Base;
        } else if line.starts_with(SPLIT) && side != Side::Both {
            side = Side::Theirs;
        } else if line.starts_with(THEIRS) {
            side = Side::Both;
        } else {
            match side {
                Side::Both => {
                    ours.push_str(line);
                    ours.push('\n');
                    theirs.push_str(line);
                    theirs.push('\n');
                }
                Side::Ours => {
                    ours.push_str(line);
                    ours.push('\n');
                }
                Side::Theirs => {
                    theirs.push_str(line);
                    theirs.push('\n');
                }
                // The common ancestor, which is neither side's answer.
                Side::Base => {}
            }
        }
    }

    Some(vec![version("Yours", &ours), version("Theirs", &theirs)])
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Side {
    Both,
    Ours,
    Base,
    Theirs,
}

/// Read one version's frontmatter and body, falling back to showing the whole
/// file as text when it does not parse.
///
/// A side of a conflict is not guaranteed to be a whole valid entry — the
/// markers can fall inside the frontmatter — and a version that cannot be
/// parsed is still one the user can read and choose.
fn version(label: &str, contents: &str) -> Version {
    match crate::store::parse_entry(contents) {
        Ok((frontmatter, text)) => Version {
            label: label.to_owned(),
            text,
            date: Some(frontmatter.journal_date()),
            image: frontmatter.image,
        },
        Err(_) => Version {
            label: label.to_owned(),
            text: contents.trim().to_owned(),
            date: None,
            image: None,
        },
    }
}

/// Write `chosen` as the entry's only version, and put the rest in the trash.
///
/// The losers go to `.lazuli-trash/` rather than being removed: choosing
/// between two versions of a sentence is exactly the kind of decision somebody
/// wants back an hour later.
pub fn resolve(
    root: &Path,
    dir: &Path,
    conflict: &Conflict,
    chosen: usize,
    by: Option<&str>,
) -> Result<()> {
    let version = conflict
        .versions
        .get(chosen)
        .ok_or_else(|| anyhow::anyhow!("there is no version {chosen} to keep"))?;

    let entry_file = dir.join(ENTRY_FILE);

    // Read before anything moves it. `created` says when the entry was made,
    // which choosing between two sentences does not change, and it is what
    // orders two entries sharing a day.
    let existing = crate::store::read_entry_file(&entry_file).ok();

    // Then the file the user has been looking at goes to the trash, so it is
    // recoverable whichever version wins.
    crate::trashcan::put(root, &entry_file, by)?;

    let frontmatter = EntryFrontmatter {
        date: version.date,
        // Kept from whatever was there, because it says when the entry was
        // made and no version of a conflict changes that.
        created: existing
            .as_ref()
            .map(|(frontmatter, _)| frontmatter.created)
            .unwrap_or_else(|| chrono::Local::now().fixed_offset()),
        image: version.image.clone(),
        author: existing.and_then(|(frontmatter, _)| frontmatter.author),
    };
    crate::store::write_entry_file(dir, &frontmatter, &version.text)?;

    // And the sidecars, now that one of them has been chosen or rejected.
    for sidecar in sidecars(dir) {
        crate::trashcan::put(root, &sidecar, by)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MERGED: &str = "\
---
date: 2026-09-20
created: 2026-09-20T10:00:00+02:00
image: null
---

<<<<<<< HEAD
The roof went on today.
=======
Put the roof on, finally.
>>>>>>> their-branch
";

    #[test]
    fn a_file_with_no_markers_is_not_a_conflict() {
        assert!(split_markers("---\nnothing here\n---\n\nplain").is_none());
    }

    #[test]
    fn the_two_sides_of_a_merge_come_out_whole() {
        let versions = split_markers(MERGED).expect("should be a conflict");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].label, "Yours");
        assert_eq!(versions[0].text, "The roof went on today.");
        assert_eq!(versions[1].text, "Put the roof on, finally.");
        // The frontmatter was outside the conflict, so both sides keep it.
        assert_eq!(versions[0].date, versions[1].date);
        assert!(versions[0].date.is_some());
    }

    #[test]
    fn a_diff3_conflict_leaves_the_common_ancestor_out() {
        // `merge.conflictStyle = diff3` adds a third section, which is what the
        // two sides disagreed *from* and is nobody's answer.
        let merged = "\
---
date: 2026-09-20
created: 2026-09-20T10:00:00+02:00
image: null
---

<<<<<<< HEAD
mine
||||||| base
what we both started from
=======
theirs
>>>>>>> branch
";
        let versions = split_markers(merged).expect("should be a conflict");
        assert_eq!(versions[0].text, "mine");
        assert_eq!(versions[1].text, "theirs");
    }

    #[test]
    fn a_conflict_inside_the_frontmatter_gives_two_readable_entries() {
        // The markers fall in the frontmatter rather than the sentence, so
        // neither side parses while they are there. Split, each side is a whole
        // entry again, and what they disagree about is the day.
        let merged = "\
---
<<<<<<< HEAD
date: 2026-09-20
=======
date: 2026-09-21
>>>>>>> branch
created: 2026-09-20T10:00:00+02:00
---

Same sentence either way.
";
        let versions = split_markers(merged).expect("should be a conflict");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].text, "Same sentence either way.");
        assert_eq!(versions[1].text, "Same sentence either way.");
        assert_eq!(
            versions[0].date,
            chrono::NaiveDate::from_ymd_opt(2026, 9, 20)
        );
        assert_eq!(
            versions[1].date,
            chrono::NaiveDate::from_ymd_opt(2026, 9, 21)
        );
    }

    #[test]
    fn a_side_that_still_does_not_parse_is_shown_as_its_own_text() {
        // Nothing guarantees a side is a whole entry — a merge can cut the
        // fence itself — and a version the user can read and copy from beats an
        // entry that disappears.
        let merged = "<<<<<<< HEAD\nnot an entry at all\n=======\nnor this\n>>>>>>> branch\n";
        let versions = split_markers(merged).expect("should be a conflict");
        assert_eq!(versions[0].text, "not an entry at all");
        assert_eq!(versions[0].date, None);
    }

    #[test]
    fn an_unfinished_conflict_still_yields_what_it_has() {
        let merged = "<<<<<<< HEAD\nmine\n=======\ntheirs\n";
        let versions = split_markers(merged).expect("should be a conflict");
        assert_eq!(versions[0].text, "mine");
        assert_eq!(versions[1].text, "theirs");
    }

    #[test]
    fn the_syncers_own_names_are_recognised() {
        assert!(is_sidecar_name("entry.sync-conflict-20260922-101010-ABCDEFG.md"));
        assert!(is_sidecar_name("entry (Jules's conflicted copy 2026-09-22).md"));
    }

    #[test]
    fn a_file_the_user_put_there_is_not_a_conflict() {
        // An entry folder is theirs, and notes beside a picture are a fair use
        // of it.
        assert!(!is_sidecar_name("notes.md"));
        assert!(!is_sidecar_name("entry.md"));
        assert!(!is_sidecar_name("README.md"));
        assert!(!is_sidecar_name("photo.jpg"));
    }
}
