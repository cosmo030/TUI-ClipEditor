//! Drawing the app state with ratatui. Reads state only; never changes it.
//!
//! Every colour comes from the [`Theme`] (see ADR 0003); no widget here
//! names a colour directly.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Screen, Status};
use super::browser::{Browser, EntryKind};
use super::keys::{BROWSER_HINTS, EDITOR_HELP, EDITOR_HINTS};
use super::session::{ExportRun, Leave, Overlay, Session, count_clips};
use super::terminal_image::{PreviewImage, TerminalImage};
use super::theme::Theme;
use super::timeline::Timeline;
use crate::clip::Clip;

/// Width of the clip list, including its border.
const CLIP_LIST_WIDTH: u16 = 44;

/// What the runtime has for the preview box.
#[derive(Default)]
pub(super) struct PreviewDisplay<'a> {
    /// The encoded frame, if one is ready.
    pub(super) image: Option<&'a TerminalImage>,
    /// The image mode, for the title (for example `blocks` or `sixel`).
    pub(super) label: &'a str,
    /// Where playback time goes, while playing.
    pub(super) stats: Option<PlaybackStats>,
    /// Where the last frame left an image uncovered ([`Drawn::image`]).
    pub(super) previous_image: Option<Rect>,
}

/// Playback measurements shown in the preview title.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct PlaybackStats {
    /// New frames written to the terminal per second.
    pub(super) shown_fps: usize,
    /// Frames arriving from the decoder per second.
    pub(super) decoded_fps: usize,
    /// Average time to encode a frame for the terminal.
    pub(super) encode_ms: Option<f64>,
    /// Average time to write a frame to the terminal (includes waiting for
    /// the terminal to take earlier output).
    pub(super) draw_ms: Option<f64>,
    /// Average image data per frame, where known.
    pub(super) frame_kb: Option<f64>,
}

impl PlaybackStats {
    /// `24 fps · decode 30/s · encode 16 ms · draw 22 ms · 180 KB`.
    fn summary(&self) -> String {
        let mut parts = vec![
            format!("{} fps", self.shown_fps),
            format!("decode {}/s", self.decoded_fps),
        ];
        if let Some(ms) = self.encode_ms {
            parts.push(format!("encode {ms:.0} ms"));
        }
        if let Some(ms) = self.draw_ms {
            parts.push(format!("draw {ms:.0} ms"));
        }
        if let Some(kb) = self.frame_kb {
            parts.push(format!("{kb:.0} KB"));
        }
        parts.join(" · ")
    }
}

/// Where the preview ended up on screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Drawn {
    /// The area available for the preview image, so frames can be sized to it.
    pub(super) preview_area: Option<Rect>,
    /// Where an image was drawn, if nothing (such as a dialog) covers it.
    pub(super) image: Option<Rect>,
}

/// Draws the whole screen.
pub(super) fn draw(
    frame: &mut Frame<'_>,
    app: &App,
    preview: &PreviewDisplay<'_>,
    theme: &Theme,
) -> Drawn {
    let area = frame.area();
    frame.render_widget(Block::new().style(theme.base()), area);
    match app.screen() {
        Screen::Starting => {
            frame.render_widget(Paragraph::new("Loading…").style(theme.muted()), area);
            Drawn::default()
        }
        Screen::Browser(browser) => {
            draw_browser(frame, area, browser, app.status(), theme);
            Drawn::default()
        }
        Screen::Editing(session) => draw_editor(frame, area, session, app.status(), preview, theme),
    }
}

/// A panel: a bordered block on the panel background.
fn panel<'a>(theme: &Theme, title: impl Into<Line<'a>>) -> Block<'a> {
    Block::bordered()
        .border_style(theme.border())
        .style(Style::new().bg(theme.background_panel).fg(theme.text))
        .title(title)
}

/// A dialog: like a panel, on the raised element background.
fn dialog<'a>(theme: &Theme, title: impl Into<Line<'a>>) -> Block<'a> {
    Block::bordered()
        .border_style(Style::new().fg(theme.border_active))
        .style(Style::new().bg(theme.background_element).fg(theme.text))
        .title(title)
}

fn draw_browser(
    frame: &mut Frame<'_>,
    area: Rect,
    browser: &Browser,
    status: Option<&Status>,
    theme: &Theme,
) {
    let [list_area, status_area, hints_area] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let block = panel(theme, format!(" Open a video · {} ", browser.dir.display()));
    if browser.entries.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "No folders or video files here.",
            theme.muted(),
        ))
        .block(block);
        frame.render_widget(empty, list_area);
    } else {
        let items = browser.entries.iter().map(|entry| match entry.kind {
            EntryKind::Parent => ListItem::new(Span::styled(".. (up a folder)", theme.muted())),
            EntryKind::Directory => ListItem::new(Span::styled(
                format!("▸ {}/", entry.name),
                Style::new().fg(theme.primary),
            )),
            EntryKind::Video => ListItem::new(format!("  {}", entry.name)),
        });
        let list = List::new(items)
            .block(block)
            .highlight_style(theme.selected());
        let mut state = ListState::default().with_selected(Some(browser.selected));
        frame.render_stateful_widget(list, list_area, &mut state);
    }
    frame.render_widget(status_line(None, status, theme), status_area);
    frame.render_widget(
        Paragraph::new(Span::styled(BROWSER_HINTS, theme.muted())),
        hints_area,
    );
}

