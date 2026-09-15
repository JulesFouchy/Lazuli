//! Filename collision handling.
//!
//! Nothing in a project folder is ever overwritten. When a name is taken, the
//! incoming file gets a ` (2)`, ` (3)`, ... suffix and both files survive. Every
//! write that puts a file into a project folder goes through [`unique_path`].

use std::path::{Path, PathBuf};

/// Highest suffix tried before giving up, so a pathological directory cannot
/// spin forever.
const MAX_ATTEMPTS: u32 = 10_000;

/// A path inside `dir` for `filename` that does not collide with anything
/// already there.
///
/// Returns `dir/filename` untouched when it is free, otherwise inserts a
/// counter before the extension: `photo.jpg` becomes `photo (2).jpg`.
///
/// This is inherently racy against another process creating the same name in
/// the window between the check and the write. That is accepted: the competing
/// writer is a human in Explorer, and the watcher reconciles whatever ends up
/// on disk.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let (stem, extension) = split_extension(filename);
    for n in 2..MAX_ATTEMPTS {
        let candidate = dir.join(format!("{stem} ({n}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    // Astronomically unlikely. Fall back to something unmistakably unique
    // rather than returning a path that would overwrite a real file.
    dir.join(format!("{stem} ({}){extension}", uuid::Uuid::new_v4()))
}

/// Characters Windows refuses in a filename. Kept as an explicit list rather
/// than a platform `cfg`: a project folder made on Windows should still be
/// openable when the folder is synced to a Mac, and vice versa.
const ILLEGAL_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Device names Windows still reserves, with or without an extension.
const RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Cap on a derived folder name, so a pasted paragraph does not produce a path
/// long enough to break the tools the user opens the folder with.
const MAX_FOLDER_NAME: usize = 120;

/// Used when a name sanitises away to nothing, e.g. `???`.
const FALLBACK_FOLDER_NAME: &str = "Project";

/// The folder name a project called `name` gets.
///
/// The project's real name lives in `journaley.yaml` and can be anything; this
/// is only the folder it sits in, so it trades exactness for being a name the
/// user can type in a terminal. Illegal characters become spaces rather than
/// being dropped, so `Kitchen/Bathroom` reads as two words instead of one.
pub fn folder_name_for(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if ILLEGAL_CHARS.contains(&character) || character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();

    let mut folder: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // Truncate on a character boundary, then re-trim: cutting mid-word can
    // leave a trailing space, which Windows would silently drop anyway.
    if folder.chars().count() > MAX_FOLDER_NAME {
        folder = folder.chars().take(MAX_FOLDER_NAME).collect();
    }
    // Windows stores neither a trailing dot nor a trailing space.
    let folder = folder.trim_end_matches(['.', ' ']).to_owned();

    if folder.is_empty() {
        return FALLBACK_FOLDER_NAME.to_owned();
    }

    // `CON` and `CON.txt` are both the console device; a trailing underscore is
    // the least surprising way out.
    let stem = folder.split('.').next().unwrap_or(&folder);
    if RESERVED_STEMS
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return format!("{folder}_");
    }
    folder
}

/// Whether `folder` is the folder name `name` would have produced, allowing for
/// the ` (2)` a collision may have added when it was created.
///
/// Asked before a project's folder is renamed to follow its name: a folder the
/// user named themselves, or a repository a journal happens to live in, is not
/// something renaming the journal should move.
pub fn is_named_after(folder: &str, name: &str) -> bool {
    match folder.strip_prefix(&folder_name_for(name)) {
        Some("") => true,
        Some(rest) => rest
            .strip_prefix(" (")
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|digits| {
                !digits.is_empty() && digits.chars().all(|digit| digit.is_ascii_digit())
            }),
        None => false,
    }
}

