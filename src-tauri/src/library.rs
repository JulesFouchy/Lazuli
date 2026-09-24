//! The projects the launch screen lists, and the tabs they are filed under.
//!
//! The one piece of state that is not in a project folder, because it is about
//! the app rather than any one project. A project folder does not know which
//! tab it is in — filing is the user's view of their own machine, and the same
//! folder on a second machine may well belong somewhere else.
//!
//! The order is the user's and nothing else touches it. Opening a project does
//! not move it, and there is no cap on the list: both would quietly undo an
//! arrangement made by hand.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::atomic;

const LIBRARY_FILE: &str = "projects.json";

/// The list as it was before tabs: a flat array of paths, most recently opened
/// first. Read once, when there is no `projects.json` yet, and then left where
/// it is — nothing is deleted without being asked for, and an old build reading
/// it back is a better outcome than an old build finding nothing.
const LEGACY_FILE: &str = "recent.json";

/// What the first tab is called until it is renamed.
const FIRST_TAB: &str = "Projects";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tab {
    pub name: String,
    pub projects: Vec<PathBuf>,
    /// Fields this build does not know, carried through a rewrite untouched,
    /// so that an older build filing a project cannot strip what a newer one
    /// added to the tab.
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Library {
    pub tabs: Vec<Tab>,
    /// As [`Tab::rest`], for the list as a whole.
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// Where a project sits: which tab, and where in it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Slot {
    pub tab: usize,
    pub index: usize,
}

impl Library {
    /// The invariants every read and every write ends on.
    ///
    /// There is always a tab, because the first one is where anything with
    /// nowhere else to go lands; and a project is in exactly one tab, because
    /// a row in two places is a row that can be dragged away from itself.
    fn tidy(&mut self) {
        if self.tabs.is_empty() {
            self.tabs.push(Tab {
                name: FIRST_TAB.to_string(),
                ..Tab::default()
            });
        }
        let mut seen = HashSet::new();
        for tab in &mut self.tabs {
            tab.projects.retain(|path| seen.insert(path.clone()));
        }
    }

    pub fn slot_of(&self, path: &Path) -> Option<Slot> {
        self.tabs.iter().enumerate().find_map(|(tab, entry)| {
            entry
                .projects
                .iter()
                .position(|candidate| candidate == path)
                .map(|index| Slot { tab, index })
        })
    }

    /// Take a project out of whichever tab holds it, saying where it was.
    pub fn remove(&mut self, path: &Path) -> Option<Slot> {
        let slot = self.slot_of(path)?;
        self.tabs[slot.tab].projects.remove(slot.index);
        Some(slot)
    }

    /// Put a project at a slot, taking it out of wherever it was first.
    ///
    /// Both ends of a drag go through here: moving a row within its tab and
    /// moving it to another one differ only in the `tab` asked for.
    pub fn insert(&mut self, path: PathBuf, slot: Slot) {
        self.remove(&path);
        let tab = slot.tab.min(self.tabs.len() - 1);
        let projects = &mut self.tabs[tab].projects;
        let index = slot.index.min(projects.len());
        projects.insert(index, path);
    }

    /// Make sure a project is listed, without moving one that already is.
    ///
    /// What opening a project does. A folder named on the command line has
    /// never been filed, so it goes to the top of the first tab; one that is
    /// already somewhere stays exactly where the user put it.
    pub fn ensure(&mut self, path: &Path) -> bool {
        if self.slot_of(path).is_some() {
            return false;
        }
        self.tabs[0].projects.insert(0, path.to_path_buf());
        true
    }

    /// Follow a project whose folder has moved, keeping its slot.
    ///
    /// Renaming a project renames its folder, and the row should not be
    /// re-filed for it: the same project is in the same tab, at the same
    /// height, under a different path.
    pub fn replace_path(&mut self, old: &Path, new: PathBuf) {
        match self.slot_of(old) {
            Some(slot) => {
                self.remove(&new);
                // Re-read: removing `new` from elsewhere may have shifted it.
                let slot = self.slot_of(old).unwrap_or(slot);
                self.tabs[slot.tab].projects[slot.index] = new;
            }
            None => {
                self.ensure(&new);
            }
        }
    }

    /// Add a tab at the end, returning where it landed.
    pub fn add_tab(&mut self, name: String) -> usize {
        self.tabs.push(Tab {
            name,
            ..Tab::default()
        });
        self.tabs.len() - 1
    }

    pub fn rename_tab(&mut self, index: usize, name: String) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.name = name;
        }
    }

    /// Remove a tab, sending whatever was filed under it to the first tab.
    ///
    /// The first tab itself cannot go: it is where everything homeless ends up,
    /// so there has to be one. Which tab that is follows the strip rather than
    /// being fixed to a particular tab — drag another one to the front and it
    /// becomes the home, and the tab that used to be first can then be deleted
    /// like any other.
    pub fn remove_tab(&mut self, index: usize) -> Option<Tab> {
        if index == 0 || index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        self.tabs[0].projects.extend(tab.projects.iter().cloned());
        Some(tab)
    }

    /// Put a removed tab back where it was, projects and all.
    pub fn insert_tab(&mut self, index: usize, tab: Tab) {
        for path in &tab.projects {
            self.remove(path);
        }
        let index = index.clamp(1, self.tabs.len());
        self.tabs.insert(index, tab);
    }

    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to.min(self.tabs.len()), tab);
    }
}

fn library_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(LIBRARY_FILE))
}

/// Held across every read-change-write of the list.
///
/// Commands run on more than one thread, and two changes interleaved would end
/// with the second written from a copy that predates the first.
static LIBRARY: Mutex<()> = Mutex::new(());

