//! Drawing the app state with ratatui. Reads state only; never changes it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::Image;
use ratatui_image::protocol::Protocol;

use super::app::{App, Screen, Status};
use super::browser::{Browser, EntryKind};
use super::keys::{BROWSER_HINTS, EDITOR_HELP, EDITOR_HINTS};
use super::session::{ExportRun, Leave, Overlay, Session, count_clips};
use super::timeline::Timeline;
use crate::clip::Clip;

/// Width of the clip list, including its border.
const CLIP_LIST_WIDTH: u16 = 44;
const HIGHLIGHT: Style = Style::new().add_modifier(Modifier::REVERSED);

/// What the runtime has for the preview box.
#[derive(Default)]
pub(super) struct PreviewDisplay<'a> {
    /// The encoded frame, if one is ready.
    pub(super) image: Option<&'a Protocol>,
    /// The image mode, for the title (for example `blocks` or `sixel`).
    pub(super) label: &'a str,
    /// Preview frames shown per second, while playing.
    pub(super) fps: Option<usize>,
}

/// Draws the whole screen. Returns the area available for the preview
/// image, so the runtime can size frames to it.
pub(super) fn draw(frame: &mut Frame<'_>, app: &App, preview: &PreviewDisplay<'_>) -> Option<Rect> {
    let area = frame.area();
    match app.screen() {
        Screen::Starting => {
            frame.render_widget(Paragraph::new("Loading…"), area);
            None
        }
        Screen::Browser(browser) => {
            draw_browser(frame, area, browser, app.status());
            None
        }
        Screen::Editing(session) => Some(draw_editor(frame, area, session, app.status(), preview)),
    }
}

fn draw_browser(frame: &mut Frame<'_>, area: Rect, browser: &Browser, status: Option<&Status>) {
    let [list_area, status_area, hints_area] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let block = Block::bordered().title(format!(" Open a video · {} ", browser.dir.display()));
    if browser.entries.is_empty() {
        let empty = Paragraph::new("No folders or video files here.".dim()).block(block);
        frame.render_widget(empty, list_area);
    } else {
        let items = browser.entries.iter().map(|entry| match entry.kind {
            EntryKind::Parent => ListItem::new(".. (up a folder)".dim()),
            EntryKind::Directory => ListItem::new(format!("▸ {}/", entry.name).blue()),
            EntryKind::Video => ListItem::new(format!("  {}", entry.name)),
        });
        let list = List::new(items).block(block).highlight_style(HIGHLIGHT);
        let mut state = ListState::default().with_selected(Some(browser.selected));
        frame.render_stateful_widget(list, list_area, &mut state);
    }
    frame.render_widget(status_line(None, status), status_area);
    frame.render_widget(Paragraph::new(BROWSER_HINTS.dim()), hints_area);
}

fn draw_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &Session,
    status: Option<&Status>,
    preview: &PreviewDisplay<'_>,
) -> Rect {
    let [
        title_area,
        main_area,
        timeline_area,
        status_area,
        hints_area,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let [preview_area, clips_area] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(CLIP_LIST_WIDTH)])
            .areas(main_area);

    frame.render_widget(title_line(session), title_area);
    let image_area = draw_preview(frame, preview_area, session, preview);
    draw_clip_list(frame, clips_area, session);
    draw_timeline(frame, timeline_area, session);
    let playback = if session.editor.is_playing() {
        "▶ Playing"
    } else {
        "⏸ Paused"
    };
    frame.render_widget(status_line(Some(playback), status), status_area);
    frame.render_widget(Paragraph::new(EDITOR_HINTS.dim()), hints_area);

    draw_overlay(frame, area, session);
    image_area
}

fn title_line(session: &Session) -> Line<'static> {
    let name = session.path.file_name().map_or_else(
        || session.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let video = session.media.video;
    let audio = if session.media.has_audio {
        ""
    } else {
        " · no audio"
    };
    Line::from(vec![
        " clipedit ".bold().reversed(),
        format!(
            " {name} · {}×{} · {:.2} fps · {}{audio}",
            video.width,
            video.height,
            video.frame_rate.frames_per_second(),
            session.media.duration
        )
        .into(),
    ])
}

/// Draws the preview box with the image centred in it. The title names the
/// image mode (so a mode the terminal cannot show is easy to spot and
/// override with `--preview`) and, while playing, the frames shown per second.
fn draw_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &Session,
    preview: &PreviewDisplay<'_>,
) -> Rect {
    let rate = preview
        .fps
        .map(|fps| format!(" · {fps} fps"))
        .unwrap_or_default();
    let block = Block::bordered().title(format!(" Preview · {}{rate} ", preview.label));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match (preview.image, &session.frame) {
        (Some(protocol), Some(_)) => {
            let size = protocol.size();
            frame.render_widget(
                Image::new(protocol),
                centered(inner, size.width, size.height),
            );
        }
        _ => frame.render_widget(Paragraph::new("Loading preview…".dim()), inner),
    }
    inner
}

