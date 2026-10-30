//! The home screen's folder search: listing a folder and matching typed text.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::fuzzy;

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

/// Home screen state: the folder being searched, its entries, the search
/// text, and the selected match.
#[derive(Debug, Clone)]
pub(super) struct Browser {
    pub(super) dir: PathBuf,
    pub(super) entries: Vec<Entry>,
    pub(super) query: String,
    /// Index into [`Browser::matches`].
    pub(super) selected: usize,
}

impl Browser {
    pub(super) fn new(dir: PathBuf, entries: Vec<Entry>) -> Self {
        Self {
            dir,
            entries,
            query: String::new(),
            selected: 0,
        }
    }

    /// Entries matching the search, best first. `..` is offered only while
    /// not searching.
    pub(super) fn matches(&self) -> Vec<&Entry> {
        if self.query.is_empty() {
            return self.entries.iter().collect();
        }
        let candidates: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.kind != EntryKind::Parent)
            .collect();
        fuzzy::ranked(
            candidates.iter().map(|entry| entry.name.as_str()),
            &self.query,
        )
        .into_iter()
        .map(|index| candidates[index])
        .collect()
    }

    pub(super) fn selected_entry(&self) -> Option<&Entry> {
        self.matches().get(self.selected).copied()
    }

    pub(super) fn select_next(&mut self) {
        if self.selected + 1 < self.matches().len() {
            self.selected += 1;
        }
    }

    pub(super) const fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Adds a character to the search and selects the best match.
    pub(super) fn type_char(&mut self, character: char) {
        self.query.push(character);
        self.selected = 0;
    }

    /// Removes the last search character. Returns `false` if the search was
    /// already empty.
    pub(super) fn backspace(&mut self) -> bool {
        let removed = self.query.pop().is_some();
        self.selected = 0;
        removed
    }

    /// Clears the search. Returns `false` if it was already empty.
    pub(super) fn clear_query(&mut self) -> bool {
        let had_query = !self.query.is_empty();
        self.query.clear();
        self.selected = 0;
        had_query
    }

    /// The search text as a path, when it contains a path separator
    /// (relative paths are taken from the current folder).
    pub(super) fn typed_path(&self) -> Option<PathBuf> {
        let text = self.query.trim();
        if !text.contains(['/', std::path::MAIN_SEPARATOR]) {
            return None;
        }
        let path = PathBuf::from(text);
        Some(if path.is_absolute() {
            path
        } else {
            self.dir.join(path)
        })
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

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from("videos").join(name),
            kind,
        }
    }

    fn browser() -> Browser {
        Browser::new(
            PathBuf::from("videos"),
            vec![
                entry("..", EntryKind::Parent),
                entry("trips", EntryKind::Directory),
                entry("bodycam.mp4", EntryKind::Video),
                entry("about bob.mkv", EntryKind::Video),
            ],
        )
    }

    fn match_names(browser: &Browser) -> Vec<&str> {
        browser
            .matches()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    #[test]
    fn without_a_search_everything_is_offered() {
        assert_eq!(
            match_names(&browser()),
            ["..", "trips", "bodycam.mp4", "about bob.mkv"]
        );
    }

    #[test]
    fn typing_filters_and_ranks_and_hides_the_parent() {
        let mut browser = browser();
        browser.type_char('b');
        browser.type_char('o');
        assert_eq!(match_names(&browser), ["bodycam.mp4", "about bob.mkv"]);
        assert_eq!(browser.selected_entry().unwrap().name, "bodycam.mp4");
    }

    #[test]
    fn selection_stays_within_the_matches() {
        let mut browser = browser();
        browser.type_char('b');
        browser.select_previous();
        assert_eq!(browser.selected, 0);
        for _ in 0..5 {
            browser.select_next();
        }
        assert_eq!(browser.selected, browser.matches().len() - 1);
    }

    #[test]
    fn typing_resets_the_selection_to_the_best_match() {
        let mut browser = browser();
        browser.select_next();
        browser.type_char('t');
        assert_eq!(browser.selected, 0);
    }

    #[test]
    fn backspace_and_clear_report_whether_there_was_text() {
        let mut browser = browser();
        assert!(!browser.backspace());
        browser.type_char('x');
        assert!(browser.backspace());
        assert!(browser.query.is_empty());
        browser.type_char('y');
        assert!(browser.clear_query());
        assert!(!browser.clear_query());
    }

    #[test]
    fn text_with_a_separator_is_a_path_from_the_current_folder() {
        let mut browser = browser();
        for character in "trips/day1.mp4".chars() {
            browser.type_char(character);
        }
        assert_eq!(
            browser.typed_path(),
            Some(PathBuf::from("videos").join("trips/day1.mp4"))
        );
        browser.clear_query();
        browser.type_char('a');
        assert_eq!(browser.typed_path(), None);
    }
}
