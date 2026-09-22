//! Downscaled copies of a project's images.
//!
//! A card is at most ~440px tall and a picker thumbnail ~104px square, but the
//! browser decodes a full-resolution photo for each. `loading="lazy"` keeps the
//! offscreen ones from being decoded at all, so the cost only shows up while
//! scrolling — which is exactly when it is most noticeable.
//!
//! A thumbnail mirrors its original's place in the project, with a `.jpg`
//! extension:
//!
//! ```text
//! entries/<uuid>/IMG_4821.heic  ->  .lazuli-thumbs/entries/<uuid>/IMG_4821.jpg
//! cover/banner.png              ->  .lazuli-thumbs/cover/banner.jpg
//! ```
//!
//! **Named after the path rather than the contents**, which is worth saying
//! because the obvious objection is that a modification time is not stable
//! across machines. Neither it is, and it is not used as the name: a relative
//! path *is* the same on every machine, so both ends of a sync agree on what a
//! thumbnail is called without either having to read a single photograph. The
//! alternative, hashing every image to name it, would mean reading every
//! photograph in the project before the timeline could ask for one, or a second
//! index on disk to avoid that.
//!
//! It is safe here because **an image is written once and never edited**: a new
//! picture is a new file beside the old one, so a name that is taken always
//! means the same bytes. A file replaced from outside the app is caught anyway,
//! by the modification time, which *is* a fair question to ask of a local file.
//!
//! **They belong to the project, not to the machine.** They are derived, and
//! anyone holding the original can rebuild them, but they are worth carrying: a
//! phone opening a shared project should not have to fetch full-size photos to
//! make its own.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::atomic;
use crate::model::is_image;
use crate::store::{COVER_DIR, ENTRIES_DIR};

/// Where thumbnails live inside a project.
pub const THUMBS_DIR: &str = ".lazuli-thumbs";

/// The long edge of a thumbnail, in pixels.
///
/// One size for both the card and the picker: a card's picture is at most about
/// 440px tall and a picker thumbnail about 104px square, and 880 covers the
/// larger of those on a 2× screen. A second, smaller size would save little —
/// the picker's are the cheap ones — and would double what a shared project has
/// to carry.
const LONG_EDGE: u32 = 880;

/// JPEG quality: high enough to look no worse at the size it is shown, low
/// enough to be a small fraction of the original.
const QUALITY: u8 = 82;

/// Where `image`'s thumbnail goes, given the project it is in.
///
/// `None` for a path outside the project, or one that is not valid UTF-8.
pub fn path_for(root: &Path, image: &Path) -> Option<PathBuf> {
    let relative = image.strip_prefix(root).ok()?;
    Some(root.join(THUMBS_DIR).join(relative).with_extension("jpg"))
}

/// Make `image`'s thumbnail unless a current one is already there.
///
/// Returns whether one was written. Never an error: a thumbnail is an
/// optimisation, and a picture in a format nothing here can decode — `heic` and
/// `avif` are in `IMAGE_EXTENSIONS` and are not among the formats built in — is
/// shown at full size instead, which is what the app did for all of them until
/// now.
pub fn build_one(root: &Path, image: &Path) -> bool {
    let Some(destination) = path_for(root, image) else {
        return false;
    };
    if is_current(image, &destination) {
        return false;
    }
    let Ok(bytes) = fs::read(image) else {
        return false;
    };
    write_thumbnail(&destination, &bytes).is_ok()
}

/// Whether `thumbnail` is there and no older than the picture it is of.
///
/// The modification time is only ever asked of two files on the same machine,
/// which is a question it can answer. A picture replaced from outside the app
/// under the same name is the case this catches; the app itself never does it.
fn is_current(image: &Path, thumbnail: &Path) -> bool {
    let Ok(made) = modified(thumbnail) else {
        return false;
    };
    modified(image).is_ok_and(|source| made >= source)
}

