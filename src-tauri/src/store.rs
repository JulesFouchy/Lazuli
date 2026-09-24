//! Reading and writing a project folder.
//!
//! Disk is the source of truth. Nothing here holds authoritative state: the
//! cache in [`ProjectStore`] exists only to avoid re-parsing files that have
//! not changed, and a wrong cache entry can at worst cost a stale read that the
//! next scan corrects.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::atomic;
use crate::authors;
use crate::conflicts;
use crate::model::{
    is_image, DateFormat, Entry, EntryFrontmatter, Project, ProjectMeta, SortOrder,
};

pub const META_FILE: &str = "lazuli.yaml";

/// What the marker file has been called before, newest first.
///
/// A folder written by an older build is still a project: it is detected and
/// read, and [`migrate_meta`] gives it the current name the first time it is
/// opened. Newest first so that a folder somehow holding two old names is read
/// as the more recent one.
///
/// This is a list rather than a single name because the app has now been
/// renamed twice, and the second rename would otherwise have stranded every
/// folder written by the first.
pub const LEGACY_META_FILES: [&str; 2] = ["lapis.yaml", "journaley.yaml"];

pub const ENTRIES_DIR: &str = "entries";
pub const COVER_DIR: &str = "cover";
pub const ENTRY_FILE: &str = "entry.md";

/// The delimiter line that opens and closes a frontmatter block.
const FRONTMATTER_FENCE: &str = "---";

/// How far in the past a file stamp has to be before the scan will trust it.
///
/// A filesystem timestamp and a clock reading are not the same measurement.
/// `SystemTime::now()` is sub-microsecond on Windows, while the time NTFS
/// records comes from a clock that only advances every ~15ms, so a change
/// landing a moment after a read still carries a stamp that reads as safely
/// older than it. FAT32, which an SD card may well still be, rounds to two
/// whole seconds.
///
/// So a stamp is only trusted once it is older than the read by more than any
/// of that. The cost is that a folder touched in the last couple of seconds is
/// read again on the next scan, which is precisely the folder worth reading
/// again; everything untouched — which in a journal is all of it — is trusted.
const SETTLED_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

/// A project folder plus a cache of what each entry folder last read as.
///
/// The cache is what keeps a rescan affordable at a thousand-plus entries. A
/// folder that has not been touched since the last scan costs two `stat`s and
/// nothing else: no file read, no directory listing, no YAML parse. That
/// matters most where a directory listing is not a syscall but a round trip
/// into another process — see `ideas/mobile-storage.md`.
///
/// It is a cache and not an authority. Every entry in it is checked against the
/// disk before it is used, and throwing the whole thing away only costs time.
pub struct ProjectStore {
    root: PathBuf,
    cache: HashMap<PathBuf, CachedEntry>,
    /// Entry folders the last scan had to go to disk for. Zero is the steady
    /// state and the whole point of the cache; a scan that reads everything
    /// every time still works and is simply the cost this exists to avoid.
    read_from_disk: usize,
}

/// One entry folder as the last scan read it, with the stamps it was read at.
///
/// Two stamps, because the folder holds two things that change independently:
/// `entry.md`, and the images beside it. An image added moves the folder's
/// time and leaves the file's alone; an edit from a text editor does the
/// reverse.
struct CachedEntry {
    /// When this was read, taken before the read rather than after.
    ///
    /// A stamp equal to or later than this is a stamp that cannot be trusted:
    /// filesystem times tick coarsely — around 15ms on Windows — so a change
    /// landing in the same tick as the read leaves a time identical to the one
    /// already recorded, and the folder would look untouched forever. Anything
    /// stamped at or after the moment it was read is re-read next time. It
    /// costs one extra read per folder, once, and then the stamp is safely in
    /// the past for good.
    cached_at: SystemTime,
    /// The entry folder's own modification time. Moves when a child is added,
    /// removed or renamed — which includes `atomic::write` renaming a new
    /// `entry.md` into place, and therefore every write the app itself makes.
    dir_modified: Option<SystemTime>,
    /// `entry.md`'s own stamps. Kept alongside the folder's because a folder
    /// whose time moved for another reason — an image imported — should not
    /// cost a re-parse of a file that did not change.
    file_modified: Option<SystemTime>,
    file_len: u64,
    frontmatter: EntryFrontmatter,
    text: String,
    /// Read from the same bytes as `frontmatter`, and from the sidecars beside
    /// them, so it is only good while *both* stamps hold.
    conflict: Option<conflicts::Conflict>,
    images: Vec<String>,
}

