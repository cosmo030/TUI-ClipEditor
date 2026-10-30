//! The event loop: owns the terminal, runs background work, and feeds
//! results back into [`App::update`].

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::layout::Rect;
use ratatui_image::picker::Picker;

use super::Settings;
use super::app::{
    App, DecodedFrame, Effect, ExportEvent, ExportRequest, FrameRequest, Msg, Screen,
};
use super::browser::read_entries;
use super::encoder::{
    EncodeRequest, EncodeTarget, Encoded, FrameRateMeter, RollingAverage, spawn_encoder,
};
use super::playback::{PlaybackCommand, spawn_playback_worker};
use super::preview::{create_picker, protocol_label};
use super::terminal_image::{Quality, TerminalImage};
use super::theme::{TerminalInfo, Theme};
use super::view::{PlaybackStats, PreviewDisplay};
use super::{Start, TuiError, view};
use crate::export::{ExportJob, plan_exports};
use crate::ffmpeg::Ffmpeg;

/// How often the playhead advances while playing.
const PLAYBACK_TICK: Duration = Duration::from_millis(33);
/// Largest preview frame decoded, in pixels. Bigger frames cost more to
/// decode, encode, and draw than they gain in a terminal.
const MAX_PREVIEW_PIXELS: (u32, u32) = (1920, 1080);

/// Everything the event loop waits for.
enum LoopEvent {
    App(Msg),
    /// A preview frame was encoded for the terminal.
    Encoded(Encoded),
    /// The terminal was resized; only a redraw is needed.
    Resized,
    /// Reading terminal input failed; the UI cannot continue.
    InputFailed(io::Error),
}

/// The encoded preview currently on screen.
struct ShownPreview {
    target: EncodeTarget,
    image: TerminalImage,
}

/// Where playback time goes, for the preview title: frames reaching the
/// screen and arriving from the decoder per second, encode and draw time,
/// and how much image data each frame sends.
#[derive(Default)]
struct PlaybackMeter {
    shown: FrameRateMeter,
    decoded: FrameRateMeter,
    encode_ms: RollingAverage,
    draw_ms: RollingAverage,
    frame_kb: RollingAverage,
    /// The last image written to the terminal, to count each one once.
    last_drawn: Option<EncodeTarget>,
}

impl PlaybackMeter {
    fn stats(&mut self, now: Instant) -> PlaybackStats {
        PlaybackStats {
            shown_fps: self.shown.per_second(now),
            decoded_fps: self.decoded.per_second(now),
            encode_ms: self.encode_ms.value(),
            draw_ms: self.draw_ms.value(),
            frame_kb: self.frame_kb.value(),
        }
    }
}

/// Runs the UI until the user quits.
///
/// # Errors
///
/// Returns [`TuiError::Terminal`] if the terminal cannot be set up, drawn
/// to, read from, or restored.
pub fn run(start: Start, settings: Settings, ffmpeg: Ffmpeg) -> Result<(), TuiError> {
    let theme = Theme::resolve(settings.theme, &TerminalInfo::from_env());
    let mut terminal = ratatui::try_init()?;
    // Queries the terminal, so it must run before the input reader starts.
    let picker = create_picker(settings.preview);
    let result = Runtime::new(start, settings.output_dir, ffmpeg, picker, theme).run(&mut terminal);
    let restored = ratatui::try_restore();
    result?;
    restored.map_err(TuiError::from)
}

struct Runtime {
    app: App,
    pending: VecDeque<Effect>,
    ffmpeg: Ffmpeg,
    picker: Picker,
    theme: Theme,
    events: Sender<LoopEvent>,
    inbox: Receiver<LoopEvent>,
    frames: Sender<FrameRequest>,
    playback: Sender<PlaybackCommand>,
    encoder: Sender<EncodeRequest>,
    export_cancel: Option<Arc<AtomicBool>>,
    preview: Option<ShownPreview>,
    /// Where the last frame left an image uncovered, so the next image there
    /// can be drawn without erasing first.
    last_image: Option<Rect>,
    /// The newest frame sent to the encoder, so it is not sent twice.
    encoding: Option<EncodeTarget>,
    preview_area: Option<Rect>,
    meter: PlaybackMeter,
    last_tick: Instant,
}