fn draw_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &Session,
    status: Option<&Status>,
    preview: &PreviewDisplay<'_>,
    theme: &Theme,
) -> Drawn {
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

    frame.render_widget(title_line(session, theme), title_area);
    let (image_area, image) = draw_preview(frame, preview_area, session, preview, theme);
    draw_clip_list(frame, clips_area, session, theme);
    draw_timeline(frame, timeline_area, session, theme);
    let playback = if session.editor.is_playing() {
        "▶ Playing"
    } else {
        "⏸ Paused"
    };
    frame.render_widget(status_line(Some(playback), status, theme), status_area);
    frame.render_widget(
        Paragraph::new(Span::styled(EDITOR_HINTS, theme.muted())),
        hints_area,
    );

    draw_overlay(frame, area, session, theme);
    Drawn {
        preview_area: Some(image_area),
        // A dialog drawn over the image leaves text in its cells, so the
        // next image must erase first.
        image: image.filter(|_| matches!(session.overlay, Overlay::None)),
    }
}

fn title_line(session: &Session, theme: &Theme) -> Line<'static> {
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
        Span::styled(" clipedit ", theme.chip()),
        Span::raw(format!(
            " {name} · {}×{} · {:.2} fps · {}{audio}",
            video.width,
            video.height,
            video.frame_rate.frames_per_second(),
            session.media.duration
        )),
    ])
}

/// Draws the preview box with the image centred in it. The title names the
/// image mode (so a mode the terminal cannot show is easy to spot and
/// override with `--preview`) and, while playing, the frames shown per second.
///
/// Returns the preview area and, if an image was drawn, its rectangle.
fn draw_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    session: &Session,
    preview: &PreviewDisplay<'_>,
    theme: &Theme,
) -> (Rect, Option<Rect>) {
    let rate = preview
        .stats
        .map(|stats| format!(" · {}", stats.summary()))
        .unwrap_or_default();
    let block = panel(theme, format!(" Preview · {}{rate} ", preview.label));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match (preview.image, &session.frame) {
        (Some(image), Some(_)) => {
            let size = image.size();
            let rect = centered(inner, size.width, size.height);
            // Erasing first is only needed when this exact spot did not
            // already show an image last frame (see `terminal_image`).
            let erase = preview.previous_image != Some(rect);
            frame.render_widget(PreviewImage { image, erase }, rect);
            (inner, Some(rect))
        }
        _ => {
            frame.render_widget(
                Paragraph::new(Span::styled("Loading preview…", theme.muted())),
                inner,
            );
            (inner, None)
        }
    }
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

fn draw_clip_list(frame: &mut Frame<'_>, area: Rect, session: &Session, theme: &Theme) {
    let clips = session.editor.clips();
    let block = panel(theme, format!(" Clips ({}) ", clips.len()));
    if clips.is_empty() {
        let help = Paragraph::new(vec![
            Line::styled("No clips yet.", theme.muted()),
            Line::from(""),
            Line::styled("Press i at the start of a", theme.muted()),
            Line::styled("part and o at its end.", theme.muted()),
        ])
        .block(block);
        frame.render_widget(help, area);
        return;
    }
    let items = clips
        .iter()
        .enumerate()
        .map(|(index, clip)| ListItem::new(clip_row(index, *clip)));
    let list = List::new(items)
        .block(block)
        .highlight_style(theme.selected());
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

fn draw_timeline(frame: &mut Frame<'_>, area: Rect, session: &Session, theme: &Theme) {
    let editor = &session.editor;
    let mut block = panel(
        theme,
        format!(" {} / {} ", editor.playhead(), editor.duration()),
    );
    if let Some(in_point) = editor.pending_in() {
        block = block.title_bottom(
            Line::styled(
                format!(" in point {in_point} "),
                Style::new().fg(theme.accent),
            )
            .right_aligned(),
        );
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
            theme,
        },
        inner,
    );
}

fn status_line<'a>(
    prefix: Option<&'a str>,
    status: Option<&'a Status>,
    theme: &Theme,
) -> Paragraph<'a> {
    let mut spans = Vec::new();
    if let Some(prefix) = prefix {
        spans.push(Span::styled(format!(" {prefix}  "), bold(theme)));
    }
    if let Some(status) = status {
        let color = if status.is_error {
            theme.error
        } else {
            theme.info
        };
        spans.push(Span::styled(status.text.as_str(), Style::new().fg(color)));
    }
    Paragraph::new(Line::from(spans))
}