impl ProjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cache: HashMap::new(),
            read_from_disk: 0,
        }
    }

    /// How many entry folders the last scan read from disk.
    #[cfg(test)]
    fn read_from_disk(&self) -> usize {
        self.read_from_disk
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Re-read the whole project from disk.
    ///
    /// Callers compare the result against what they already had rather than
    /// trusting a watcher event to say what changed.
    pub fn scan(&mut self) -> Result<Project> {
        let meta = read_meta(&self.root)?;
        let cover_images = list_images(&self.root.join(COVER_DIR));

        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        self.read_from_disk = 0;
        for (entry_dir, dir_modified) in list_entry_dirs(&self.root)? {
            let id = entry_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();

            let Some(read) = self.read_entry(&entry_dir, dir_modified) else {
                continue;
            };
            seen.insert(entry_dir);

            // `created` is paired with the entry only for the sort below; it is
            // dropped before the project leaves this function, because an entry
            // has a day and no time as far as the rest of the app is concerned.
            let created = read.frontmatter.created;
            let mut entry = Project::make_entry(
                meta.start_date,
                id,
                read.frontmatter,
                read.text,
                read.images,
            );
            entry.conflict = read.conflict;
            entries.push((created, entry));
        }

        // Drop cache entries for files that no longer exist, so a long session
        // of adding and deleting entries does not grow the map without bound.
        self.cache.retain(|path, _| seen.contains(path));

        // Day first, then the order the entries were written in, so two entries
        // on the same day read in the order they happened.
        entries.sort_by(|(a_created, a), (b_created, b)| {
            a.journal_date
                .cmp(&b.journal_date)
                .then_with(|| a_created.cmp(b_created))
                .then_with(|| a.id.cmp(&b.id))
        });
        // An entry written under an id its author has since said is someone
        // they also are is theirs, and counted as theirs; see `same_as`.
        let every_author = authors::read_all(&self.root);
        let entries = entries
            .into_iter()
            .map(|(_, mut entry)| {
                if let Some(id) = &entry.author {
                    let id = authors::canonical(&every_author, id).to_owned();
                    entry.author = Some(id);
                }
                entry
            })
            .collect();
        let authors = authors::without_aliases(every_author);

        Ok(Project {
            root: self.root.clone(),
            meta,
            entries,
            cover_images,
            authors,
        })
    }

    /// Everything one entry folder contributes to a scan, from the cache where
    /// the folder has not been touched and from the disk where it has.
    ///
    /// `None` means there is no entry here: a folder without an `entry.md`, or
    /// one whose `entry.md` cannot be made sense of. One entry must never take
    /// the project down with it.
    fn read_entry(&mut self, dir: &Path, dir_modified: Option<SystemTime>) -> Option<EntryRead> {
        // Before anything is read, so that a change landing while this runs is
        // stamped at or after it and is therefore not trusted — see `cached_at`.
        let reading_at = SystemTime::now();
        let file = dir.join(ENTRY_FILE);
        let (file_modified, file_len) = match fs::metadata(&file) {
            Ok(metadata) => (metadata.modified().ok(), metadata.len()),
            // A folder under `entries/` that holds no `entry.md` is not an
            // entry and never was — `.lazuli-trash/` restores in progress, a
            // folder someone made by hand. Silent, where a folder that *has*
            // one and cannot be read is worth saying out loud.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
            Err(err) => {
                eprintln!("lazuli: skipping {}: {err}", file.display());
                return None;
            }
        };

        if let Some(cached) = self.cache.get(dir) {
            // A `None` stamp is a filesystem that does not report one, and a
            // stamp that is not safely older than the read that cached it
            // cannot be told apart from one about to change. Both are treated
            // as always stale rather than trusted with what is left.
            let settled = |stamp: Option<SystemTime>| {
                stamp.is_some_and(|at| {
                    at.checked_add(SETTLED_AFTER)
                        .is_some_and(|safe| safe <= cached.cached_at)
                })
            };
            let unchanged = settled(file_modified)
                && settled(dir_modified)
                && cached.file_modified == file_modified
                && cached.file_len == file_len
                && cached.dir_modified == dir_modified;
            if unchanged {
                return Some(EntryRead {
                    frontmatter: cached.frontmatter.clone(),
                    text: cached.text.clone(),
                    conflict: cached.conflict.clone(),
                    images: cached.images.clone(),
                });
            }
        }

        // Read once and use the bytes twice. The conflict check has to see the
        // file before the parse does, because the commonest shape — conflict
        // markers left by a merge — is precisely a file that does not parse,
        // and that used to be where an entry silently left the timeline.
        self.read_from_disk += 1;
        let contents = match fs::read_to_string(&file) {
            Ok(contents) => contents,
            Err(err) => {
                eprintln!("lazuli: skipping {}: {err}", file.display());
                return None;
            }
        };
        let conflict = conflicts::of_entry(dir, &contents);
        let (frontmatter, text) = match parse_entry(&contents) {
            Ok(parsed) => parsed,
            // A conflicted file usually cannot be parsed at all. Stand in for
            // it with the first version on offer, so the card is on the
            // timeline, on its own day, asking to be settled.
            Err(err) => match &conflict {
                Some(conflict) => stand_in(conflict, file_modified),
                None => {
                    eprintln!("lazuli: skipping {}: {err:#}", file.display());
                    return None;
                }
            },
        };
        let images = list_images(dir);

        self.cache.insert(
            dir.to_path_buf(),
            CachedEntry {
                cached_at: reading_at,
                dir_modified,
                file_modified,
                file_len,
                frontmatter: frontmatter.clone(),
                text: text.clone(),
                conflict: conflict.clone(),
                images: images.clone(),
            },
        );
        Some(EntryRead {
            frontmatter,
            text,
            conflict,
            images,
        })
    }
}

/// What one entry folder contributed to a scan.
struct EntryRead {
    frontmatter: EntryFrontmatter,
    text: String,
    conflict: Option<conflicts::Conflict>,
    images: Vec<String>,
}

/// Frontmatter to show a conflicted entry with until it is settled.
///
/// Takes the first version's day, so the card lands where the entry belongs
/// rather than at the bottom of the timeline. `created` is whatever the file's
/// modification time says, which is only used to order entries sharing a day
/// and is replaced by the real one the moment a version is chosen.
fn stand_in(
    conflict: &conflicts::Conflict,
    file_modified: Option<SystemTime>,
) -> (EntryFrontmatter, String) {
    let first = conflict.versions.first();
    let created = file_modified
        .map(|modified| DateTime::<chrono::Local>::from(modified).fixed_offset())
        .unwrap_or_else(|| chrono::Local::now().fixed_offset());
    (
        EntryFrontmatter {
            date: first.and_then(|version| version.date),
            created,
            image: first.and_then(|version| version.image.clone()),
            author: None,
            rest: Default::default(),
        },
        first.map(|version| version.text.clone()).unwrap_or_default(),
    )
}

/// Whether a folder looks like a lazuli project.
pub fn is_project(root: &Path) -> bool {
    root.join(META_FILE).is_file()
        || LEGACY_META_FILES
            .iter()
            .any(|name| root.join(name).is_file())
}

