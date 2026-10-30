//! Application state and its update function.
//!
//! [`App::update`] takes one [`Msg`] (a key press, a decoded frame, export
//! progress, …) and returns the [`Effect`]s the runtime must perform. It does
//! no I/O itself, so the whole UI flow is testable without a terminal,
//! ffmpeg, or a clock.

use std::path::PathBuf;
use std::time::Duration;

use image::RgbImage;
use ratatui::crossterm::event::{KeyEvent, KeyEventKind};

use super::browser::{Browser, Entry, EntryKind};
use super::keys::{self, HomeAction, PaletteKey};
use super::palette::{EDITOR_COMMANDS, HOME_COMMANDS, Palette, PaletteAction};
use super::playback::{PlaybackRequest, StreamedFrame};
use super::session::{Leave, Overlay, Session};
use super::theme::Theme;
use crate::clip::Clip;
use crate::export::{ExportJob, ExportMode};
use crate::ffmpeg::ExportOutcome;
use crate::media::MediaInfo;
use crate::timecode::Timestamp;

/// Something that happened, delivered to [`App::update`].
#[derive(Debug)]
pub(crate) enum Msg {
    /// A key was pressed (release events are ignored).
    Key(KeyEvent),
    /// Wall-clock time passed while playing.
    Tick(Duration),
    /// The preview area changed; the largest frame size, in pixels, it shows.
    PreviewBounds((u32, u32)),
    /// A preview frame finished decoding.
    FrameDecoded(DecodedFrame),
    /// The playback stream delivered a frame (or failed).
    StreamFrame(StreamedFrame),
    /// The decoded frame could not be encoded for the terminal.
    PreviewFailed(String),
    /// A video finished probing.
    Opened {
        path: PathBuf,
        result: Result<MediaInfo, String>,
    },
    /// A directory finished listing.
    DirectoryListed {
        dir: PathBuf,
        result: Result<Vec<Entry>, String>,
    },
    /// Progress of a running export.
    Export(ExportEvent),
}

/// Result of an [`Effect::RequestFrame`].
#[derive(Debug)]
pub(crate) struct DecodedFrame {
    pub(crate) generation: u64,
    pub(crate) at: Timestamp,
    pub(crate) size: (u32, u32),
    pub(crate) image: Result<RgbImage, String>,
}

/// Events from the export worker, in order.
#[derive(Debug)]
pub(crate) enum ExportEvent {
    /// The jobs were planned and the first one is starting.
    Started(Vec<ExportJob>),
    /// Job `index` has written `written` of its clip.
    Progress { index: usize, written: Timestamp },
    /// Job `index` ended.
    JobDone {
        index: usize,
        result: Result<ExportOutcome, String>,
    },
    /// All jobs ended (or the rest were skipped after a cancel).
    Finished,
}

/// Work the runtime performs on the app's behalf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    Quit,
    ListDirectory(PathBuf),
    /// Open a video (or, for a folder, show it on the home screen).
    Open(PathBuf),
    RequestFrame(FrameRequest),
    /// Feed the preview from a playback stream (replacing any running one).
    StartPlayback(PlaybackRequest),
    /// Stop the playback stream.
    StopPlayback,
    StartExport(ExportRequest),
    CancelExport,
}

/// Decode the frame at `at` of `path`, scaled to `size`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameRequest {
    /// Identifies the opened file, so frames from a previous file are dropped.
    pub(crate) generation: u64,
    pub(crate) path: PathBuf,
    pub(crate) at: Timestamp,
    pub(crate) size: (u32, u32),
}

/// Plan and run the export of `clips`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportRequest {
    pub(crate) input: PathBuf,
    pub(crate) output_dir: PathBuf,
    pub(crate) clips: Vec<Clip>,
    pub(crate) mode: ExportMode,
}

/// A message on the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) text: String,
    pub(crate) is_error: bool,
}

