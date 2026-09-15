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

use crate::model::{is_image, DateFormat, Entry, EntryFrontmatter, Project, ProjectMeta};

pub const META_FILE: &str = "lapis.yaml";

/// What the marker file was called before the app was renamed.
///
/// A folder written by an older build is still a project: it is detected and
/// read, and [`migrate_meta`] gives it the current name the first time it is
/// opened. Delete this and its three uses once no one is running a build from
/// before the rename.
pub const LEGACY_META_FILE: &str = "journaley.yaml";

pub const ENTRIES_DIR: &str = "entries";
pub const COVER_DIR: &str = "cover";
pub const ENTRY_FILE: &str = "entry.md";

/// The delimiter line that opens and closes a frontmatter block.
const FRONTMATTER_FENCE: &str = "---";

/// A project folder plus a parse cache keyed by `entry.md` path.
///
/// The cache is what keeps a full rescan affordable at a thousand-plus entries:
/// directory listings still happen every time, but unchanged files cost a
/// `stat` rather than a YAML parse.
pub struct ProjectStore {
    root: PathBuf,
    cache: HashMap<PathBuf, CachedEntry>,
}

/// A parsed `entry.md` with the file stamps it was parsed from.
struct CachedEntry {
    modified: Option<SystemTime>,
    len: u64,
    frontmatter: EntryFrontmatter,
    text: String,
}

impl ProjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cache: HashMap::new(),
        }
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
        let cover_images = list_images(&self.root.join(COVER_DIR))?;

        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for entry_dir in list_entry_dirs(&self.root)? {
            let id = entry_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            let entry_file = entry_dir.join(ENTRY_FILE);

            let (frontmatter, text) = match self.read_cached(&entry_file) {
                Ok(parsed) => parsed,
                // One malformed or half-written entry must not take the whole
                // project down; skip it and keep going.
                Err(err) => {
                    eprintln!("lapis: skipping {}: {err:#}", entry_file.display());
                    continue;
                }
            };
            seen.insert(entry_file);

            // `created` is paired with the entry only for the sort below; it is
            // dropped before the project leaves this function, because an entry
            // has a day and no time as far as the rest of the app is concerned.
            let created = frontmatter.created;
            let images = list_images(&entry_dir)?;
            entries.push((
                created,
                Project::make_entry(meta.start_date, id, frontmatter, text, images),
            ));
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
        let entries = entries.into_iter().map(|(_, entry)| entry).collect();

        Ok(Project {
            root: self.root.clone(),
            meta,
            entries,
            cover_images,
        })
    }

    /// Parse `entry.md`, reusing the cached parse when the file is untouched.
    fn read_cached(&mut self, path: &Path) -> Result<(EntryFrontmatter, String)> {
        let metadata = fs::metadata(path)
            .with_context(|| format!("reading metadata for {}", path.display()))?;
        let modified = metadata.modified().ok();
        let len = metadata.len();

        if let Some(cached) = self.cache.get(path) {
            // `modified` is `None` on filesystems that do not report it; treat
            // that as "always stale" rather than trusting the length alone.
            if cached.modified.is_some() && cached.modified == modified && cached.len == len {
                return Ok((cached.frontmatter.clone(), cached.text.clone()));
            }
        }

        let (frontmatter, text) = read_entry_file(path)?;
        self.cache.insert(
            path.to_path_buf(),
            CachedEntry {
                modified,
                len,
                frontmatter: frontmatter.clone(),
                text: text.clone(),
            },
        );
        Ok((frontmatter, text))
    }
}

/// Whether a folder looks like a lapis project.
pub fn is_project(root: &Path) -> bool {
    root.join(META_FILE).is_file() || root.join(LEGACY_META_FILE).is_file()
}

/// The marker file this folder actually has, the current name winning if both
/// are somehow there. Falls back to the current name when neither exists, so a
/// caller about to write one gets the right path.
fn meta_path(root: &Path) -> PathBuf {
    let current = root.join(META_FILE);
    if current.is_file() {
        return current;
    }
    let legacy = root.join(LEGACY_META_FILE);
    if legacy.is_file() {
        return legacy;
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
    let legacy = root.join(LEGACY_META_FILE);
    let current = root.join(META_FILE);
    if !legacy.is_file() || current.exists() {
        return Ok(());
    }
    fs::rename(&legacy, &current).with_context(|| {
        format!("renaming {} to {}", legacy.display(), current.display())
    })
}

pub fn read_meta(root: &Path) -> Result<ProjectMeta> {
    let path = meta_path(root);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml::from_str(&contents).with_context(|| format!("parsing {}", path.display()))
}

pub fn write_meta(root: &Path, meta: &ProjectMeta) -> Result<()> {
    let path = root.join(META_FILE);
    let yaml = serde_yaml::to_string(meta).context("serialising project metadata")?;
    fs::write(&path, yaml).with_context(|| format!("writing {}", path.display()))
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
    };
    write_meta(root, &meta)?;
    Ok(meta)
}