/// The list on disk, or why it cannot be had.
///
/// No `projects.json` is a first launch — or one from before tabs — and reads
/// as whatever the flat list held. A file that is there and cannot be read or
/// parsed is **not** that: read as empty, the next change would write the
/// emptiness back over every tab the user had arranged. So it is an error,
/// retried briefly because on Windows a file being renamed over is unopenable
/// for the duration.
fn load(app: &AppHandle) -> anyhow::Result<Library> {
    use anyhow::Context;

    let path = library_file(app).ok_or_else(|| anyhow::anyhow!("the app has no config folder"))?;
    let mut attempts = 0;
    loop {
        let read = match fs::read_to_string(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let mut library = from_legacy(app);
                library.tidy();
                return Ok(library);
            }
            read => read
                .with_context(|| format!("reading {}", path.display()))
                .and_then(|text| {
                    serde_json::from_str::<Library>(&text)
                        .with_context(|| format!("parsing {}", path.display()))
                }),
        };
        match read {
            Err(err) if attempts < 5 => {
                attempts += 1;
                eprintln!("lazuli: project list unreadable, retrying: {err:#}");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(err) => return Err(err),
            Ok(mut library) => {
                library.tidy();
                return Ok(library);
            }
        }
    }
}

/// The list for showing. A list that cannot be read shows as empty for that
/// one paint, which is harmless because nothing reads it this way and then
/// writes it back.
pub fn read(app: &AppHandle) -> Library {
    load(app).unwrap_or_else(|_| {
        let mut library = Library::default();
        library.tidy();
        library
    })
}

/// Everything the flat recents list held, as a single tab.
fn from_legacy(app: &AppHandle) -> Library {
    let paths: Vec<PathBuf> = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(LEGACY_FILE))
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    Library {
        tabs: vec![Tab {
            name: FIRST_TAB.to_string(),
            projects: paths,
            ..Tab::default()
        }],
        ..Library::default()
    }
}

fn write(app: &AppHandle, library: &Library) {
    let Some(path) = library_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(library) {
        let _ = atomic::write(&path, text);
    }
}

/// Read, change, write. Every mutation goes through here so that none of them
/// can write a list the invariants do not hold for.
///
/// A list that could not be read is not written: the change is made to a copy
/// so the caller still gets an answer, and is lost, which is a thing the user
/// can do again — where writing it would lose every tab they had.
pub fn update<T>(app: &AppHandle, change: impl FnOnce(&mut Library) -> T) -> T {
    let _held = LIBRARY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Ok(mut library) = load(app).inspect_err(|err| {
        eprintln!("lazuli: project list not saved: {err:#}");
    }) else {
        return change(&mut read(app));
    };
    let outcome = change(&mut library);
    library.tidy();
    write(app, &library);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Library {
        let mut library = Library {
            tabs: vec![
                Tab {
                    name: "Projects".into(),
                    projects: vec![PathBuf::from("/a"), PathBuf::from("/b")],
                    ..Tab::default()
                },
                Tab {
                    name: "Done".into(),
                    projects: vec![PathBuf::from("/c")],
                    ..Tab::default()
                },
            ],
            ..Library::default()
        };
        library.tidy();
        library
    }

    #[test]
    fn a_field_a_newer_build_wrote_survives_a_rewrite() {
        let text = r#"{ "tabs": [ { "name": "Projects", "projects": [], "colour": "blue" } ], "pinned": ["/a"] }"#;
        let library: Library = serde_json::from_str(text).expect("should parse");
        let back: serde_json::Value =
            serde_json::to_value(&library).expect("should serialise");
        assert_eq!(back["tabs"][0]["colour"], "blue");
        assert_eq!(back["pinned"][0], "/a");
    }

    #[test]
    fn a_project_is_in_one_tab_only() {
        let mut library = library();
        library.insert(PathBuf::from("/a"), Slot { tab: 1, index: 0 });
        assert_eq!(library.tabs[0].projects, vec![PathBuf::from("/b")]);
        assert_eq!(
            library.tabs[1].projects,
            vec![PathBuf::from("/a"), PathBuf::from("/c")]
        );
    }

    #[test]
    fn moving_a_row_down_its_own_tab_lands_where_asked() {
        let mut library = library();
        library.insert(PathBuf::from("/a"), Slot { tab: 0, index: 1 });
        assert_eq!(
            library.tabs[0].projects,
            vec![PathBuf::from("/b"), PathBuf::from("/a")]
        );
    }

    #[test]
    fn opening_a_filed_project_does_not_move_it() {
        let mut library = library();
        assert!(!library.ensure(Path::new("/c")));
        assert_eq!(library.slot_of(Path::new("/c")).unwrap().tab, 1);
    }

    #[test]
    fn a_renamed_folder_keeps_its_place() {
        let mut library = library();
        library.replace_path(Path::new("/c"), PathBuf::from("/c2"));
        assert_eq!(library.tabs[1].projects, vec![PathBuf::from("/c2")]);
    }

    #[test]
    fn a_deleted_tab_hands_its_projects_to_the_first() {
        let mut library = library();
        let gone = library.remove_tab(1).unwrap();
        assert_eq!(library.tabs.len(), 1);
        assert_eq!(
            library.tabs[0].projects,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c")
            ]
        );
        library.insert_tab(1, gone);
        assert_eq!(library.tabs[1].projects, vec![PathBuf::from("/c")]);
        assert_eq!(
            library.tabs[0].projects,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn the_first_tab_cannot_be_deleted() {
        let mut library = library();
        assert!(library.remove_tab(0).is_none());
        assert_eq!(library.tabs.len(), 2);
    }
}