fn draw_overlay(frame: &mut Frame<'_>, area: Rect, session: &Session, theme: &Theme) {
    match &session.overlay {
        Overlay::None => {}
        Overlay::Help => draw_help(frame, area, theme),
        Overlay::ExportDialog => draw_export_dialog(frame, area, session, theme),
        Overlay::Exporting(run) => draw_export_progress(frame, area, run, theme),
        Overlay::ConfirmLeave(leave) => draw_confirm_leave(frame, area, leave, theme),
    }
}

/// Emphasised text, such as key names.
fn bold(theme: &Theme) -> Style {
    Style::new().fg(theme.text).add_modifier(Modifier::BOLD)
}

fn draw_help(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let lines: Vec<Line<'_>> = EDITOR_HELP
        .iter()
        .map(|(keys, action)| {
            Line::from(vec![
                Span::styled(format!(" {keys:<12}"), bold(theme)),
                Span::raw(*action),
            ])
        })
        .collect();
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let popup = popup_area(area, 52, height);
    frame.render_widget(Clear, popup);
    let block = dialog(theme, " Keys ")
        .title_bottom(Line::styled(" any key closes ", theme.muted()).right_aligned());
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_export_dialog(frame: &mut Frame<'_>, area: Rect, session: &Session, theme: &Theme) {
    let mode = session.export_mode;
    let lines = vec![
        Line::from(vec![
            Span::raw(" Mode:  "),
            Span::styled(
                format!("‹ {mode} ›"),
                Style::new().fg(theme.primary).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  (Tab switches)", theme.muted()),
        ]),
        Line::styled(format!("        {}", mode.description()), theme.muted()),
        Line::from(""),
        Line::from(format!(" To:    {}", session.output_dir.display())),
        Line::from(""),
        Line::styled(" Enter export · Esc back", theme.muted()),
    ];
    let popup = popup_area(area, 78, 8);
    frame.render_widget(Clear, popup);
    let title = format!(" Export {} ", count_clips(session.editor.clips().len()));
    let content = Paragraph::new(lines)
        .block(dialog(theme, title))
        .wrap(Wrap { trim: false });
    frame.render_widget(content, popup);
}

fn draw_export_progress(frame: &mut Frame<'_>, area: Rect, run: &ExportRun, theme: &Theme) {
    let popup = popup_area(area, 70, 7);
    frame.render_widget(Clear, popup);
    let block = dialog(theme, " Exporting ");
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
            Paragraph::new(Span::styled(
                format!(" {}", name.unwrap_or_default()),
                theme.muted(),
            )),
            file,
        );
    }
    let ratio = run.ratio();
    let gauge = Gauge::default()
        .gauge_style(Style::new().fg(theme.success).bg(theme.background_panel))
        .ratio(ratio)
        .label(format!("{:.0}%", ratio * 100.0));
    frame.render_widget(gauge, gauge_area);
    let hint_text = if run.cancelling {
        " Cancelling…"
    } else {
        " Esc cancel"
    };
    frame.render_widget(Paragraph::new(Span::styled(hint_text, theme.muted())), hint);
}