fn modified(path: &Path) -> std::io::Result<SystemTime> {
    fs::metadata(path)?.modified()
}

/// Decode, downscale and write one thumbnail.
fn write_thumbnail(destination: &Path, bytes: &[u8]) -> Result<()> {
    let source = image::load_from_memory(bytes).context("decoding the image")?;

    // `thumbnail` scales *up* as readily as down, and a small picture blown up
    // to the long edge would be a thumbnail larger than its own original in
    // both pixels and bytes. One already inside the box is re-encoded as it is.
    let small = if source.width().max(source.height()) > LONG_EDGE {
        // `thumbnail` rather than `resize`: the cheap filter, which is what a
        // picture shown at a fraction of its size wants.
        source.thumbnail(LONG_EDGE, LONG_EDGE)
    } else {
        source
    };

    let mut encoded = Vec::new();
    // Always JPEG, whatever came in: a thumbnail is a photograph shown small,
    // and a PNG of one is several times the size for no visible gain.
    // Transparency is lost, which the card's own backing already supplied.
    small
        .to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut encoded,
            QUALITY,
        ))
        .context("encoding the thumbnail")?;

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    atomic::write(destination, &encoded)
}

/// Bring the whole project's thumbnails up to date, and sweep away any whose
/// picture has gone.
///
/// Returns how many were written. Runs off the main thread — it decodes every
/// picture that has no thumbnail yet, which on a project of real photographs is
/// seconds of work the first time and nothing on every open after that.
pub fn build_all(root: &Path) -> usize {
    let images = originals(root);
    let made = images
        .iter()
        .filter(|image| build_one(root, image))
        .count();
    forget_orphans(root, &images);
    made
}

/// Remove thumbnails no image in the project points at any more.
///
/// The one place a thumbnail is deleted, and it is safe to: it is derived, and
/// whoever still has the original can rebuild it. Deleting an entry would
/// otherwise leave its pictures' thumbnails behind for good.
fn forget_orphans(root: &Path, images: &[PathBuf]) -> usize {
    let wanted: HashSet<PathBuf> = images
        .iter()
        .filter_map(|image| path_for(root, image))
        .collect();
    let mut removed = 0;
    for thumbnail in under(&root.join(THUMBS_DIR)) {
        if !wanted.contains(&thumbnail) && fs::remove_file(&thumbnail).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Every image in the project: each entry's, and every cover.
fn originals(root: &Path) -> Vec<PathBuf> {
    let mut found = files_in(&root.join(COVER_DIR));
    if let Ok(entries) = fs::read_dir(root.join(ENTRIES_DIR)) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            if entry.path().is_dir() {
                found.extend(files_in(&entry.path()));
            }
        }
    }
    found
}

/// The images directly inside one folder.
fn files_in(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_image)
        })
        .collect()
}