impl Status {
    pub(crate) fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub(crate) fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// Which screen is shown.
#[derive(Debug)]
pub(crate) enum Screen {
    /// Waiting for the first directory listing.
    Starting,
    Browser(Browser),
    Editing(Box<Session>),
}

/// The whole UI state.
#[derive(Debug)]
pub(crate) struct App {
    screen: Screen,
    /// `--output-dir`, if given; otherwise each video's own directory is used.
    output_dir: Option<PathBuf>,
    status: Option<Status>,
    preview_bounds: (u32, u32),
    next_generation: u64,
    /// The directory the browser came from, to reselect it after going up.
    previous_dir: Option<PathBuf>,
    /// The colours in use (switchable from the command palette).
    theme: Theme,
    /// The open command palette, over either screen.
    palette: Option<Palette>,
}

impl App {
    /// Starts with the file browser in `dir`.
    pub(crate) fn browsing(dir: PathBuf, output_dir: Option<PathBuf>) -> (Self, Vec<Effect>) {
        let app = Self::new(Screen::Starting, output_dir);
        (app, vec![Effect::ListDirectory(dir)])
    }

    /// Starts editing an already probed video.
    pub(crate) fn editing(path: PathBuf, media: MediaInfo, output_dir: Option<PathBuf>) -> Self {
        let mut app = Self::new(Screen::Starting, output_dir);
        app.open_session(path, media);
        app
    }

    fn new(screen: Screen, output_dir: Option<PathBuf>) -> Self {
        Self {
            screen,
            output_dir,
            status: None,
            preview_bounds: (0, 0),
            next_generation: 1,
            previous_dir: None,
            theme: Theme::SYSTEM,
            palette: None,
        }
    }

    /// Uses `theme` for drawing (the runtime picks it at startup).
    pub(crate) const fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub(crate) const fn theme(&self) -> &Theme {
        &self.theme
    }

    pub(crate) const fn palette(&self) -> Option<&Palette> {
        self.palette.as_ref()
    }

    pub(crate) const fn screen(&self) -> &Screen {
        &self.screen
    }

    pub(crate) const fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Whether the runtime should deliver [`Msg::Tick`]s.
    pub(crate) fn is_playing(&self) -> bool {
        matches!(&self.screen, Screen::Editing(session) if session.editor.is_playing())
    }

    /// Handles one message and returns the work to do.
    pub(crate) fn update(&mut self, msg: Msg) -> Vec<Effect> {
        let mut effects = Vec::new();
        match msg {
            Msg::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key, &mut effects),
            Msg::Key(_) => {}
            Msg::Tick(elapsed) => {
                if let Screen::Editing(session) = &mut self.screen {
                    session.tick(elapsed);
                }
            }
            Msg::PreviewBounds(bounds) => self.preview_bounds = bounds,
            Msg::FrameDecoded(frame) => {
                if let Screen::Editing(session) = &mut self.screen {
                    session.on_frame(frame, &mut self.status);
                }
            }
            Msg::StreamFrame(frame) => {
                if let Screen::Editing(session) = &mut self.screen {
                    session.on_stream_frame(frame, &mut self.status);
                }
            }
            Msg::PreviewFailed(error) => {
                self.status = Some(Status::error(format!("Preview failed: {error}")));
            }
            Msg::Opened { path, result } => self.on_opened(path, result),
            Msg::DirectoryListed { dir, result } => self.on_directory_listed(dir, result),
            Msg::Export(event) => {
                if let Screen::Editing(session) = &mut self.screen {
                    session.on_export_event(event, &mut self.status);
                }
            }
        }
        if let Screen::Editing(session) = &mut self.screen {
            session.sync_preview_source(self.preview_bounds, &mut effects);
        }
        effects
    }

    fn on_key(&mut self, key: KeyEvent, effects: &mut Vec<Effect>) {
        if self.palette.is_some() {
            self.on_palette_key(key, effects);
            return;
        }
        if keys::is_ctrl_p(key) {
            self.open_palette();
            return;
        }
        match &mut self.screen {
            Screen::Starting => {}
            Screen::Browser(_) => self.on_browser_key(key, effects),
            Screen::Editing(session) => {
                if let Some(leave) = session.on_key(key, &mut self.status, effects) {
                    self.leave_session(leave, effects);
                }
            }
        }
    }

    /// Opens the palette for the current screen (not over a dialog).
    fn open_palette(&mut self) {
        let commands = match &self.screen {
            Screen::Browser(_) => HOME_COMMANDS,
            Screen::Editing(session) if matches!(session.overlay, Overlay::None) => EDITOR_COMMANDS,
            _ => return,
        };
        self.palette = Some(Palette::new(commands));
    }