/// Create an entry folder with an empty `entry.md`, returning its id.
///
/// `date` is the day the entry is about; `created` is the moment it was made,
/// which is only ever used to order entries that share a day.
pub fn create_entry(
    root: &Path,
    date: NaiveDate,
    created: DateTime<FixedOffset>,
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
    let (yaml, body) = split_frontmatter(&contents)
        .with_context(|| format!("parsing frontmatter in {}", path.display()))?;
    let frontmatter: EntryFrontmatter = serde_yaml::from_str(yaml)
        .with_context(|| format!("parsing frontmatter in {}", path.display()))?;
    Ok((frontmatter, body.to_owned()))
}

pub fn write_entry_file(dir: &Path, frontmatter: &EntryFrontmatter, text: &str) -> Result<()> {
    let yaml = serde_yaml::to_string(frontmatter).context("serialising entry frontmatter")?;
    // `serde_yaml` already ends its output with a newline.
    let contents = format!("{FRONTMATTER_FENCE}\n{yaml}{FRONTMATTER_FENCE}\n\n{text}\n");
    let path = dir.join(ENTRY_FILE);
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
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
fn list_images(dir: &Path) -> Result<Vec<String>> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_image(name))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    Ok(names)
}

/// Every folder under `entries/` that contains an `entry.md`.
fn list_entry_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let dir = root.join(ENTRIES_DIR);
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    Ok(read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join(ENTRY_FILE).is_file())
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
                std::env::temp_dir().join(format!("lapis-{label}-{}", uuid::Uuid::new_v4()));
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

        let id = create_entry(&dir.0, date(2026, 7, 9), ts("2026-07-09T21:00:00+02:00"))
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
        let later = create_entry(&dir.0, date(2026, 7, 10), ts("2026-07-10T09:00:00+02:00"))
            .expect("should add");
        let second = create_entry(&dir.0, date(2026, 7, 9), ts("2026-07-09T22:00:00+02:00"))
            .expect("should add");
        let first = create_entry(&dir.0, date(2026, 7, 9), ts("2026-07-09T14:00:00+02:00"))
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
        create_entry(&dir.0, date(2026, 7, 10), ts("2026-07-10T01:00:00+02:00"))
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

    #[test]
    fn a_malformed_entry_is_skipped_not_fatal() {
        let dir = TempDir::new("malformed");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");
        let good = create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"))
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
        let id = create_entry(&dir.0, date(2026, 6, 2), ts("2026-06-02T10:00:00+02:00"))
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

    /// Turn a project's marker file back into the name an older build wrote.
    fn make_legacy(root: &Path) {
        fs::rename(root.join(META_FILE), root.join(LEGACY_META_FILE))
            .expect("should be able to rename the marker file");
    }

    #[test]
    fn a_folder_from_before_the_rename_is_still_a_project() {
        let dir = TempDir::new("legacy-detect");
        create_project(&dir.0, "Old", date(2026, 6, 1)).expect("should create");
        make_legacy(&dir.0);

        assert!(is_project(&dir.0));
        assert_eq!(read_meta(&dir.0).expect("should read").name, "Old");
    }

    #[test]
    fn opening_a_folder_from_before_the_rename_renames_its_marker_file() {
        let dir = TempDir::new("legacy-migrate");
        create_project(&dir.0, "Old", date(2026, 6, 1)).expect("should create");
        make_legacy(&dir.0);

        migrate_meta(&dir.0).expect("should migrate");

        assert!(dir.0.join(META_FILE).is_file());
        assert!(!dir.0.join(LEGACY_META_FILE).exists());
        assert_eq!(read_meta(&dir.0).expect("should read").name, "Old");
    }

    #[test]
    fn migrating_a_folder_holding_both_files_writes_over_neither() {
        let dir = TempDir::new("legacy-both");
        create_project(&dir.0, "Current", date(2026, 6, 1)).expect("should create");
        fs::write(dir.0.join(LEGACY_META_FILE), "name: Stale\nstart_date: 2020-01-01\n")
            .expect("should write the stale file");

        migrate_meta(&dir.0).expect("should be a no-op");

        assert!(dir.0.join(LEGACY_META_FILE).is_file());
        assert_eq!(read_meta(&dir.0).expect("should read").name, "Current");
    }

    #[test]
    fn migrating_a_folder_that_never_had_the_old_name_does_nothing() {
        let dir = TempDir::new("legacy-none");
        create_project(&dir.0, "P", date(2026, 6, 1)).expect("should create");

        migrate_meta(&dir.0).expect("should be a no-op");

        assert!(dir.0.join(META_FILE).is_file());
        assert!(!dir.0.join(LEGACY_META_FILE).exists());
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
        };
        let yaml = serde_yaml::to_string(&meta).expect("should serialise");
        assert!(yaml.contains("start_date: 2026-06-01"));
        let back: ProjectMeta = serde_yaml::from_str(&yaml).expect("should deserialise");
        assert_eq!(meta, back);
    }
}