/// Split a filename into its stem and its extension *including* the dot.
///
/// Hand-rolled rather than using `Path::extension` so that dotfiles and
/// multi-dot names keep their full visible name in the stem: `.gitignore` has
/// no extension, and `archive.tar.gz` yields `archive.tar` + `.gz`.
fn split_extension(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        // A leading dot is part of the name, not an extension separator.
        Some(0) | None => (filename, ""),
        Some(index) => filename.split_at(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch directory that removes itself when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("journaley-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("should be able to create a temp dir");
            Self(path)
        }

        fn touch(&self, name: &str) {
            fs::write(self.0.join(name), b"x").expect("should be able to write a test file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_free_name_is_returned_unchanged() {
        let dir = TempDir::new("free");
        assert_eq!(unique_path(&dir.0, "photo.jpg"), dir.0.join("photo.jpg"));
    }

    #[test]
    fn a_taken_name_gets_a_suffix_before_the_extension() {
        let dir = TempDir::new("taken");
        dir.touch("photo.jpg");
        assert_eq!(unique_path(&dir.0, "photo.jpg"), dir.0.join("photo (2).jpg"));
    }

    #[test]
    fn suffixes_keep_climbing() {
        let dir = TempDir::new("climb");
        dir.touch("photo.jpg");
        dir.touch("photo (2).jpg");
        dir.touch("photo (3).jpg");
        assert_eq!(unique_path(&dir.0, "photo.jpg"), dir.0.join("photo (4).jpg"));
    }

    #[test]
    fn names_without_an_extension_still_work() {
        let dir = TempDir::new("noext");
        dir.touch("README");
        assert_eq!(unique_path(&dir.0, "README"), dir.0.join("README (2)"));
    }

    #[test]
    fn only_the_last_extension_is_split_off() {
        assert_eq!(split_extension("archive.tar.gz"), ("archive.tar", ".gz"));
    }

    #[test]
    fn a_leading_dot_is_part_of_the_name() {
        // Otherwise `.gitignore` would collide as ` (2).gitignore`.
        assert_eq!(split_extension(".gitignore"), (".gitignore", ""));
    }

    #[test]
    fn an_ordinary_name_is_used_as_the_folder_name() {
        assert_eq!(folder_name_for("Woodworking bench"), "Woodworking bench");
    }

    #[test]
    fn illegal_characters_become_spaces_rather_than_vanishing() {
        assert_eq!(folder_name_for("Kitchen/Bathroom"), "Kitchen Bathroom");
        assert_eq!(folder_name_for("Trip: Iceland"), "Trip Iceland");
    }

    #[test]
    fn surrounding_and_repeated_whitespace_is_collapsed() {
        assert_eq!(folder_name_for("  a   b  "), "a b");
    }

    #[test]
    fn trailing_dots_and_spaces_are_dropped() {
        // Windows would drop them itself, leaving the app looking for a folder
        // under a name that is not the one on disk.
        assert_eq!(folder_name_for("Version 2..."), "Version 2");
    }

    #[test]
    fn a_name_that_sanitises_to_nothing_falls_back() {
        assert_eq!(folder_name_for("???"), FALLBACK_FOLDER_NAME);
        assert_eq!(folder_name_for(""), FALLBACK_FOLDER_NAME);
    }

    #[test]
    fn reserved_device_names_are_escaped() {
        assert_eq!(folder_name_for("con"), "con_");
        assert_eq!(folder_name_for("LPT1.old"), "LPT1.old_");
        // Only the exact device names; `console` is a perfectly good folder.
        assert_eq!(folder_name_for("console"), "console");
    }

    #[test]
    fn a_folder_named_after_its_project_is_recognised() {
        assert!(is_named_after("Woodworking bench", "Woodworking bench"));
        // Sanitised the same way it was on the way in.
        assert!(is_named_after("Trip Iceland", "Trip: Iceland"));
        // The suffix a collision added when the folder was created.
        assert!(is_named_after("Woodworking bench (2)", "Woodworking bench"));
    }

    #[test]
    fn a_folder_the_user_named_themselves_is_not() {
        // The journal lives at the root of the repository it is about; renaming
        // the journal must not rename that.
        assert!(!is_named_after("coollab", "Coollab"));
        assert!(!is_named_after("Woodworking bench old", "Woodworking bench"));
        assert!(!is_named_after("Woodworking bench ()", "Woodworking bench"));
        assert!(!is_named_after("Woodworking bench (final)", "Woodworking bench"));
    }

    #[test]
    fn a_very_long_name_is_truncated_without_splitting_a_character() {
        let folder = folder_name_for(&"é".repeat(500));
        assert_eq!(folder.chars().count(), MAX_FOLDER_NAME);
    }
}