fn draw_confirm_leave(frame: &mut Frame<'_>, area: Rect, leave: &Leave, theme: &Theme) {
    let action = match leave {
        Leave::Quit => "Quit",
        Leave::Browse(_) => "Open another video",
    };
    let lines = vec![
        Line::from(" Some clips have not been exported and will be lost."),
        Line::from(""),
        Line::from(vec![
            Span::raw(format!(" {action} anyway? ")),
            Span::styled("y", bold(theme)),
            Span::raw(" / "),
            Span::styled("n", bold(theme)),
        ]),
    ];
    let popup = popup_area(area, 58, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(dialog(theme, " Unsaved clips ")),
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
    use crate::tui::app::{DecodedFrame, Msg};
    use crate::tui::browser::Entry;
    use crate::tui::terminal_image::{Quality, encode};
    use image::RgbImage;
    use ratatui::layout::Size;
    use ratatui_image::picker::{Picker, ProtocolType};

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
        render_sized(app, display, 100)
    }

    fn render_sized(app: &App, display: &PreviewDisplay<'_>, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, app, display, &Theme::DARK);
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
    fn preview_title_shows_where_playback_time_goes() {
        let app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        let screen = render_sized(
            &app,
            &PreviewDisplay {
                label: "sixel",
                stats: Some(PlaybackStats {
                    shown_fps: 24,
                    decoded_fps: 30,
                    encode_ms: Some(15.6),
                    draw_ms: Some(22.2),
                    frame_kb: Some(180.4),
                }),
                ..PreviewDisplay::default()
            },
            160,
        );
        assert!(screen.contains(
            "Preview · sixel · 24 fps · decode 30/s · encode 16 ms · draw 22 ms · 180 KB"
        ));
    }

    #[test]
    fn stats_leave_out_measurements_not_taken_yet() {
        let stats = PlaybackStats {
            shown_fps: 0,
            decoded_fps: 12,
            ..PlaybackStats::default()
        };
        assert_eq!(stats.summary(), "0 fps · decode 12/s");
    }

    /// An editor with a decoded frame and that frame encoded as sixel.
    fn app_with_sixel_frame() -> (App, TerminalImage) {
        let mut app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        app.update(Msg::FrameDecoded(DecodedFrame {
            generation: 1,
            at: Timestamp::ZERO,
            size: (40, 40),
            image: Ok(RgbImage::new(40, 40)),
        }));
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Sixel);
        let image = encode(
            &picker,
            RgbImage::new(40, 40),
            Size::new(20, 10),
            Quality::Full,
        )
        .unwrap();
        (app, image)
    }

    /// Draws once; returns where things went and the image cell's symbol
    /// (empty when no uncovered image was drawn).
    fn draw_image(app: &App, image: &TerminalImage, previous: Option<Rect>) -> (Drawn, String) {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        let display = PreviewDisplay {
            image: Some(image),
            label: "sixel",
            previous_image: previous,
            ..PreviewDisplay::default()
        };
        let mut drawn = Drawn::default();
        terminal
            .draw(|frame| drawn = draw(frame, app, &display, &Theme::DARK))
            .unwrap();
        let symbol = drawn
            .image
            .map(|rect| {
                terminal.backend().buffer()[rect.as_position()]
                    .symbol()
                    .to_owned()
            })
            .unwrap_or_default();
        (drawn, symbol)
    }

    #[test]
    fn first_sixel_frame_erases_and_later_frames_do_not() {
        let (app, image) = app_with_sixel_frame();
        let (first, symbol) = draw_image(&app, &image, None);
        assert!(
            symbol.starts_with("\x1b[4X"),
            "erases before the first image"
        );
        let (second, symbol) = draw_image(&app, &image, first.image);
        assert!(
            symbol.starts_with("\x1bP"),
            "draws straight over the last image"
        );
        assert_eq!(second.image, first.image);
    }

    #[test]
    fn a_dialog_over_the_image_forces_the_next_erase() {
        let (mut app, image) = app_with_sixel_frame();
        app.update(Msg::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::NONE,
        )));
        let (drawn, _) = draw_image(&app, &image, None);
        assert_eq!(drawn.image, None, "help covers the image");
        assert!(drawn.preview_area.is_some());
    }

    #[test]
    fn centered_places_the_image_in_the_middle() {
        let area = Rect::new(2, 1, 100, 40);
        assert_eq!(centered(area, 60, 20), Rect::new(22, 11, 60, 20));
        assert_eq!(centered(area, 200, 50), area, "never larger than the area");
    }

    /// Draws the editor with `theme` and returns the buffer.
    fn render_themed(theme: &Theme) -> ratatui::buffer::Buffer {
        let app = App::editing(PathBuf::from("trip.mp4"), media(), None);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, &app, &PreviewDisplay::default(), theme);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn find(buffer: &ratatui::buffer::Buffer, text: &str) -> (u16, u16) {
        let width = buffer.area.width;
        for y in 0..buffer.area.height {
            let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
            if let Some(x) = row.find(text) {
                let column = row[..x].chars().count();
                return (u16::try_from(column).unwrap(), y);
            }
        }
        panic!("{text:?} is not on screen");
    }

    #[test]
    fn dark_theme_paints_backgrounds_and_mutes_hints() {
        let buffer = render_themed(&Theme::DARK);
        let (x, y) = find(&buffer, "No clips yet.");
        assert_eq!(buffer[(x, y)].fg, Theme::DARK.text_muted);
        assert_eq!(buffer[(x, y)].bg, Theme::DARK.background_panel);
        let (x, y) = find(&buffer, "Space play");
        assert_eq!(buffer[(x, y)].fg, Theme::DARK.text_muted);
        assert_eq!(buffer[(x, y)].bg, Theme::DARK.background);
        let (x, y) = find(&buffer, "clipedit");
        assert_eq!(buffer[(x, y)].bg, Theme::DARK.primary, "title chip");
    }

    #[test]
    fn system_theme_keeps_the_terminal_background() {
        let buffer = render_themed(&Theme::SYSTEM);
        let (x, y) = find(&buffer, "Space play");
        assert_eq!(buffer[(x, y)].bg, ratatui::style::Color::Reset);
        assert_eq!(buffer[(x, y)].fg, Theme::SYSTEM.text_muted);
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
