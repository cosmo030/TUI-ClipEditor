//! The file browser shown when no video was given on the command line.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File extensions (lowercase) offered in the browser. ffmpeg reads many
/// more formats; any file can still be opened from the command line.
const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "avi", "flv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "ogv", "ts",
    "webm", "wmv",
];

/// One row in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) kind: EntryKind,
}

/// What a browser row points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntryKind {
    /// The parent directory (`..`).
    Parent,
    /// A subdirectory.
    Directory,
    /// A file with a video extension.
    Video,
}

/// Browser state: the current directory and the selected row.
#[derive(Debug, Clone)]
pub(super) struct Browser {
    pub(super) dir: PathBuf,
    pub(super) entries: Vec<Entry>,
    pub(super) selected: usize,
}

impl Browser {
    pub(super) fn new(dir: PathBuf, entries: Vec<Entry>) -> Self {
        Self {
            dir,
            entries,
            selected: 0,
        }
    }

    pub(super) fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    pub(super) fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    pub(super) const fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
}

/// Whether `path` has one of the browser's video extensions.
pub(super) fn is_video_file(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .is_some_and(|extension| VIDEO_EXTENSIONS.contains(&extension.as_str()))
}

/// Lists `dir`: a parent entry (unless at the root), then subdirectories,
/// then video files, each sorted by name ignoring case. Hidden entries
/// (names starting with `.`) are skipped.
///
/// # Errors
///
/// Returns the I/O error if the directory cannot be read.
pub(super) fn read_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // Follows symlinks, so a link to a folder is browsable.
        let kind = if path.is_dir() {
            EntryKind::Directory
        } else if is_video_file(&path) {
            EntryKind::Video
        } else {
            continue;
        };
        entries.push(Entry { name, path, kind });
    }
    entries.sort_by(compare_entries);
    if let Some(parent) = dir.parent() {
        entries.insert(
            0,
            Entry {
                name: "..".to_owned(),
                path: parent.to_path_buf(),
                kind: EntryKind::Parent,
            },
        );
    }
    Ok(entries)
}

fn compare_entries(a: &Entry, b: &Entry) -> Ordering {
    a.kind
        .cmp(&b.kind)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn video_extensions_match_case_insensitively() {
        assert!(is_video_file(Path::new("a/Holiday.MP4")));
        assert!(is_video_file(Path::new("clip.webm")));
        assert!(!is_video_file(Path::new("notes.txt")));
        assert!(!is_video_file(Path::new("mp4")));
    }

    #[test]
    fn read_entries_lists_parent_then_folders_then_videos() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("zeta")).unwrap();
        fs::create_dir(dir.path().join("Alpha")).unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        for file in ["b.mkv", "A.mp4", "readme.txt", ".secret.mp4"] {
            fs::write(dir.path().join(file), "").unwrap();
        }

        let entries = read_entries(dir.path()).unwrap();

        assert_eq!(names(&entries), ["..", "Alpha", "zeta", "A.mp4", "b.mkv"]);
        assert_eq!(entries[0].kind, EntryKind::Parent);
        assert_eq!(entries[1].kind, EntryKind::Directory);
        assert_eq!(entries[3].kind, EntryKind::Video);
        assert_eq!(entries[3].path, dir.path().join("A.mp4"));
    }

    #[test]
    fn read_entries_reports_missing_directories() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert_eq!(
            read_entries(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn selection_stays_within_the_list() {
        let entry = |name: &str| Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            kind: EntryKind::Video,
        };
        let mut browser = Browser::new(PathBuf::from("."), vec![entry("a.mp4"), entry("b.mp4")]);
        browser.select_previous();
        assert_eq!(browser.selected, 0);
        browser.select_next();
        browser.select_next();
        assert_eq!(browser.selected_entry().unwrap().name, "b.mp4");
    }
}