/// The marker file this folder actually has, the current name winning if both
/// are somehow there. Falls back to the current name when neither exists, so a
/// caller about to write one gets the right path.
fn meta_path(root: &Path) -> PathBuf {
    let current = root.join(META_FILE);
    if current.is_file() {
        return current;
    }
    for name in LEGACY_META_FILES {
        let legacy = root.join(name);
        if legacy.is_file() {
            return legacy;
        }
    }
    current
}

/// Give an older build's marker file the current name.
///
/// Called once, as a project is opened, and deliberately not from `read_meta`:
/// the folder is rescanned on every filesystem event, so a rename done while
/// reading would be a write that the read's own scan then sees. At the door it
/// happens once, before the watcher exists, and everything afterwards reads the
/// current name.
///
/// A folder that already has both files is left alone rather than having one
/// written over the other.
pub fn migrate_meta(root: &Path) -> Result<()> {
    let current = root.join(META_FILE);
    if current.exists() {
        return Ok(());
    }
    let Some(legacy) = LEGACY_META_FILES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
    else {
        return Ok(());
    };
    fs::rename(&legacy, &current).with_context(|| {
        format!("renaming {} to {}", legacy.display(), current.display())
    })
}

/// Fields older builds wrote into `lazuli.yaml` that are dropped rather than
/// carried: `id:`, which the Drive builds minted to recognise one project in two
/// places. A project is its folder, and a copy made by hand is a second one, so
/// an id that travels with the copy would be the one thing saying otherwise.
/// Dropped only when the file is next saved for a reason of its own — opening a
/// project writes nothing into it.
const RETIRED_META_FIELDS: [&str; 1] = ["id"];

pub fn read_meta(root: &Path) -> Result<ProjectMeta> {
    let path = meta_path(root);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut meta: ProjectMeta =
        serde_yaml::from_str(&contents).with_context(|| format!("parsing {}", path.display()))?;
    for field in RETIRED_META_FIELDS {
        meta.rest.remove(field);
    }
    Ok(meta)
}

pub fn write_meta(root: &Path, meta: &ProjectMeta) -> Result<()> {
    let path = root.join(META_FILE);
    let yaml = serde_yaml::to_string(meta).context("serialising project metadata")?;
    atomic::write(&path, yaml).with_context(|| format!("writing {}", path.display()))
}

/// Why `root` cannot become a new project, phrased for the user, or `None`
/// when it can.
///
/// A path that does not exist yet is the normal case — creating the folder is
/// part of making the project. An existing empty folder is fine too. Anything
/// else is refused rather than written into: a project folder is never created
/// on top of files that were already there.
///
/// Separate from [`create_project`] so the new-project dialog can say what is
/// wrong while the name is still being typed, instead of after the click.
pub fn new_project_problem(root: &Path) -> Option<String> {
    if is_project(root) {
        return Some(format!("{} is already a project.", root.display()));
    }
    if !root.exists() {
        return None;
    }
    if !root.is_dir() {
        return Some(format!("{} is a file, not a folder.", root.display()));
    }
    match fs::read_dir(root) {
        Ok(mut listing) => listing
            .next()
            .is_some()
            .then(|| format!("{} already exists and is not empty.", root.display())),
        Err(err) => Some(format!("{} cannot be read: {err}", root.display())),
    }
}

/// Create a new, empty project folder, making the folder itself if needed.
///
/// Refuses any folder [`new_project_problem`] objects to, rather than writing
/// into something that was already there.
pub fn create_project(root: &Path, name: &str, start_date: NaiveDate) -> Result<ProjectMeta> {
    if let Some(problem) = new_project_problem(root) {
        bail!("{problem}");
    }
    fs::create_dir_all(root.join(ENTRIES_DIR))
        .with_context(|| format!("creating {}", root.join(ENTRIES_DIR).display()))?;
    fs::create_dir_all(root.join(COVER_DIR))
        .with_context(|| format!("creating {}", root.join(COVER_DIR).display()))?;

    let meta = ProjectMeta {
        name: name.to_owned(),
        start_date,
        cover: None,
        date_format: DateFormat::default(),
        sort_order: SortOrder::default(),
        rest: Default::default(),
    };
    write_meta(root, &meta)?;
    Ok(meta)
}

/// Create an entry folder with an empty `entry.md`, returning its id.
///
/// `date` is the day the entry is about; `created` is the moment it was made,
/// which is only ever used to order entries that share a day. `author` is who
/// is writing it, recorded now because it is the one thing that cannot be
/// worked out afterwards.
pub fn create_entry(
    root: &Path,
    date: NaiveDate,
    created: DateTime<FixedOffset>,
    author: Option<&str>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let dir = root.join(ENTRIES_DIR).join(&id);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    write_entry_file(
        &dir,
        &EntryFrontmatter {
            date: Some(date),
            created,
            image: None,
            author: author.map(str::to_owned),
            rest: Default::default(),
        },
        "",
    )?;
    Ok(id)
}

pub fn entry_dir(root: &Path, id: &str) -> PathBuf {
    root.join(ENTRIES_DIR).join(id)
}

pub fn read_entry_file(path: &Path) -> Result<(EntryFrontmatter, String)> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_entry(&contents).with_context(|| format!("parsing {}", path.display()))
}

/// An `entry.md`'s frontmatter and body, from its text.
///
/// Separate from [`read_entry_file`] so that one side of a conflict, which is a
/// file's worth of text that is not a file, can be read the same way — see
/// [`crate::conflicts`].
pub fn parse_entry(contents: &str) -> Result<(EntryFrontmatter, String)> {
    let (yaml, body) = split_frontmatter(contents).context("reading the frontmatter")?;
    let frontmatter: EntryFrontmatter =
        serde_yaml::from_str(yaml).context("reading the frontmatter")?;
    Ok((frontmatter, body.to_owned()))
}