/// A `width` × `height` rectangle centred in `area`, shrunk to fit.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn draw_clip_list(frame: &mut Frame<'_>, area: Rect, session: &Session) {
    let clips = session.editor.clips();
    let block = Block::bordered().title(format!(" Clips ({}) ", clips.len()));
    if clips.is_empty() {
        let help = Paragraph::new(vec![
            Line::from("No clips yet.".dim()),
            Line::from(""),
            Line::from("Press i at the start of a".dim()),
            Line::from("part and o at its end.".dim()),
        ])
        .block(block);
        frame.render_widget(help, area);
        return;
    }
    let items = clips
        .iter()
        .enumerate()
        .map(|(index, clip)| ListItem::new(clip_row(index, *clip)));
    let list = List::new(items).block(block).highlight_style(HIGHLIGHT);
    let mut state = ListState::default().with_selected(session.editor.selected_index());
    frame.render_stateful_widget(list, area, &mut state);
}

fn clip_row(index: usize, clip: Clip) -> String {
    let seconds = clip.length().as_micros() as f64 / 1_000_000.0;
    format!(
        "{:>2} {} → {} {seconds:>6.1}s",
        index + 1,
        clip.start(),
        clip.end()
    )
}

fn draw_timeline(frame: &mut Frame<'_>, area: Rect, session: &Session) {
    let editor = &session.editor;
    let mut block =
        Block::bordered().title(format!(" {} / {} ", editor.playhead(), editor.duration()));
    if let Some(in_point) = editor.pending_in() {
        block = block
            .title_bottom(Line::from(format!(" in point {in_point} ").magenta()).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Timeline {
            duration: editor.duration(),
            playhead: editor.playhead(),
            pending_in: editor.pending_in(),
            clips: editor.clips(),
            selected: editor.selected_index(),
        },
        inner,
    );
}

fn status_line<'a>(prefix: Option<&'a str>, status: Option<&'a Status>) -> Paragraph<'a> {
    let mut spans = Vec::new();
    if let Some(prefix) = prefix {
        spans.push(Span::from(format!(" {prefix}  ")).bold());
    }
    if let Some(status) = status {
        let color = if status.is_error {
            Color::Red
        } else {
            Color::Cyan
        };
        spans.push(Span::styled(status.text.as_str(), Style::new().fg(color)));
    }
    Paragraph::new(Line::from(spans))
}

fn draw_overlay(frame: &mut Frame<'_>, area: Rect, session: &Session) {
    match &session.overlay {
        Overlay::None => {}
        Overlay::Help => draw_help(frame, area),
        Overlay::ExportDialog => draw_export_dialog(frame, area, session),
        Overlay::Exporting(run) => draw_export_progress(frame, area, run),
        Overlay::ConfirmLeave(leave) => draw_confirm_leave(frame, area, leave),
    }
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let lines: Vec<Line<'_>> = EDITOR_HELP
        .iter()
        .map(|(keys, action)| Line::from(vec![format!(" {keys:<12}").bold(), Span::from(*action)]))
        .collect();
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let popup = popup_area(area, 52, height);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Keys ")
        .title_bottom(Line::from(" any key closes ").right_aligned());
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_export_dialog(frame: &mut Frame<'_>, area: Rect, session: &Session) {
    let mode = session.export_mode;
    let lines = vec![
        Line::from(vec![
            " Mode:  ".into(),
            format!("‹ {mode} ›").bold().yellow(),
            "  (Tab switches)".dim(),
        ]),
        Line::from(format!("        {}", mode.description()).dim()),
        Line::from(""),
        Line::from(format!(" To:    {}", session.output_dir.display())),
        Line::from(""),
        Line::from(" Enter export · Esc back".dim()),
    ];
    let popup = popup_area(area, 78, 8);
    frame.render_widget(Clear, popup);
    let title = format!(" Export {} ", count_clips(session.editor.clips().len()));
    let dialog = Paragraph::new(lines)
        .block(Block::bordered().title(title))
        .wrap(Wrap { trim: false });
    frame.render_widget(dialog, popup);
}

fn draw_export_progress(frame: &mut Frame<'_>, area: Rect, run: &ExportRun) {
    let popup = popup_area(area, 70, 7);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(" Exporting ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [heading, file, _, gauge_area, hint] =
        Layout::vertical([Constraint::Length(1); 5]).areas(inner);

    let total = run.jobs.len();
    let heading_text = if total == 0 {
        " Preparing…".to_owned()
    } else {
        format!(" Clip {} of {total}", (run.current + 1).min(total))
    };
    frame.render_widget(Paragraph::new(heading_text), heading);
    if let Some(job) = run.jobs.get(run.current) {
        let name = job
            .output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        frame.render_widget(
            Paragraph::new(format!(" {}", name.unwrap_or_default()).dim()),
            file,
        );
    }
    let ratio = run.ratio();
    let gauge = Gauge::default()
        .gauge_style(Style::new().fg(Color::Green))
        .ratio(ratio)
        .label(format!("{:.0}%", ratio * 100.0));
    frame.render_widget(gauge, gauge_area);
    let hint_text = if run.cancelling {
        " Cancelling…"
    } else {
        " Esc cancel"
    };
    frame.render_widget(Paragraph::new(hint_text.dim()), hint);
}

fn draw_confirm_leave(frame: &mut Frame<'_>, area: Rect, leave: &Leave) {
    let action = match leave {
        Leave::Quit => "Quit",
        Leave::Browse(_) => "Open another video",
    };
    let lines = vec![
        Line::from(" Some clips have not been exported and will be lost."),
        Line::from(""),
        Line::from(vec![
            format!(" {action} anyway? ").into(),
            "y".bold(),
            " / ".into(),
            "n".bold(),
        ]),
    ];
    let popup = popup_area(area, 58, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Unsaved clips ")),
        popup,
    );
}

/// A `width` × `height` rectangle centred in `area`, shrunk to fit.
fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    popup
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::media::{FrameRate, MediaInfo, VideoStream};
    use crate::timecode::Timestamp;
    use crate::tui::app::Msg;
    use crate::tui::browser::Entry;

    fn media() -> MediaInfo {
        MediaInfo {
            duration: Timestamp::from_secs(60),
            video: VideoStream {
                width: 1920,
                height: 1080,
                frame_rate: FrameRate::new(30_000, 1_001).unwrap(),
            },
            has_audio: false,
        }
    }

    fn render(app: &App) -> String {
        render_with(
            app,
            &PreviewDisplay {
                label: "blocks",
                ..PreviewDisplay::default()
            },
        )
    }

    fn render_with(app: &App, display: &PreviewDisplay<'_>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, app, display);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let width = buffer.area.width as usize;
        buffer
            .content
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(app: &mut App, code: KeyCode) {
        app.update(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    #[test]
    fn editor_shows_title_preview_placeholder_and_hints() {
        let app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        let screen = render(&app);
        assert!(screen.contains("trip.mp4 · 1920×1080 · 29.97 fps · 00:01:00.000 · no audio"));
        assert!(screen.contains("Loading preview…"));
        assert!(screen.contains("Preview · blocks"), "names the image mode");
        assert!(screen.contains("No clips yet."));
        assert!(screen.contains("00:00:00.000 / 00:01:00.000"));
        assert!(screen.contains("⏸ Paused"));
        assert!(screen.contains("e export"));
    }

    #[test]
    fn marked_clip_appears_in_the_list() {
        let mut app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Char('o'));
        let screen = render(&app);
        assert!(screen.contains("Clips (1)"));
        assert!(screen.contains(" 1 00:00:01.000 → 00:00:03.000    2.0s"));
    }

    #[test]
    fn export_dialog_shows_mode_and_destination() {
        let mut app = App::editing(PathBuf::from("v").join("trip.mp4"), media(), None);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('e'));
        let screen = render(&app);
        assert!(screen.contains("Export 1 clip"));
        assert!(screen.contains("‹ Fast copy ›"));
        assert!(screen.contains(&format!("To:    {}", PathBuf::from("v").display())));
    }

    #[test]
    fn help_overlay_lists_the_keys() {
        let mut app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        press(&mut app, KeyCode::Char('?'));
        let screen = render(&app);
        assert!(screen.contains("set out point (adds a clip)"));
    }

    #[test]
    fn browser_lists_entries_with_folders_marked() {
        let (mut app, _) = App::browsing(PathBuf::from("videos"), None);
        app.update(Msg::DirectoryListed {
            dir: PathBuf::from("videos"),
            result: Ok(vec![
                Entry {
                    name: "trips".to_owned(),
                    path: PathBuf::from("videos/trips"),
                    kind: EntryKind::Directory,
                },
                Entry {
                    name: "a.mp4".to_owned(),
                    path: PathBuf::from("videos/a.mp4"),
                    kind: EntryKind::Video,
                },
            ]),
        });
        let screen = render(&app);
        assert!(screen.contains("Open a video · videos"));
        assert!(screen.contains("▸ trips/"));
        assert!(screen.contains("  a.mp4"));
    }

    #[test]
    fn preview_title_shows_the_frame_rate_while_playing() {
        let app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        let screen = render_with(
            &app,
            &PreviewDisplay {
                label: "sixel",
                fps: Some(24),
                ..PreviewDisplay::default()
            },
        );
        assert!(screen.contains("Preview · sixel · 24 fps"));
    }

    #[test]
    fn centered_places_the_image_in_the_middle() {
        let area = Rect::new(2, 1, 100, 40);
        assert_eq!(centered(area, 60, 20), Rect::new(22, 11, 60, 20));
        assert_eq!(centered(area, 200, 50), area, "never larger than the area");
    }

    #[test]
    fn popup_is_centred_and_clamped() {
        let area = Rect::new(0, 0, 100, 20);
        assert_eq!(popup_area(area, 40, 10), Rect::new(30, 5, 40, 10));
        assert_eq!(
            popup_area(Rect::new(0, 0, 30, 4), 40, 10),
            Rect::new(0, 0, 30, 4)
        );
    }
}
