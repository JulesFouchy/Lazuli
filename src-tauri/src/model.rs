//! The shapes that go on disk, and the shapes that go to the frontend.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::dates;

/// `journaley.yaml` at the root of a project folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    /// Day 1. A journal date, so a project begun at 03:00 records the previous
    /// calendar day.
    pub start_date: NaiveDate,
    /// Filename within `cover/`, or `None` when no cover has been chosen. The
    /// other files in `cover/` are attempts that were kept.
    #[serde(default)]
    pub cover: Option<String>,
}

/// The frontmatter block of an `entries/<uuid>/entry.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryFrontmatter {
    /// The journal day this entry is about. The only date the entry has, and
    /// the only one the user edits.
    ///
    /// Optional purely for the entries written before this field existed, whose
    /// day was read off `created` by the 5am rule; [`Self::journal_date`] keeps
    /// meaning the same for both. It is written out from then on.
    #[serde(default)]
    pub date: Option<NaiveDate>,
    /// When the entry was made. RFC 3339 with the offset always present: see
    /// [`crate::dates`].
    ///
    /// Not a time the entry claims to be *about* — it orders entries that share
    /// a day, and dates the ones written before `date` existed. Never edited,
    /// and deliberately never sent to the frontend.
    pub created: DateTime<FixedOffset>,
    /// Filename within the entry folder, or `None` for an entry with no chosen
    /// illustration.
    #[serde(default)]
    pub image: Option<String>,
}

impl EntryFrontmatter {
    /// The journal day the entry belongs to.
    ///
    /// An entry written before `date` existed has its day derived the way it
    /// always was, so an old file keeps the day it has always shown.
    pub fn journal_date(&self) -> NaiveDate {
        self.date.unwrap_or_else(|| dates::journal_date(self.created))
    }
}

/// One entry, as the frontend sees it.
///
/// `day_number` is derived here rather than in the UI so there is exactly one
/// place that counts days. `created` is deliberately absent: an entry has a
/// day and no time, and a field the UI could read is a field the UI will
/// eventually print.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    /// The entry folder's name: a UUID, stable across date edits.
    pub id: String,
    pub text: String,
    /// The chosen image, if the file named in frontmatter still exists.
    pub image: Option<String>,
    /// Every image in the entry folder, sorted, chosen one included.
    pub images: Vec<String>,
    pub journal_date: NaiveDate,
    pub day_number: i64,
}

/// A whole project, read from disk.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Project {
    pub root: PathBuf,
    pub meta: ProjectMeta,
    /// Sorted oldest first by `created`.
    pub entries: Vec<Entry>,
    /// Every file in `cover/`, sorted.
    pub cover_images: Vec<String>,
}

impl Project {
    /// Build an [`Entry`] from its parts, deriving the journal-day fields.
    pub fn make_entry(
        start_date: NaiveDate,
        id: String,
        frontmatter: EntryFrontmatter,
        text: String,
        images: Vec<String>,
    ) -> Entry {
        let journal_date = frontmatter.journal_date();
        // A frontmatter `image` naming a file that has since been deleted from
        // Explorer degrades to "no chosen image" rather than a broken card.
        let image = frontmatter
            .image
            .filter(|name| images.iter().any(|candidate| candidate == name));
        Entry {
            id,
            text,
            image,
            images,
            journal_date,
            day_number: dates::day_number(start_date, journal_date),
        }
    }
}

/// File extensions treated as images when listing an entry's candidates.
///
/// An allowlist rather than "anything that is not entry.md", so a stray
/// `notes.txt` or `Thumbs.db` in a folder does not appear as a broken thumbnail.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "tif", "tiff", "heic", "heif",
];

/// Whether a filename looks like an image we can display.
pub fn is_image(filename: &str) -> bool {
    filename
        .rsplit_once('.')
        .map(|(_, extension)| {
            let extension = extension.to_ascii_lowercase();
            IMAGE_EXTENSIONS.contains(&extension.as_str())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frontmatter(image: Option<&str>) -> EntryFrontmatter {
        EntryFrontmatter {
            date: NaiveDate::from_ymd_opt(2026, 6, 10),
            created: DateTime::parse_from_rfc3339("2026-06-10T09:00:00+02:00")
                .expect("valid test timestamp"),
            image: image.map(str::to_owned),
        }
    }

    fn start() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 1).expect("valid test date")
    }

    #[test]
    fn extensions_are_matched_case_insensitively() {
        assert!(is_image("photo.JPG"));
        assert!(is_image("photo.jpeg"));
        assert!(!is_image("entry.md"));
        assert!(!is_image("Thumbs.db"));
        assert!(!is_image("no-extension"));
    }

    #[test]
    fn a_chosen_image_that_still_exists_is_kept() {
        let entry = Project::make_entry(
            start(),
            "id".into(),
            frontmatter(Some("a.jpg")),
            "text".into(),
            vec!["a.jpg".into(), "b.jpg".into()],
        );
        assert_eq!(entry.image.as_deref(), Some("a.jpg"));
    }

    #[test]
    fn a_chosen_image_deleted_outside_the_app_degrades_to_none() {
        // Deleted in Explorer while the app was open. The card goes imageless
        // rather than pointing at a file that is not there.
        let entry = Project::make_entry(
            start(),
            "id".into(),
            frontmatter(Some("gone.jpg")),
            "text".into(),
            vec!["b.jpg".into()],
        );
        assert_eq!(entry.image, None);
        // The surviving candidate is not silently promoted.
        assert_eq!(entry.images, vec!["b.jpg".to_string()]);
    }

    #[test]
    fn an_explicit_date_is_taken_as_written() {
        // 01:00 with no `date` would file under the 9th under the 5am rule; an
        // entry that says which day it is about is believed instead.
        let frontmatter = EntryFrontmatter {
            date: NaiveDate::from_ymd_opt(2026, 6, 10),
            created: DateTime::parse_from_rfc3339("2026-06-10T01:00:00+02:00")
                .expect("valid test timestamp"),
            image: None,
        };
        assert_eq!(
            frontmatter.journal_date(),
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("valid test date")
        );
    }

    #[test]
    fn an_entry_written_before_date_existed_keeps_the_day_it_always_had() {
        // The whole point of the fallback: files on disk must not shift a day
        // when the app that reads them is upgraded.
        let frontmatter = EntryFrontmatter {
            date: None,
            created: DateTime::parse_from_rfc3339("2026-06-10T01:00:00+02:00")
                .expect("valid test timestamp"),
            image: None,
        };
        assert_eq!(
            frontmatter.journal_date(),
            NaiveDate::from_ymd_opt(2026, 6, 9).expect("valid test date")
        );
    }

    #[test]
    fn the_day_number_comes_from_the_journal_date() {
        let entry = Project::make_entry(
            start(),
            "id".into(),
            frontmatter(None),
            "text".into(),
            vec![],
        );
        assert_eq!(entry.day_number, 10);
    }
}