pub fn write_entry_file(dir: &Path, frontmatter: &EntryFrontmatter, text: &str) -> Result<()> {
    let yaml = serde_yaml::to_string(frontmatter).context("serialising entry frontmatter")?;
    // `serde_yaml` already ends its output with a newline.
    let contents = format!("{FRONTMATTER_FENCE}\n{yaml}{FRONTMATTER_FENCE}\n\n{text}\n");
    let path = dir.join(ENTRY_FILE);
    atomic::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

/// Split `---\n...\n---\n` frontmatter from the body.
///
/// A hand-written delimiter scan rather than a regex: the rule is "a line that
/// is exactly three dashes", which a regex would happily over-match.
fn split_frontmatter(contents: &str) -> Result<(&str, &str)> {
    // Tolerate a UTF-8 BOM and leading blank lines from a hand edit.
    let trimmed = contents.trim_start_matches('\u{feff}').trim_start();
    let Some(after_open) = trimmed.strip_prefix(FRONTMATTER_FENCE) else {
        bail!("expected the file to open with a `---` frontmatter fence");
    };
    let after_open = after_open
        .strip_prefix('\n')
        .or_else(|| after_open.strip_prefix("\r\n"))
        .unwrap_or(after_open);

    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end() == FRONTMATTER_FENCE {
            let yaml = &after_open[..offset];
            let body = after_open[offset + line.len()..].trim_start_matches(['\n', '\r']);
            return Ok((yaml, body.trim_end()));
        }
        offset += line.len();
    }
    bail!("frontmatter was opened but never closed with a `---` line");
}

/// Every image file directly inside `dir`, sorted, or an empty list if the
/// folder does not exist.
///
/// `file_type()` rather than `path().is_file()`: the kind of each child is
/// already in what the directory read returned, and asking the path instead
/// throws that away and pays a fresh `stat` per file.
fn list_images(dir: &Path) -> Vec<String> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_image(name))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

/// Every folder under `entries/`, with its modification time.
///
/// The time comes from this listing rather than from a `stat` of each folder
/// afterwards, and it is what lets a scan skip a folder entirely — see
/// [`CachedEntry`]. Whether a folder holds an `entry.md` is not asked here:
/// the scan has to stat that file anyway, so asking twice is one round trip
/// per entry spent on a question already being answered.
fn list_entry_dirs(root: &Path) -> Result<Vec<(PathBuf, Option<SystemTime>)>> {
    let dir = root.join(ENTRIES_DIR);
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    Ok(read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| {
            let modified = entry.metadata().ok().and_then(|meta| meta.modified().ok());
            (entry.path(), modified)
        })
        .collect())
}

/// Rewrite an entry, preserving whatever is already on disk for the fields not
/// being changed.
pub fn update_entry(
    root: &Path,
    id: &str,
    date: Option<NaiveDate>,
    text: Option<&str>,
    image: Option<Option<&str>>,
) -> Result<()> {
    let dir = entry_dir(root, id);
    let (mut frontmatter, mut body) = read_entry_file(&dir.join(ENTRY_FILE))?;
    // `created` is never touched: it says when the entry was made, which moving
    // the entry to another day does not change.
    if let Some(date) = date {
        frontmatter.date = Some(date);
    }
    if let Some(text) = text {
        body = text.to_owned();
    }
    if let Some(image) = image {
        frontmatter.image = image.map(str::to_owned);
    }
    write_entry_file(&dir, &frontmatter, &body)
}

/// The entries a project would show, without needing a live [`ProjectStore`].
pub fn read_project(root: &Path) -> Result<Project> {
    ProjectStore::new(root).scan()
}