    fn on_palette_key(&mut self, key: KeyEvent, effects: &mut Vec<Effect>) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        match keys::palette_key(key) {
            Some(PaletteKey::Previous) => palette.select_previous(),
            Some(PaletteKey::Next) => palette.select_next(),
            Some(PaletteKey::Backspace) => palette.backspace(),
            Some(PaletteKey::Type(character)) => palette.type_char(character),
            Some(PaletteKey::Close) => self.palette = None,
            Some(PaletteKey::Run) => {
                let command = palette.selected_command();
                self.palette = None;
                if let Some(command) = command {
                    self.run_palette_action(command.action, effects);
                }
            }
            None => {}
        }
    }

    fn run_palette_action(&mut self, action: PaletteAction, effects: &mut Vec<Effect>) {
        match action {
            PaletteAction::SwitchTheme => {
                let (theme, name) = if self.theme == Theme::DARK {
                    (Theme::SYSTEM, "system")
                } else {
                    (Theme::DARK, "dark")
                };
                self.theme = theme;
                self.status = Some(Status::info(format!("Theme: {name}")));
            }
            PaletteAction::UpFolder => self.go_up_a_folder(effects),
            PaletteAction::Quit => effects.push(Effect::Quit),
            PaletteAction::ToggleExportMode => {
                if let Screen::Editing(session) = &mut self.screen {
                    session.toggle_export_mode(&mut self.status);
                }
            }
            PaletteAction::Edit(edit) => {
                if let Screen::Editing(session) = &mut self.screen
                    && let Some(leave) = session.run_action(edit, &mut self.status)
                {
                    self.leave_session(leave, effects);
                }
            }
        }
    }

    fn on_browser_key(&mut self, key: KeyEvent, effects: &mut Vec<Effect>) {
        let Screen::Browser(browser) = &mut self.screen else {
            return;
        };
        let Some(action) = keys::home_action(key) else {
            return;
        };
        self.status = None;
        match action {
            HomeAction::Previous => browser.select_previous(),
            HomeAction::Next => browser.select_next(),
            HomeAction::Type(character) => browser.type_char(character),
            HomeAction::Quit => effects.push(Effect::Quit),
            HomeAction::ClearOrQuit => {
                if !browser.clear_query() {
                    effects.push(Effect::Quit);
                }
            }
            HomeAction::Backspace => {
                if !browser.backspace() {
                    self.go_up_a_folder(effects);
                }
            }
            HomeAction::EnterFolder => {
                if let Some(entry) = browser.selected_entry()
                    && entry.kind != EntryKind::Video
                {
                    let target = entry.path.clone();
                    self.list_folder(target, effects);
                }
            }
            HomeAction::Open => self.open_from_home(effects),
        }
    }

    /// Enter on the home screen: a typed path wins, then the selected match.
    fn open_from_home(&mut self, effects: &mut Vec<Effect>) {
        let Screen::Browser(browser) = &self.screen else {
            return;
        };
        if let Some(path) = browser.typed_path() {
            self.status = Some(Status::info(format!("Opening {}…", path.display())));
            effects.push(Effect::Open(path));
            return;
        }
        let Some(entry) = browser.selected_entry() else {
            return;
        };
        match entry.kind {
            EntryKind::Video => {
                self.status = Some(Status::info(format!("Opening {}…", entry.name)));
                effects.push(Effect::Open(entry.path.clone()));
            }
            EntryKind::Parent | EntryKind::Directory => {
                let target = entry.path.clone();
                self.list_folder(target, effects);
            }
        }
    }

    fn go_up_a_folder(&mut self, effects: &mut Vec<Effect>) {
        let Screen::Browser(browser) = &self.screen else {
            return;
        };
        if let Some(parent) = browser.dir.parent() {
            let parent = parent.to_path_buf();
            self.list_folder(parent, effects);
        }
    }

    /// Lists `target`, remembering the current folder so going up reselects it.
    fn list_folder(&mut self, target: PathBuf, effects: &mut Vec<Effect>) {
        if let Screen::Browser(browser) = &self.screen
            && browser.dir.starts_with(&target)
        {
            self.previous_dir = Some(browser.dir.clone());
        }
        effects.push(Effect::ListDirectory(target));
    }

    fn leave_session(&mut self, leave: Leave, effects: &mut Vec<Effect>) {
        match leave {
            Leave::Quit => effects.push(Effect::Quit),
            Leave::Browse(dir) => effects.push(Effect::ListDirectory(dir)),
        }
    }

    fn on_opened(&mut self, path: PathBuf, result: Result<MediaInfo, String>) {
        match result {
            Ok(media) => {
                self.status = None;
                self.open_session(path, media);
            }
            Err(error) => self.status = Some(Status::error(error)),
        }
    }

    fn open_session(&mut self, path: PathBuf, media: MediaInfo) {
        let output_dir = self
            .output_dir
            .clone()
            .unwrap_or_else(|| crate::export::default_output_dir(&path));
        let generation = self.next_generation;
        self.next_generation += 1;
        self.screen = Screen::Editing(Box::new(Session::new(path, media, output_dir, generation)));
    }

    fn on_directory_listed(&mut self, dir: PathBuf, result: Result<Vec<Entry>, String>) {
        let entries = match result {
            Ok(entries) => entries,
            Err(error) => {
                self.status = Some(Status::error(format!(
                    "Cannot read {}: {error}",
                    dir.display()
                )));
                if !matches!(self.screen, Screen::Starting) {
                    return;
                }
                Vec::new()
            }
        };
        let mut browser = Browser::new(dir, entries);
        if let Some(previous) = self.previous_dir.take()
            && let Some(index) = browser
                .entries
                .iter()
                .position(|entry| entry.path == previous)
        {
            browser.selected = index;
        }
        // "Opening <folder>…" has done its job once the folder is shown.
        if self.status.as_ref().is_some_and(|status| !status.is_error) {
            self.status = None;
        }
        self.screen = Screen::Browser(browser);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{FrameRate, VideoStream};
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    fn media() -> MediaInfo {
        MediaInfo {
            duration: Timestamp::from_secs(60),
            video: VideoStream {
                width: 1920,
                height: 1080,
                frame_rate: FrameRate::new(25, 1).unwrap(),
            },
            has_audio: true,
        }
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Effect> {
        app.update(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from("videos").join(name),
            kind,
        }
    }

    fn session(app: &App) -> &Session {
        match app.screen() {
            Screen::Editing(session) => session,
            other => panic!("expected the editor, got {other:?}"),
        }
    }

    #[test]
    fn browsing_starts_by_listing_the_directory() {
        let (_, effects) = App::browsing(PathBuf::from("videos"), None);
        assert_eq!(effects, [Effect::ListDirectory(PathBuf::from("videos"))]);
    }

    #[test]
    fn opening_a_video_from_the_browser_probes_it() {
        let (mut app, _) = App::browsing(PathBuf::from("videos"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos"),
            result: Ok(vec![entry("a.mp4", EntryKind::Video)]),
        });
        let effects = press(&mut app, KeyCode::Enter);
        assert_eq!(
            effects,
            [Effect::Open(PathBuf::from("videos").join("a.mp4"))]
        );
        assert_eq!(app.status().unwrap().text, "Opening a.mp4…");
    }

    #[test]
    fn failed_open_keeps_the_browser_and_shows_the_error() {
        let (mut app, _) = App::browsing(PathBuf::from("videos"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos"),
            result: Ok(vec![]),
        });
        app.update(Msg::Opened {
            path: PathBuf::from("videos/a.mp4"),
            result: Err("cannot open a.mp4: no video stream found".to_owned()),
        });
        assert!(matches!(app.screen(), Screen::Browser(_)));
        assert!(app.status().unwrap().is_error);
    }

    #[test]
    fn going_up_reselects_the_folder_we_came_from() {
        let (mut app, _) = App::browsing(PathBuf::from("videos").join("trip"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos").join("trip"),
            result: Ok(vec![entry("..", EntryKind::Parent)]),
        });
        let effects = press(&mut app, KeyCode::Backspace);
        assert_eq!(effects, [Effect::ListDirectory(PathBuf::from("videos"))]);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos"),
            result: Ok(vec![
                entry("..", EntryKind::Parent),
                entry("other", EntryKind::Directory),
                entry("trip", EntryKind::Directory),
            ]),
        });
        let Screen::Browser(browser) = app.screen() else {
            panic!("expected the browser");
        };
        assert_eq!(browser.selected_entry().unwrap().name, "trip");
    }

    #[test]
    fn unreadable_start_directory_still_shows_a_browser() {
        let (mut app, _) = App::browsing(PathBuf::from("locked"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("locked"),
            result: Err("access denied".to_owned()),
        });
        assert!(matches!(app.screen(), Screen::Browser(_)));
        assert_eq!(
            app.status().unwrap().text,
            "Cannot read locked: access denied"
        );
    }

    #[test]
    fn successful_open_starts_editing_with_the_default_output_dir() {
        let (mut app, _) = App::browsing(PathBuf::from("videos"), None);
        app.update(Msg::Opened {
            path: PathBuf::from("videos").join("a.mp4"),
            result: Ok(media()),
        });
        assert_eq!(session(&app).output_dir, PathBuf::from("videos"));
    }

    #[test]
    fn output_dir_option_overrides_the_default() {
        let app = App::editing(
            PathBuf::from("a.mp4"),
            media(),
            Some(PathBuf::from("clips")),
        );
        assert_eq!(session(&app).output_dir, PathBuf::from("clips"));
    }

    #[test]
    fn first_frame_is_requested_once_the_preview_size_is_known() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        assert!(
            press(&mut app, KeyCode::Right).is_empty(),
            "no size known yet"
        );
        let effects = app.update(Msg::PreviewBounds((640, 640)));
        assert_eq!(
            effects,
            [Effect::RequestFrame(FrameRequest {
                generation: 1,
                path: PathBuf::from("a.mp4"),
                at: Timestamp::from_secs(1),
                size: (640, 360),
            })]
        );
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        let mut release = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        app.update(Msg::Key(release));
        assert_eq!(session(&app).editor.playhead(), Timestamp::ZERO);
    }

    #[test]
    fn ticks_advance_playback() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        press(&mut app, KeyCode::Char(' '));
        assert!(app.is_playing());
        app.update(Msg::Tick(Duration::from_millis(500)));
        assert_eq!(session(&app).editor.playhead(), Timestamp::from_millis(500));
    }

    #[test]
    fn quitting_with_no_clips_needs_no_confirmation() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        assert_eq!(press(&mut app, KeyCode::Char('q')), [Effect::Quit]);
    }

    #[test]
    fn browse_from_the_editor_lists_the_video_directory() {
        let mut app = App::editing(PathBuf::from("videos").join("a.mp4"), media(), None);
        assert_eq!(
            press(&mut app, KeyCode::Char('b')),
            [Effect::ListDirectory(PathBuf::from("videos"))]
        );
    }

    fn home(entries: Vec<Entry>) -> App {
        let (mut app, _) = App::browsing(PathBuf::from("videos"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos"),
            result: Ok(entries),
        });
        app
    }

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            press(app, KeyCode::Char(character));
        }
    }

    #[test]
    fn typing_on_home_then_enter_opens_the_best_match() {
        let mut app = home(vec![
            entry("..", EntryKind::Parent),
            entry("about bob.mkv", EntryKind::Video),
            entry("bodycam.mp4", EntryKind::Video),
        ]);
        type_text(&mut app, "bod");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            [Effect::Open(PathBuf::from("videos").join("bodycam.mp4"))]
        );
    }

    #[test]
    fn a_typed_path_is_opened_directly() {
        let mut app = home(vec![entry("trips", EntryKind::Directory)]);
        type_text(&mut app, "trips/day1.mp4");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            [Effect::Open(PathBuf::from("videos").join("trips/day1.mp4"))]
        );
    }

    #[test]
    fn escape_clears_the_search_before_quitting() {
        let mut app = home(vec![entry("a.mp4", EntryKind::Video)]);
        type_text(&mut app, "zz");
        assert!(press(&mut app, KeyCode::Esc).is_empty(), "first Esc clears");
        assert_eq!(press(&mut app, KeyCode::Esc), [Effect::Quit]);
    }

    #[test]
    fn letters_are_search_text_not_shortcuts() {
        let mut app = home(vec![entry("quarry.mp4", EntryKind::Video)]);
        assert!(
            press(&mut app, KeyCode::Char('q')).is_empty(),
            "q does not quit"
        );
        let Screen::Browser(browser) = app.screen() else {
            panic!("expected the home screen");
        };
        assert_eq!(browser.query, "q");
    }

    #[test]
    fn tab_goes_into_the_selected_folder_but_not_into_videos() {
        let mut app = home(vec![
            entry("trips", EntryKind::Directory),
            entry("a.mp4", EntryKind::Video),
        ]);
        assert_eq!(
            press(&mut app, KeyCode::Tab),
            [Effect::ListDirectory(PathBuf::from("videos").join("trips"))]
        );
        press(&mut app, KeyCode::Down);
        assert!(press(&mut app, KeyCode::Tab).is_empty());
    }

    #[test]
    fn showing_a_folder_clears_the_opening_message() {
        let mut app = home(vec![entry("trips", EntryKind::Directory)]);
        type_text(&mut app, "trips/");
        press(&mut app, KeyCode::Enter);
        assert!(app.status().is_some());
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos").join("trips"),
            result: Ok(vec![]),
        });
        assert_eq!(app.status(), None);
    }

    fn ctrl_p(app: &mut App) -> Vec<Effect> {
        app.update(Msg::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        )))
    }

    fn run_command(app: &mut App, search: &str) -> Vec<Effect> {
        ctrl_p(app);
        type_text(app, search);
        press(app, KeyCode::Enter)
    }

    #[test]
    fn ctrl_p_opens_and_closes_the_palette_on_both_screens() {
        let mut app = home(vec![]);
        ctrl_p(&mut app);
        assert!(app.palette().is_some());
        ctrl_p(&mut app);
        assert!(app.palette().is_none());

        let mut editor = App::editing(PathBuf::from("a.mp4"), media(), None);
        ctrl_p(&mut editor);
        assert!(editor.palette().is_some());
        press(&mut editor, KeyCode::Esc);
        assert!(editor.palette().is_none());
    }

    #[test]
    fn palette_does_not_open_over_a_dialog() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        press(&mut app, KeyCode::Char('?'));
        ctrl_p(&mut app);
        assert!(app.palette().is_none());
    }

    #[test]
    fn palette_keys_do_not_reach_the_editor() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        ctrl_p(&mut app);
        assert!(
            press(&mut app, KeyCode::Char('q')).is_empty(),
            "q is search text"
        );
        assert_eq!(app.palette().unwrap().query, "q");
    }

    #[test]
    fn running_a_palette_command_does_what_its_key_does() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        press(&mut app, KeyCode::Right);
        run_command(&mut app, "set in");
        assert!(app.palette().is_none(), "running closes the palette");
        assert_eq!(
            session(&app).editor.pending_in(),
            Some(Timestamp::from_secs(1))
        );
        assert_eq!(run_command(&mut app, "quit"), [Effect::Quit]);
    }

    #[test]
    fn palette_switches_the_theme_and_says_so() {
        let mut app = home(vec![]).with_theme(Theme::DARK);
        run_command(&mut app, "theme");
        assert_eq!(*app.theme(), Theme::SYSTEM);
        assert_eq!(app.status().unwrap().text, "Theme: system");
        run_command(&mut app, "theme");
        assert_eq!(*app.theme(), Theme::DARK);
    }

    #[test]
    fn palette_switches_the_export_mode() {
        let mut app = App::editing(PathBuf::from("a.mp4"), media(), None);
        run_command(&mut app, "export mode");
        assert_eq!(session(&app).export_mode, ExportMode::Precise);
        assert_eq!(app.status().unwrap().text, "Export mode: Precise");
    }

    #[test]
    fn palette_goes_up_a_folder_from_home() {
        let (mut app, _) = App::browsing(PathBuf::from("videos").join("trip"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos").join("trip"),
            result: Ok(vec![]),
        });
        assert_eq!(
            run_command(&mut app, "up"),
            [Effect::ListDirectory(PathBuf::from("videos"))]
        );
    }
}