/// Every file anywhere beneath `directory`.
fn under(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("lazuli-{label}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("should be able to create a temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A PNG of `size`×`size`, written where a real photo would be.
    fn write_png(path: &Path, size: u32) {
        let mut picture = image::RgbImage::new(size, size);
        for (x, y, pixel) in picture.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("should create");
        }
        picture.save(path).expect("should write the test png");
    }

    #[test]
    fn a_thumbnail_mirrors_its_original_as_a_jpg() {
        // The name is what both ends of a sync have to agree on without reading
        // a single photograph.
        let dir = TempDir::new("thumb-path");
        let image = dir.0.join("entries").join("an-id").join("IMG_4821.heic");
        assert_eq!(
            path_for(&dir.0, &image).expect("inside the project"),
            dir.0
                .join(THUMBS_DIR)
                .join("entries")
                .join("an-id")
                .join("IMG_4821.jpg")
        );
    }

    #[test]
    fn a_big_picture_is_brought_down_to_the_long_edge() {
        let dir = TempDir::new("thumb-big");
        let image = dir.0.join("cover").join("banner.png");
        write_png(&image, 1400);

        assert!(build_one(&dir.0, &image));

        let made = image::open(path_for(&dir.0, &image).expect("inside")).expect("should read");
        assert_eq!(made.width().max(made.height()), LONG_EDGE);
        // Pixels, not bytes. How many bytes a thumbnail takes depends on the
        // picture — a synthetic gradient is a case where a PNG of the original
        // beats a JPEG of the smaller copy — and the saving that matters is a
        // real photograph's, which the manual check measures.
    }

    #[test]
    fn a_picture_already_smaller_than_the_long_edge_is_left_at_its_size() {
        let dir = TempDir::new("thumb-small");
        let image = dir.0.join("cover").join("small.png");
        write_png(&image, 120);

        build_one(&dir.0, &image);

        let made = image::open(path_for(&dir.0, &image).expect("inside")).expect("should read");
        assert_eq!((made.width(), made.height()), (120, 120));
    }

    #[test]
    fn a_thumbnail_that_is_already_current_is_not_built_again() {
        let dir = TempDir::new("thumb-reuse");
        let image = dir.0.join("cover").join("banner.png");
        write_png(&image, 400);

        assert!(build_one(&dir.0, &image), "the first one is written");
        assert!(!build_one(&dir.0, &image), "the second is not");
    }

    #[test]
    fn a_picture_replaced_from_outside_gets_a_fresh_thumbnail() {
        // The app never rewrites an image in place, but Explorer can.
        let dir = TempDir::new("thumb-stale");
        let image = dir.0.join("cover").join("banner.png");
        write_png(&image, 400);
        build_one(&dir.0, &image);

        let thumbnail = path_for(&dir.0, &image).expect("inside");
        let before = fs::metadata(&thumbnail)
            .and_then(|meta| meta.modified())
            .expect("should stat");
        // Far enough ahead that a coarse filesystem clock still sees it.
        let later = before + std::time::Duration::from_secs(10);
        write_png(&image, 500);
        // Opened for writing: Windows refuses to set a time through a
        // read-only handle.
        fs::OpenOptions::new()
            .write(true)
            .open(&image)
            .and_then(|file| file.set_modified(later))
            .expect("should touch");

        assert!(build_one(&dir.0, &image), "the picture changed under it");
    }

    #[test]
    fn a_file_that_cannot_be_decoded_gets_no_thumbnail() {
        // A `heic` from a phone, or a file that is not a picture at all: the
        // page shows the original instead.
        let dir = TempDir::new("thumb-undecodable");
        let image = dir.0.join("cover").join("not-really.png");
        fs::create_dir_all(image.parent().expect("has a parent")).expect("should create");
        fs::write(&image, b"this is not a png").expect("should write");

        assert!(!build_one(&dir.0, &image));
        assert!(!path_for(&dir.0, &image).expect("inside").exists());
    }

    #[test]
    fn covers_and_entry_images_are_both_built() {
        let dir = TempDir::new("thumb-all");
        write_png(&dir.0.join("entries").join("one").join("photo.png"), 200);
        write_png(&dir.0.join("entries").join("two").join("photo.png"), 210);
        write_png(&dir.0.join("cover").join("banner.png"), 220);

        assert_eq!(build_all(&dir.0), 3);
        assert_eq!(build_all(&dir.0), 0, "nothing left to do");
    }

    #[test]
    fn a_thumbnail_whose_picture_has_gone_is_swept_up() {
        let dir = TempDir::new("thumb-orphan");
        let image = dir.0.join("entries").join("one").join("photo.png");
        write_png(&image, 200);
        build_all(&dir.0);
        let thumbnail = path_for(&dir.0, &image).expect("inside");
        assert!(thumbnail.is_file());

        fs::remove_dir_all(image.parent().expect("has a parent")).expect("should remove");
        build_all(&dir.0);

        assert!(!thumbnail.exists());
    }
}