/// Convenience for the frontend's entry list without exposing `Entry` writes.
pub fn find_entry<'a>(project: &'a Project, id: &str) -> Option<&'a Entry> {
    project.entries.iter().find(|entry| entry.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProjectMeta;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("lazuli-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("should be able to create a temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ts(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).expect("valid test timestamp")
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
    }

    #[test]
    fn frontmatter_splits_at_a_bare_dashes_line() {
        let (yaml, body) = split_frontmatter("---\ncreated: x\n---\n\nHello there.\n")
            .expect("well-formed frontmatter should parse");
        assert_eq!(yaml, "created: x\n");
        assert_eq!(body, "Hello there.");
    }

    #[test]
    fn frontmatter_tolerates_crlf_and_a_bom() {
        let (yaml, body) = split_frontmatter("\u{feff}---\r\ncreated: x\r\n---\r\n\r\nHi.\r\n")
            .expect("a hand-edited file should still parse");
        assert_eq!(yaml.trim_end(), "created: x");
        assert_eq!(body, "Hi.");
    }

    #[test]
    fn a_dashes_line_inside_the_body_is_not_a_fence() {
        // The body's horizontal rule must not be mistaken for the close.
        let (_, body) = split_frontmatter("---\ncreated: x\n---\n\nBefore\n\n---\n\nAfter\n")
            .expect("should parse");
        assert!(body.contains("Before"));
        assert!(body.contains("After"));
    }

    #[test]
    fn missing_frontmatter_is_an_error_not_a_silent_empty_entry() {
        assert!(split_frontmatter("Just a sentence.\n").is_err());
        assert!(split_frontmatter("---\ncreated: x\n").is_err());
    }

    #[test]
    fn a_project_round_trips_through_disk() {
        let dir = TempDir::new("roundtrip");
        create_project(&dir.0, "Woodworking bench", date(2026, 6, 1))
            .expect("should create a project");

        let id = create_entry(&dir.0, date(2026, 7, 9), ts("2026-07-09T21:00:00+02:00"), None)
            .expect("should add entry");
        update_entry(
            &dir.0,
            &id,
            None,
            Some("Finally got the dovetails to close without a gap."),
            None,
        )
        .expect("should update entry");

        let project = read_project(&dir.0).expect("should read the project back");
        assert_eq!(project.meta.name, "Woodworking bench");
        assert_eq!(project.entries.len(), 1);
        let entry = &project.entries[0];
        assert_eq!(entry.text, "Finally got the dovetails to close without a gap.");
        assert_eq!(entry.day_number, 39);
        assert_eq!(entry.image, None);
    }

    #[test]
    fn creating_a_project_twice_in_one_folder_is_refused() {
        let dir = TempDir::new("twice");
        create_project(&dir.0, "First", date(2026, 6, 1)).expect("first should succeed");
        // Overwriting would silently discard the existing name and start date.
        assert!(create_project(&dir.0, "Second", date(2026, 1, 1)).is_err());
        assert_eq!(
            read_meta(&dir.0).expect("meta should still be readable").name,
            "First"
        );
    }

    #[test]
    fn a_project_folder_that_does_not_exist_yet_is_created() {
        let dir = TempDir::new("fresh");
        let root = dir.0.join("Woodworking bench");
        assert_eq!(new_project_problem(&root), None);
        create_project(&root, "Woodworking bench", date(2026, 6, 1))
            .expect("the folder should be created along with the project");
        assert!(root.join(META_FILE).is_file());
    }

    #[test]
    fn a_folder_with_someone_elses_files_in_it_is_refused() {
        let dir = TempDir::new("occupied");
        fs::write(dir.0.join("holiday.jpg"), b"x").expect("should write a test file");
        assert!(new_project_problem(&dir.0).is_some());
        assert!(create_project(&dir.0, "P", date(2026, 6, 1)).is_err());
        // The file that was there is still there, untouched.
        assert!(dir.0.join("holiday.jpg").is_file());
    }

    #[test]
    fn entries_sort_by_day_then_by_when_they_were_written() {
        let dir = TempDir::new("sort");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");

        // Added out of order, with two entries sharing Day 39.
        let later = create_entry(&dir.0, date(2026, 7, 10), ts("2026-07-10T09:00:00+02:00"), None)
            .expect("should add");
        let second = create_entry(&dir.0, date(2026, 7, 9), ts("2026-07-09T22:00:00+02:00"), None)
            .expect("should add");
        let first = create_entry(&dir.0, date(2026, 7, 9), ts("2026-07-09T14:00:00+02:00"), None)
            .expect("should add");

        let project = read_project(&dir.0).expect("should read");
        let days: Vec<i64> = project.entries.iter().map(|e| e.day_number).collect();
        assert_eq!(days, vec![39, 39, 40]);
        // Within Day 39, the one written first comes first.
        let ids: Vec<&str> = project.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec![first, second, later]);
    }

    #[test]
    fn an_entry_is_filed_under_its_date_not_the_hour_it_was_written() {
        let dir = TempDir::new("late-night");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        // Written at 01:00 but marked as the 10th: the file says which day it
        // is about, and nothing re-derives it from the clock.
        create_entry(&dir.0, date(2026, 7, 10), ts("2026-07-10T01:00:00+02:00"), None)
            .expect("should add");

        let project = read_project(&dir.0).expect("should read");
        assert_eq!(project.entries[0].journal_date, date(2026, 7, 10));
    }

    #[test]
    fn an_entry_file_without_a_date_field_still_reads() {
        let dir = TempDir::new("legacy");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");

        // Exactly what the app wrote before entries carried their own date.
        let entry = dir.0.join(ENTRIES_DIR).join("legacy-id");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join(ENTRY_FILE),
            "---\ncreated: 2026-07-10T01:00:00+02:00\nimage: null\n---\n\nStill up.\n",
        )
        .expect("should write");

        let project = read_project(&dir.0).expect("should read");
        // The 5am rule applied to `created`, as it always did for this file.
        assert_eq!(project.entries[0].journal_date, date(2026, 7, 9));
        assert_eq!(project.entries[0].text, "Still up.");
    }

    /// Exactly what the app wrote before projects and entries had identities.
    fn write_a_project_from_before_identity(dir: &Path) {
        fs::write(
            dir.join(META_FILE),
            "name: Old\nstart_date: 2026-06-01\ncover: null\ndate_format: real\nsort_order: newest\n",
        )
        .expect("should write the meta file");
        let entry = dir.join(ENTRIES_DIR).join("legacy-id");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join(ENTRY_FILE),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\nBefore any of this.\n",
        )
        .expect("should write");
    }

    #[test]
    fn a_new_project_is_only_its_own_files() {
        // Everything in a project folder syncs, whatever syncs it, so nothing
        // this machine alone should know may be written there. The one file
        // that used to be, a `.gitignore` for a sync base, went with the base.
        let dir = TempDir::new("new-project-files");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let mut names: Vec<String> = fs::read_dir(&dir.0)
            .expect("should list")
            .map(|entry| entry.expect("should read").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [COVER_DIR, ENTRIES_DIR, META_FILE]);
    }

    #[test]
    fn a_field_a_newer_build_wrote_survives_editing_the_project() {
        // Everyone sharing a folder runs whichever build they have. One that
        // dropped what it did not understand would strip a newer build's fields
        // every time it renamed the project.
        let dir = TempDir::new("meta-rest");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let path = dir.0.join(META_FILE);
        let written = fs::read_to_string(&path).expect("should read");
        fs::write(&path, format!("{written}mood: calm\n")).expect("should write");

        let mut meta = read_meta(&dir.0).expect("should read");
        meta.name = "Renamed".into();
        write_meta(&dir.0, &meta).expect("should write");

        let after = fs::read_to_string(&path).expect("should read");
        assert!(after.contains("name: Renamed"));
        assert!(after.contains("mood: calm"), "{after}");
    }

    #[test]
    fn a_field_a_newer_build_wrote_survives_editing_the_entry() {
        // An older build editing a newer entry used to lose its `author:` this
        // way, and would lose whatever comes after it just the same.
        let dir = TempDir::new("entry-rest");
        let entry = dir.0.join("e");
        fs::create_dir_all(&entry).expect("should create");
        let file = entry.join(ENTRY_FILE);
        fs::write(
            &file,
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\nweather: sunny\n---\n\nBefore.\n",
        )
        .expect("should write");

        let (frontmatter, _) = read_entry_file(&file).expect("should read");
        write_entry_file(&entry, &frontmatter, "After.").expect("should write");

        let after = fs::read_to_string(&file).expect("should read");
        assert!(after.contains("weather: sunny"), "{after}");
        assert!(after.contains("After."));
        // And `created` still round-trips byte for byte: its offset is what
        // keeps an entry on the day it was written.
        assert!(after.contains("created: 2026-06-10T09:00:00+02:00"), "{after}");
    }

    #[test]
    fn opening_a_project_from_before_writes_nothing_into_it() {
        // Nothing is minted or migrated on the way in: in a project kept in a
        // repository, a file the app touched just by opening it is a diff.
        let dir = TempDir::new("open-untouched");
        write_a_project_from_before_identity(&dir.0);
        let meta_file = dir.0.join(META_FILE);
        let entry_file = dir.0.join(ENTRIES_DIR).join("legacy-id").join(ENTRY_FILE);
        let before = (fs::read(&meta_file).unwrap(), fs::read(&entry_file).unwrap());

        read_project(&dir.0).expect("should read");

        assert_eq!(fs::read(&meta_file).unwrap(), before.0);
        assert_eq!(fs::read(&entry_file).unwrap(), before.1);
    }

    #[test]
    fn the_id_the_drive_builds_wrote_goes_at_the_next_save() {
        // A copy made by hand is a second project, and an id travelling with
        // the copy would be the one thing saying otherwise.
        let dir = TempDir::new("retired-id");
        fs::write(
            dir.0.join(META_FILE),
            "id: 1f0e-uuid\nname: Old\nstart_date: 2026-06-01\ncover: null\n",
        )
        .unwrap();
        let mut meta = read_meta(&dir.0).expect("should read");
        meta.name = "Renamed".into();
        write_meta(&dir.0, &meta).expect("should write");
        let after = fs::read_to_string(dir.0.join(META_FILE)).unwrap();
        assert!(after.contains("name: Renamed"));
        assert!(!after.contains("1f0e-uuid"), "{after}");
    }

    #[test]
    fn an_entry_from_before_authors_reads_with_none() {
        let dir = TempDir::new("legacy-author");
        write_a_project_from_before_identity(&dir.0);

        let project = read_project(&dir.0).expect("should read");
        assert_eq!(project.entries[0].author, None);
        // And everything it always showed is unchanged.
        assert_eq!(project.entries[0].journal_date, date(2026, 6, 10));
        assert_eq!(project.entries[0].text, "Before any of this.");
    }

    #[test]
    fn a_new_entry_records_who_wrote_it() {
        let dir = TempDir::new("entry-author");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let created = DateTime::parse_from_rfc3339("2026-06-10T09:00:00+02:00")
            .expect("valid test timestamp");

        create_entry(&dir.0, date(2026, 6, 10), created, Some("author-1"))
            .expect("should create");

        let project = read_project(&dir.0).expect("should read");
        assert_eq!(project.entries[0].author.as_deref(), Some("author-1"));
    }

    #[test]
    fn editing_an_entry_does_not_change_who_wrote_it() {
        // Editing someone else's sentence does not make it yours.
        let dir = TempDir::new("author-kept");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let created = DateTime::parse_from_rfc3339("2026-06-10T09:00:00+02:00")
            .expect("valid test timestamp");
        let id = create_entry(&dir.0, date(2026, 6, 10), created, Some("author-1"))
            .expect("should create");

        update_entry(&dir.0, &id, None, Some("Edited by someone else"), None)
            .expect("should update");

        let project = read_project(&dir.0).expect("should read");
        assert_eq!(project.entries[0].author.as_deref(), Some("author-1"));
        assert_eq!(project.entries[0].text, "Edited by someone else");
    }

    #[test]
    fn an_entry_left_in_two_minds_by_a_merge_stays_on_the_timeline() {
        // The bug this fixes: conflict markers make the file unparseable, the
        // scan skipped it, and the entry left the timeline without a word.
        let dir = TempDir::new("conflict-visible");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let entry = dir.0.join(ENTRIES_DIR).join("conflicted");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join(ENTRY_FILE),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\n\
             <<<<<<< HEAD\nThe roof went on.\n=======\nPut the roof on.\n>>>>>>> theirs\n",
        )
        .expect("should write");

        let project = read_project(&dir.0).expect("should read");

        assert_eq!(project.entries.len(), 1, "the entry is still there");
        let conflict = project.entries[0]
            .conflict
            .as_ref()
            .expect("it should be marked as conflicted");
        assert_eq!(conflict.versions.len(), 2);
        assert_eq!(conflict.versions[0].text, "The roof went on.");
        assert_eq!(conflict.versions[1].text, "Put the roof on.");
        // On its own day, rather than adrift at one end of the timeline.
        assert_eq!(project.entries[0].journal_date, date(2026, 6, 10));
    }

    #[test]
    fn a_second_copy_left_by_a_syncer_is_offered_beside_the_first() {
        let dir = TempDir::new("conflict-sidecar");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let entry = dir.0.join(ENTRIES_DIR).join("two-copies");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join(ENTRY_FILE),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\nMine.\n",
        )
        .expect("should write");
        fs::write(
            entry.join("entry.sync-conflict-20260610-090000-ABCDEFG.md"),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\nTheirs.\n",
        )
        .expect("should write");

        let project = read_project(&dir.0).expect("should read");

        let conflict = project.entries[0]
            .conflict
            .as_ref()
            .expect("should be conflicted");
        assert_eq!(conflict.versions.len(), 2);
        assert_eq!(conflict.versions[0].text, "Mine.");
        assert_eq!(conflict.versions[1].text, "Theirs.");
        // The entry itself still reads normally while it waits to be settled.
        assert_eq!(project.entries[0].text, "Mine.");
    }

    #[test]
    fn an_ordinary_entry_is_not_conflicted() {
        let dir = TempDir::new("conflict-none");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        create_entry(&dir.0, date(2026, 6, 10), ts("2026-06-10T09:00:00+02:00"), None)
            .expect("should add");
        assert!(read_project(&dir.0).expect("should read").entries[0]
            .conflict
            .is_none());
    }

    #[test]
    fn keeping_one_version_leaves_the_entry_settled_and_the_rest_in_the_trash() {
        let dir = TempDir::new("conflict-resolve");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let entry = dir.0.join(ENTRIES_DIR).join("conflicted");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join(ENTRY_FILE),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\n\
             <<<<<<< HEAD\nMine.\n=======\nTheirs.\n>>>>>>> theirs\n",
        )
        .expect("should write");

        let project = read_project(&dir.0).expect("should read");
        let conflict = project.entries[0]
            .conflict
            .clone()
            .expect("should be conflicted");
        crate::conflicts::resolve(&dir.0, &entry, &conflict, 1, None).expect("should resolve");

        let settled = read_project(&dir.0).expect("should read");
        assert_eq!(settled.entries[0].text, "Theirs.");
        assert!(settled.entries[0].conflict.is_none(), "no longer in two minds");
        assert_eq!(settled.entries[0].journal_date, date(2026, 6, 10));
        // The version that lost is recoverable, not gone.
        assert_eq!(crate::trashcan::list(&dir.0).len(), 1);
    }

    #[test]
    fn settling_a_conflict_keeps_the_entry_created_stamp() {
        // It says when the entry was made, which choosing between two sentences
        // does not change — and it is what orders entries sharing a day.
        let dir = TempDir::new("conflict-created");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let entry = dir.0.join(ENTRIES_DIR).join("conflicted");
        fs::create_dir_all(&entry).expect("should create");
        fs::write(
            entry.join(ENTRY_FILE),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\nMine.\n",
        )
        .expect("should write");
        fs::write(
            entry.join("entry (Jules's conflicted copy 2026-06-11).md"),
            "---\ndate: 2026-06-10\ncreated: 2026-06-10T09:00:00+02:00\nimage: null\n---\n\nTheirs.\n",
        )
        .expect("should write");

        let project = read_project(&dir.0).expect("should read");
        let conflict = project.entries[0]
            .conflict
            .clone()
            .expect("should be conflicted");
        crate::conflicts::resolve(&dir.0, &entry, &conflict, 1, None).expect("should resolve");

        let (frontmatter, _) =
            read_entry_file(&entry.join(ENTRY_FILE)).expect("should read");
        assert_eq!(
            frontmatter.created.to_rfc3339(),
            ts("2026-06-10T09:00:00+02:00").to_rfc3339()
        );
        // And the syncer's file is gone from beside it.
        assert!(crate::conflicts::sidecars(&entry).is_empty());
    }

    #[test]
    fn a_malformed_entry_is_skipped_not_fatal() {
        let dir = TempDir::new("malformed");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let good = create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"), None)
            .expect("should add");

        let broken = dir.0.join(ENTRIES_DIR).join("not-a-uuid");
        fs::create_dir_all(&broken).expect("should create");
        fs::write(broken.join(ENTRY_FILE), "no frontmatter here").expect("should write");

        let project = read_project(&dir.0).expect("a broken entry must not fail the scan");
        assert_eq!(project.entries.len(), 1);
        assert_eq!(project.entries[0].id, good);
    }

    #[test]
    fn the_cache_serves_unchanged_files_and_notices_edits() {
        let dir = TempDir::new("cache");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let id = create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"), None)
            .expect("should add");

        let mut store = ProjectStore::new(&dir.0);
        let first = store.scan().expect("first scan");
        let second = store.scan().expect("second scan should hit the cache");
        assert_eq!(first, second);

        // An edit from outside the app must still be picked up. The length
        // changes here, which the cache checks alongside mtime -- mtime alone
        // has too coarse a resolution to rely on within a single test.
        update_entry(&dir.0, &id, None, Some("Edited externally."), None)
            .expect("should update");
        let third = store.scan().expect("third scan");
        assert_eq!(third.entries[0].text, "Edited externally.");
    }

    /// The images beside `entry.md` are cached on the folder's own stamp, not
    /// on the file's, so a picture arriving without the file changing has to
    /// still show up.
    #[test]
    fn an_image_added_beside_an_unchanged_entry_is_noticed() {
        let dir = TempDir::new("images-cache");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let id = create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"), None)
            .expect("should add");
        let folder = entry_dir(&dir.0, &id);

        let mut store = ProjectStore::new(&dir.0);
        assert!(store.scan().expect("first scan").entries[0].images.is_empty());

        fs::write(folder.join("shot.jpg"), b"not really a jpeg").expect("should write");
        let with_image = store.scan().expect("second scan");
        assert_eq!(with_image.entries[0].images, vec!["shot.jpg".to_string()]);

        fs::remove_file(folder.join("shot.jpg")).expect("should remove");
        assert!(store.scan().expect("third scan").entries[0].images.is_empty());
    }

    /// A folder under `entries/` that holds no `entry.md` is not an entry. It
    /// is skipped without comment, where one that *has* a file it cannot read
    /// is worth saying out loud.
    #[test]
    fn a_folder_without_an_entry_file_is_not_an_entry() {
        let dir = TempDir::new("no-entry-file");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"), None)
            .expect("should add");
        fs::create_dir_all(dir.0.join(ENTRIES_DIR).join("not-an-entry"))
            .expect("should create");

        let project = ProjectStore::new(&dir.0).scan().expect("should scan");
        assert_eq!(project.entries.len(), 1);
    }

    /// The point of the cache, asserted directly: nothing else in the suite
    /// would notice it quietly regressing to reading every folder every time,
    /// because the results would all still be right.
    ///
    /// The sleep is the contract, not a workaround: a stamp is not trusted
    /// until it is older than the read by more than any filesystem's timestamp
    /// granularity — see [`SETTLED_AFTER`].
    #[test]
    fn a_settled_project_is_scanned_without_touching_a_single_entry() {
        let dir = TempDir::new("cache-hits");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        for day in 2..6 {
            create_entry(&dir.0, date(2026, 6, day), ts("2026-06-02T10:00:00+02:00"), None)
                .expect("should add");
        }

        let mut store = ProjectStore::new(&dir.0);
        store.scan().expect("first scan");
        assert_eq!(store.read_from_disk(), 4, "a cold scan reads every entry");

        // Once the stamps are old enough to trust, one more scan is still
        // needed: what was cached was cached at a moment those stamps were
        // fresh, so it is that reading which has to be re-dated. That is the
        // "one extra read per folder, once" of `cached_at`.
        std::thread::sleep(SETTLED_AFTER + std::time::Duration::from_millis(100));
        store.scan().expect("settling scan");
        assert_eq!(store.read_from_disk(), 4, "the stamps are re-dated once");

        let before = store.scan().expect("settled scan");
        assert_eq!(
            store.read_from_disk(),
            0,
            "a project nothing has touched costs no reads at all"
        );

        // And the result is the same as the scan that did read everything.
        let mut cold = ProjectStore::new(&dir.0);
        assert_eq!(before, cold.scan().expect("cold scan"));
        assert_eq!(cold.read_from_disk(), 4);
    }

    /// A deleted entry's cache line has to go with it, or a long session of
    /// adding and removing entries grows the map without bound.
    #[test]
    fn a_removed_entry_leaves_nothing_behind_in_the_cache() {
        let dir = TempDir::new("cache-evict");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let id = create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"), None)
            .expect("should add");

        let mut store = ProjectStore::new(&dir.0);
        store.scan().expect("first scan");
        assert_eq!(store.cache.len(), 1);

        fs::remove_dir_all(entry_dir(&dir.0, &id)).expect("should remove");
        let project = store.scan().expect("second scan");
        assert!(project.entries.is_empty());
        assert!(store.cache.is_empty());
    }

    #[test]
    fn writing_meta_preserves_a_chosen_cover() {
        let dir = TempDir::new("cover");
        let mut meta = create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        meta.cover = Some("sunset.jpg".into());
        write_meta(&dir.0, &meta).expect("should write");
        assert_eq!(
            read_meta(&dir.0).expect("should read").cover.as_deref(),
            Some("sunset.jpg")
        );
    }

    /// Turn a project's marker file back into a name an older build wrote.
    fn make_legacy(root: &Path, legacy: &str) {
        fs::rename(root.join(META_FILE), root.join(legacy))
            .expect("should be able to rename the marker file");
    }

    #[test]
    fn a_folder_from_before_either_rename_is_still_a_project() {
        // Every old name, not just the most recent. The app has been renamed
        // twice, and a folder last written before the first rename is exactly
        // what a single-name check would strand.
        for legacy in LEGACY_META_FILES {
            let dir = TempDir::new("legacy-detect");
            create_project(&dir.0, "Old", date(2026, 6, 1)).expect("should create");
            make_legacy(&dir.0, legacy);

            assert!(is_project(&dir.0), "{legacy} should be detected");
            assert_eq!(read_meta(&dir.0).expect("should read").name, "Old");
        }
    }

    #[test]
    fn opening_a_folder_from_before_either_rename_renames_its_marker_file() {
        for legacy in LEGACY_META_FILES {
            let dir = TempDir::new("legacy-migrate");
            create_project(&dir.0, "Old", date(2026, 6, 1)).expect("should create");
            make_legacy(&dir.0, legacy);

            migrate_meta(&dir.0).expect("should migrate");

            assert!(dir.0.join(META_FILE).is_file(), "{legacy} should migrate");
            assert!(!dir.0.join(legacy).exists());
            assert_eq!(read_meta(&dir.0).expect("should read").name, "Old");
        }
    }

    #[test]
    fn migrating_a_folder_holding_both_files_writes_over_neither() {
        let dir = TempDir::new("legacy-both");
        create_project(&dir.0, "Current", date(2026, 6, 1)).expect("should create");
        fs::write(dir.0.join(LEGACY_META_FILES[0]), "name: Stale\nstart_date: 2020-01-01\n")
            .expect("should write the stale file");

        migrate_meta(&dir.0).expect("should be a no-op");

        assert!(dir.0.join(LEGACY_META_FILES[0]).is_file());
        assert_eq!(read_meta(&dir.0).expect("should read").name, "Current");
    }

    #[test]
    fn migrating_a_folder_that_never_had_the_old_name_does_nothing() {
        let dir = TempDir::new("legacy-none");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");

        migrate_meta(&dir.0).expect("should be a no-op");

        assert!(dir.0.join(META_FILE).is_file());
        assert!(LEGACY_META_FILES
            .iter()
            .all(|name| !dir.0.join(name).exists()));
    }

    #[test]
    fn an_unreadable_meta_file_is_an_error_not_a_default_project() {
        let dir = TempDir::new("nometa");
        assert!(read_meta(&dir.0).is_err());
        assert!(!is_project(&dir.0));
    }

    #[test]
    fn meta_round_trips_through_yaml() {
        let meta = ProjectMeta {
            name: "Woodworking bench".into(),
            start_date: date(2026, 6, 1),
            cover: Some("sunset-take2.jpg".into()),
            date_format: DateFormat::Day,
            sort_order: SortOrder::Oldest,
            rest: Default::default(),
        };
        let yaml = serde_yaml::to_string(&meta).expect("should serialise");
        assert!(yaml.contains("start_date: 2026-06-01"));
        let back: ProjectMeta = serde_yaml::from_str(&yaml).expect("should deserialise");
        assert_eq!(meta, back);
    }
}