impl Runtime {
    fn new(
        start: Start,
        output_dir: Option<PathBuf>,
        ffmpeg: Ffmpeg,
        picker: Picker,
        theme: Theme,
    ) -> Self {
        let (events, inbox) = mpsc::channel();
        let frames = spawn_frame_worker(ffmpeg.clone(), events.clone());
        let playback = {
            let events = events.clone();
            spawn_playback_worker(ffmpeg.clone(), move |frame| {
                events.send(LoopEvent::App(Msg::StreamFrame(frame))).is_ok()
            })
        };
        let encoder = {
            let events = events.clone();
            spawn_encoder(picker.clone(), move |encoded| {
                events.send(LoopEvent::Encoded(encoded)).is_ok()
            })
        };
        let (app, effects) = match start {
            Start::Browse(dir) => App::browsing(dir, output_dir),
            Start::Edit { path, media } => (App::editing(path, media, output_dir), Vec::new()),
        };
        Self {
            app,
            pending: effects.into(),
            ffmpeg,
            picker,
            theme,
            events,
            inbox,
            frames,
            playback,
            encoder,
            export_cancel: None,
            preview: None,
            last_image: None,
            encoding: None,
            preview_area: None,
            meter: PlaybackMeter::default(),
            last_tick: Instant::now(),
        }
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<(), TuiError> {
        spawn_input_reader(self.events.clone());
        loop {
            if self.perform_pending() {
                return Ok(());
            }
            self.draw(terminal)?;
            // Drawing can queue work (a new preview size needs a new frame);
            // do it before blocking on the next event.
            if !self.pending.is_empty() {
                continue;
            }
            for event in self.next_events()? {
                match event {
                    LoopEvent::App(msg) => {
                        if matches!(msg, Msg::StreamFrame(_)) {
                            self.meter.decoded.record(Instant::now());
                        }
                        self.dispatch(msg);
                    }
                    LoopEvent::Encoded(encoded) => self.on_encoded(encoded),
                    LoopEvent::Resized => {}
                    LoopEvent::InputFailed(error) => return Err(TuiError::Terminal(error)),
                }
            }
        }
    }

    fn dispatch(&mut self, msg: Msg) {
        let effects = self.app.update(msg);
        self.pending.extend(effects);
    }

    /// Performs queued effects. Returns `true` when the app asked to quit.
    fn perform_pending(&mut self) -> bool {
        while let Some(effect) = self.pending.pop_front() {
            match effect {
                Effect::Quit => return true,
                Effect::ListDirectory(dir) => {
                    let result = read_entries(&dir).map_err(|error| error.to_string());
                    self.dispatch(Msg::DirectoryListed { dir, result });
                }
                Effect::Open(path) => self.open_in_background(path),
                // The workers only stop when these senders are dropped, so
                // sending cannot fail while the runtime exists.
                Effect::RequestFrame(request) => drop(self.frames.send(request)),
                Effect::StartPlayback(request) => {
                    drop(self.playback.send(PlaybackCommand::Start(request)));
                }
                Effect::StopPlayback => drop(self.playback.send(PlaybackCommand::Stop)),
                Effect::StartExport(request) => self.start_export(request),
                Effect::CancelExport => {
                    if let Some(cancel) = &self.export_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
        false
    }

    fn draw(&mut self, terminal: &mut DefaultTerminal) -> Result<(), TuiError> {
        self.sync_preview();
        let now = Instant::now();
        let stats = self.app.is_playing().then(|| self.meter.stats(now));
        let display = PreviewDisplay {
            image: self.preview.as_ref().map(|preview| &preview.image),
            label: protocol_label(self.picker.protocol_type()),
            stats,
            previous_image: self.last_image,
        };
        let new_image = self
            .preview
            .as_ref()
            .filter(|preview| Some(preview.target) != self.meter.last_drawn)
            .map(|preview| (preview.target, preview.image.byte_len()));
        let mut drawn = view::Drawn::default();
        // Synchronized output (DEC mode 2026) asks the terminal to show the
        // frame only once it is complete, so a half-written image is never
        // painted. Terminals without support ignore both sequences.
        execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        let result = terminal
            .draw(|frame| drawn = view::draw(frame, &self.app, &display, &self.theme))
            .map(|_| ());
        execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
        result?;
        // Writing blocks while the terminal is still busy with earlier
        // output, so the draw time shows how fast the terminal takes images.
        if let (Some((target, bytes)), Some(_)) = (new_image, drawn.image) {
            self.meter.last_drawn = Some(target);
            self.meter.shown.record(Instant::now());
            self.meter
                .draw_ms
                .record(now.elapsed().as_secs_f64() * 1_000.0);
            if let Some(bytes) = bytes {
                self.meter.frame_kb.record(bytes as f64 / 1_024.0);
            }
        }
        self.last_image = drawn.image;
        let image_area = drawn.preview_area;
        if image_area != self.preview_area {
            self.preview_area = image_area;
            if let Some(area) = image_area {
                let bounds = preview_bounds(area, &self.picker);
                self.dispatch(Msg::PreviewBounds(bounds));
            }
        }
        Ok(())
    }

    /// Sends the newest decoded frame to the encoder when the shown preview
    /// is out of date (new frame, or a resized preview area).
    fn sync_preview(&mut self) {
        let Screen::Editing(session) = self.app.screen() else {
            self.preview = None;
            self.encoding = None;
            return;
        };
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.target.key.0 != session.generation())
        {
            self.preview = None;
        }
        let (Some(frame), Some(area)) = (&session.frame, self.preview_area) else {
            return;
        };
        // Stream frames are drawn lighter while playing; once paused, the
        // same frame is encoded again at full quality.
        let quality = if frame.from_stream && session.editor.is_playing() {
            Quality::Playback
        } else {
            Quality::Full
        };
        let wanted = EncodeTarget {
            key: (session.generation(), frame.serial),
            cells: area.as_size(),
            quality,
        };
        let is_shown = self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.target == wanted);
        if is_shown || self.encoding == Some(wanted) {
            return;
        }
        self.encoding = Some(wanted);
        drop(self.encoder.send(EncodeRequest {
            target: wanted,
            image: frame.image.clone(),
        }));
    }

    fn on_encoded(&mut self, encoded: Encoded) {
        let generation = match self.app.screen() {
            Screen::Editing(session) => session.generation(),
            _ => return,
        };
        if encoded.target.key.0 != generation {
            return;
        }
        match encoded.image {
            Ok(image) => {
                self.meter
                    .encode_ms
                    .record(encoded.took.as_secs_f64() * 1_000.0);
                self.preview = Some(ShownPreview {
                    target: encoded.target,
                    image,
                });
            }
            Err(error) => self.dispatch(Msg::PreviewFailed(error)),
        }
    }

    /// Waits for the next event; while playing, also wakes up to advance time.
    fn next_events(&mut self) -> Result<Vec<LoopEvent>, TuiError> {
        let is_playing = self.app.is_playing();
        let received = if is_playing {
            match self.inbox.recv_timeout(PLAYBACK_TICK) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return Err(channel_closed()),
            }
        } else {
            Some(self.inbox.recv().map_err(|_| channel_closed())?)
        };
        let now = Instant::now();
        let elapsed = now - self.last_tick;
        self.last_tick = now;

        let mut events = Vec::new();
        if is_playing {
            events.push(LoopEvent::App(Msg::Tick(elapsed)));
        }
        events.extend(received);
        // Handle everything already queued before redrawing, so a burst of
        // export progress or held-down keys costs one redraw, not many.
        events.extend(self.inbox.try_iter());
        Ok(events)
    }

    fn open_in_background(&self, path: PathBuf) {
        let ffmpeg = self.ffmpeg.clone();
        let events = self.events.clone();
        thread::spawn(move || {
            let result = ffmpeg.probe(&path).map_err(|error| error.to_string());
            drop(events.send(LoopEvent::App(Msg::Opened { path, result })));
        });
    }

    fn start_export(&mut self, request: ExportRequest) {
        let jobs = plan_exports(
            &request.input,
            &request.output_dir,
            &request.clips,
            request.mode,
            Path::exists,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        self.export_cancel = Some(Arc::clone(&cancel));
        let ffmpeg = self.ffmpeg.clone();
        let events = self.events.clone();
        thread::spawn(move || {
            run_export_jobs(&ffmpeg, jobs, &cancel, |event| {
                // A closed channel means the UI is gone; nothing is left to tell.
                drop(events.send(LoopEvent::App(Msg::Export(event))));
            });
        });
    }
}

/// Runs `jobs` one after another, reporting each step through `send`.
/// After a cancel, the remaining jobs are skipped.
fn run_export_jobs(
    ffmpeg: &Ffmpeg,
    jobs: Vec<ExportJob>,
    cancel: &AtomicBool,
    send: impl Fn(ExportEvent),
) {
    send(ExportEvent::Started(jobs.clone()));
    for (index, job) in jobs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let result = ffmpeg
            .export(
                job,
                |written| send(ExportEvent::Progress { index, written }),
                cancel,
            )
            .map_err(|error| error.to_string());
        send(ExportEvent::JobDone { index, result });
    }
    send(ExportEvent::Finished);
}

/// The pixel size that fills `area` for the detected font size, capped.
fn preview_bounds(area: Rect, picker: &Picker) -> (u32, u32) {
    let font = picker.font_size();
    let width = u32::from(area.width) * u32::from(font.width);
    let height = u32::from(area.height) * u32::from(font.height);
    (
        width.min(MAX_PREVIEW_PIXELS.0),
        height.min(MAX_PREVIEW_PIXELS.1),
    )
}

fn channel_closed() -> TuiError {
    TuiError::Terminal(io::Error::other("internal event channel closed"))
}

/// Decodes preview frames one request at a time.
fn spawn_frame_worker(ffmpeg: Ffmpeg, events: Sender<LoopEvent>) -> Sender<FrameRequest> {
    let (requests, inbox) = mpsc::channel::<FrameRequest>();
    thread::spawn(move || {
        for request in inbox {
            let image = ffmpeg
                .extract_frame(&request.path, request.at, request.size)
                .map_err(|error| error.to_string());
            let decoded = DecodedFrame {
                generation: request.generation,
                at: request.at,
                size: request.size,
                image,
            };
            if events
                .send(LoopEvent::App(Msg::FrameDecoded(decoded)))
                .is_err()
            {
                return;
            }
        }
    });
    requests
}

/// Forwards terminal input to the event loop.
fn spawn_input_reader(events: Sender<LoopEvent>) {
    thread::spawn(move || {
        loop {
            let forwarded = match event::read() {
                Ok(Event::Key(key)) => events.send(LoopEvent::App(Msg::Key(key))),
                Ok(Event::Resize(..)) => events.send(LoopEvent::Resized),
                Ok(_) => Ok(()),
                Err(error) => {
                    drop(events.send(LoopEvent::InputFailed(error)));
                    return;
                }
            };
            if forwarded.is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Clip;
    use crate::export::ExportMode;
    use crate::ffmpeg::ExportOutcome;
    use crate::timecode::Timestamp;

    #[test]
    fn preview_bounds_scale_cells_by_font_size_and_cap() {
        let picker = Picker::halfblocks();
        let font = picker.font_size();
        let small = preview_bounds(Rect::new(0, 0, 10, 5), &picker);
        assert_eq!(
            small,
            (10 * u32::from(font.width), 5 * u32::from(font.height))
        );
        let huge = preview_bounds(Rect::new(0, 0, 1_000, 1_000), &picker);
        assert_eq!(huge, MAX_PREVIEW_PIXELS);
    }

    /// Writes a 2 s synthetic video with ffmpeg (required for the test suite).
    fn make_video(dir: &Path) -> PathBuf {
        let path = dir.join("in.mkv");
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-nostdin", "-v", "error", "-f", "lavfi"])
            .args(["-i", "testsrc=duration=2:size=160x120:rate=25"])
            .args(["-c:v", "mpeg4", "-g", "25"])
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    fn collect_events(jobs: Vec<ExportJob>, cancel: &AtomicBool) -> Vec<ExportEvent> {
        let events = std::sync::Mutex::new(Vec::new());
        run_export_jobs(&Ffmpeg::from_path(), jobs, cancel, |event| {
            events.lock().unwrap().push(event);
        });
        events.into_inner().unwrap()
    }

    #[test]
    fn export_worker_runs_every_job_and_reports_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let input = make_video(dir.path());
        let clips = [
            Clip::new(Timestamp::ZERO, Timestamp::from_secs(1)).unwrap(),
            Clip::new(Timestamp::from_secs(1), Timestamp::from_secs(2)).unwrap(),
        ];
        let jobs = plan_exports(
            &input,
            dir.path(),
            &clips,
            ExportMode::FastCopy,
            Path::exists,
        );

        let events = collect_events(jobs.clone(), &AtomicBool::new(false));

        assert!(matches!(events.first(), Some(ExportEvent::Started(started)) if *started == jobs));
        assert!(matches!(events.last(), Some(ExportEvent::Finished)));
        let done: Vec<usize> = events
            .iter()
            .filter_map(|event| match event {
                ExportEvent::JobDone {
                    index,
                    result: Ok(ExportOutcome::Completed),
                } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(done, [0, 1]);
        assert!(jobs.iter().all(|job| job.output.exists()));
    }

    #[test]
    fn export_worker_skips_all_jobs_once_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let input = make_video(dir.path());
        let clips = [Clip::new(Timestamp::ZERO, Timestamp::from_secs(1)).unwrap()];
        let jobs = plan_exports(
            &input,
            dir.path(),
            &clips,
            ExportMode::Precise,
            Path::exists,
        );

        let events = collect_events(jobs.clone(), &AtomicBool::new(true));

        assert_eq!(events.len(), 2, "only Started and Finished: {events:?}");
        assert!(!jobs[0].output.exists());
    }
}
