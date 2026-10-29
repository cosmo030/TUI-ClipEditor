//! The editing screen: one open video, its clips, and the dialogs on top.

use std::path::PathBuf;
use std::time::Duration;

use image::RgbImage;
use ratatui::crossterm::event::KeyEvent;

use super::app::{DecodedFrame, Effect, ExportEvent, ExportRequest, FrameRequest, Status};
use super::keys::{self, DialogAction, EditAction};
use crate::editor::Editor;
use crate::export::{ExportJob, ExportMode, default_output_dir};
use crate::ffmpeg::{ExportOutcome, fit_within};
use crate::media::MediaInfo;
use crate::timecode::Timestamp;

/// Where the user wants to go when leaving the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Leave {
    Quit,
    Browse(PathBuf),
}

/// What is drawn over the editor.
#[derive(Debug)]
pub(crate) enum Overlay {
    None,
    Help,
    ExportDialog,
    Exporting(ExportRun),
    /// Leaving would lose clips that were not exported.
    ConfirmLeave(Leave),
}

/// The most recent decoded preview frame.
#[derive(Debug)]
pub(crate) struct PreviewFrame {
    /// Increases with every new frame, so the view knows when to re-encode.
    pub(crate) serial: u64,
    pub(crate) at: Timestamp,
    pub(crate) size: (u32, u32),
    pub(crate) image: RgbImage,
}

/// Progress of a running export.
#[derive(Debug, Default)]
pub(crate) struct ExportRun {
    /// Empty until the runtime has planned the jobs.
    pub(crate) jobs: Vec<ExportJob>,
    pub(crate) current: usize,
    pub(crate) written: Timestamp,
    pub(crate) completed: usize,
    pub(crate) failures: Vec<String>,
    pub(crate) cancelling: bool,
}

impl ExportRun {
    /// Fraction of the total clip length written so far, from 0 to 1.
    pub(crate) fn ratio(&self) -> f64 {
        let total: u64 = self
            .jobs
            .iter()
            .map(|job| job.clip.length().as_micros())
            .sum();
        if total == 0 {
            return 0.0;
        }
        let done: u64 = self
            .jobs
            .iter()
            .take(self.current)
            .map(|job| job.clip.length().as_micros())
            .sum::<u64>()
            + self.written.as_micros();
        (done as f64 / total as f64).clamp(0.0, 1.0)
    }
}

/// One open video being edited.
#[derive(Debug)]
pub(crate) struct Session {
    pub(crate) path: PathBuf,
    pub(crate) media: MediaInfo,
    pub(crate) editor: Editor,
    pub(crate) output_dir: PathBuf,
    pub(crate) export_mode: ExportMode,
    pub(crate) overlay: Overlay,
    pub(crate) frame: Option<PreviewFrame>,
    generation: u64,
    frame_in_flight: bool,
    /// The last position and size that failed to decode, so playback does
    /// not retry it on every tick.
    failed_frame: Option<(Timestamp, (u32, u32))>,
    has_unexported_clips: bool,
}

impl Session {
    pub(crate) fn new(
        path: PathBuf,
        media: MediaInfo,
        output_dir: PathBuf,
        generation: u64,
    ) -> Self {
        Self {
            editor: Editor::new(&media),
            path,
            media,
            output_dir,
            export_mode: ExportMode::default(),
            overlay: Overlay::None,
            frame: None,
            generation,
            frame_in_flight: false,
            failed_frame: None,
            has_unexported_clips: false,
        }
    }

    /// Identifies this session's file; see [`FrameRequest::generation`].
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn tick(&mut self, elapsed: Duration) {
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.editor.advance(Timestamp::from_micros(micros));
    }

    /// Handles a key press. Returns where to go if the user leaves the editor.
    pub(crate) fn on_key(
        &mut self,
        key: KeyEvent,
        status: &mut Option<Status>,
        effects: &mut Vec<Effect>,
    ) -> Option<Leave> {
        match &self.overlay {
            Overlay::None => self.on_editor_key(key, status),
            Overlay::Help => {
                // Any key closes the help.
                self.overlay = Overlay::None;
                None
            }
            Overlay::ExportDialog => {
                self.on_dialog_key(key, status, effects);
                None
            }
            Overlay::Exporting(_) => {
                self.on_exporting_key(key, effects);
                None
            }
            Overlay::ConfirmLeave(leave) => {
                let leave = leave.clone();
                match keys::confirm_answer(key) {
                    Some(true) => return Some(leave),
                    Some(false) => self.overlay = Overlay::None,
                    None => {}
                }
                None
            }
        }
    }

    fn on_editor_key(&mut self, key: KeyEvent, status: &mut Option<Status>) -> Option<Leave> {
        let action = keys::edit_action(key)?;
        *status = None;
        match action {
            EditAction::Quit => return self.leave(Leave::Quit),
            EditAction::Browse => return self.leave(Leave::Browse(default_output_dir(&self.path))),
            EditAction::OpenExport => self.open_export_dialog(status),
            EditAction::ToggleHelp => self.overlay = Overlay::Help,
            EditAction::MarkOut => self.mark_out(status),
            EditAction::MarkIn => {
                self.editor.mark_in();
                *status = Some(Status::info(format!(
                    "In point set at {}",
                    self.editor.playhead()
                )));
            }
            EditAction::DeleteSelected => {
                if let Some(clip) = self.editor.delete_selected() {
                    self.has_unexported_clips = !self.editor.clips().is_empty();
                    *status = Some(Status::info(format!(
                        "Deleted clip {} – {}",
                        clip.start(),
                        clip.end()
                    )));
                }
            }
            other => self.apply_navigation(other),
        }
        None
    }

    fn apply_navigation(&mut self, action: EditAction) {
        let editor = &mut self.editor;
        match action {
            EditAction::TogglePlayback => editor.toggle_playback(),
            EditAction::Seek(delta) => editor.seek_by(delta),
            EditAction::StepFrames(frames) => editor.step_frames(frames),
            EditAction::JumpToStart => editor.seek_to(Timestamp::ZERO),
            EditAction::JumpToEnd => editor.seek_to(editor.duration()),
            EditAction::ClearIn => {
                editor.clear_in();
            }
            EditAction::SelectPrevious => editor.select_previous(),
            EditAction::SelectNext => editor.select_next(),
            EditAction::JumpToSelected => editor.jump_to_selected(),
            _ => {}
        }
    }

    fn leave(&mut self, leave: Leave) -> Option<Leave> {
        if self.has_unexported_clips {
            self.editor.pause();
            self.overlay = Overlay::ConfirmLeave(leave);
            None
        } else {
            Some(leave)
        }
    }

    fn mark_out(&mut self, status: &mut Option<Status>) {
        match self.editor.mark_out() {
            Ok(clip) => {
                self.has_unexported_clips = true;
                *status = Some(Status::info(format!(
                    "Added clip {} – {} ({} in total)",
                    clip.start(),
                    clip.end(),
                    count_clips(self.editor.clips().len())
                )));
            }
            Err(error) => *status = Some(Status::error(error.to_string())),
        }
    }

    fn open_export_dialog(&mut self, status: &mut Option<Status>) {
        if self.editor.clips().is_empty() {
            *status = Some(Status::error(
                "No clips to export yet: mark one with i (in) and o (out)",
            ));
        } else {
            self.editor.pause();
            self.overlay = Overlay::ExportDialog;
        }
    }

    fn on_dialog_key(
        &mut self,
        key: KeyEvent,
        status: &mut Option<Status>,
        effects: &mut Vec<Effect>,
    ) {
        match keys::dialog_action(key) {
            Some(DialogAction::ToggleMode) => self.export_mode = self.export_mode.toggled(),
            Some(DialogAction::Cancel) => self.overlay = Overlay::None,
            Some(DialogAction::Confirm) => {
                *status = None;
                effects.push(Effect::StartExport(ExportRequest {
                    input: self.path.clone(),
                    output_dir: self.output_dir.clone(),
                    clips: self.editor.clips().to_vec(),
                    mode: self.export_mode,
                }));
                self.overlay = Overlay::Exporting(ExportRun::default());
            }
            None => {}
        }
    }

    fn on_exporting_key(&mut self, key: KeyEvent, effects: &mut Vec<Effect>) {
        if let Overlay::Exporting(run) = &mut self.overlay
            && keys::cancels_export(key)
            && !run.cancelling
        {
            run.cancelling = true;
            effects.push(Effect::CancelExport);
        }
    }

    pub(crate) fn on_export_event(&mut self, event: ExportEvent, status: &mut Option<Status>) {
        let Overlay::Exporting(run) = &mut self.overlay else {
            return;
        };
        match event {
            ExportEvent::Started(jobs) => run.jobs = jobs,
            ExportEvent::Progress { index, written } => {
                run.current = index;
                run.written = written;
            }
            ExportEvent::JobDone { index, result } => {
                run.current = index + 1;
                run.written = Timestamp::ZERO;
                match result {
                    Ok(ExportOutcome::Completed) => run.completed += 1,
                    Ok(ExportOutcome::Cancelled) => {}
                    Err(error) => run.failures.push(error),
                }
            }
            ExportEvent::Finished => self.finish_export(status),
        }
    }

    fn finish_export(&mut self, status: &mut Option<Status>) {
        let Overlay::Exporting(run) = std::mem::replace(&mut self.overlay, Overlay::None) else {
            return;
        };
        let total = run.jobs.len();
        let folder = self.output_dir.display();
        *status = Some(match (run.failures.first(), run.cancelling) {
            (Some(first), _) => Status::error(format!(
                "Exported {} of {}; {} failed: {first}",
                run.completed,
                count_clips(total),
                run.failures.len()
            )),
            (None, true) => Status::error(format!(
                "Export cancelled after {} of {} (in {folder})",
                run.completed,
                count_clips(total)
            )),
            (None, false) => {
                self.has_unexported_clips = false;
                Status::info(format!("Exported {} to {folder}", count_clips(total)))
            }
        });
    }

    pub(crate) fn on_frame(&mut self, frame: DecodedFrame, status: &mut Option<Status>) {
        if frame.generation != self.generation {
            return;
        }
        self.frame_in_flight = false;
        match frame.image {
            Ok(image) => {
                let serial = self
                    .frame
                    .as_ref()
                    .map_or(1, |previous| previous.serial + 1);
                self.failed_frame = None;
                self.frame = Some(PreviewFrame {
                    serial,
                    at: frame.at,
                    size: frame.size,
                    image,
                });
            }
            Err(error) => {
                self.failed_frame = Some((frame.at, frame.size));
                *status = Some(Status::error(format!("Preview failed: {error}")));
            }
        }
    }

    /// Asks for a new preview frame when the shown one is out of date and no
    /// request is already running (one at a time keeps ffmpeg from piling up).
    pub(crate) fn request_frame_if_needed(
        &mut self,
        bounds: (u32, u32),
        effects: &mut Vec<Effect>,
    ) {
        if self.frame_in_flight || bounds.0 == 0 || bounds.1 == 0 {
            return;
        }
        let size = fit_within((self.media.video.width, self.media.video.height), bounds);
        let at = self.editor.preview_position();
        let is_current = self
            .frame
            .as_ref()
            .is_some_and(|frame| frame.at == at && frame.size == size);
        if is_current || self.failed_frame == Some((at, size)) {
            return;
        }
        self.frame_in_flight = true;
        effects.push(Effect::RequestFrame(FrameRequest {
            generation: self.generation,
            path: self.path.clone(),
            at,
            size,
        }));
    }
}

/// "1 clip", "3 clips".
pub(crate) fn count_clips(count: usize) -> String {
    if count == 1 {
        "1 clip".to_owned()
    } else {
        format!("{count} clips")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Clip;
    use crate::media::{FrameRate, VideoStream};
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    const BOUNDS: (u32, u32) = (320, 320);

    fn session() -> Session {
        let media = MediaInfo {
            duration: Timestamp::from_secs(60),
            video: VideoStream {
                width: 640,
                height: 360,
                frame_rate: FrameRate::new(25, 1).unwrap(),
            },
            has_audio: true,
        };
        Session::new(
            PathBuf::from("v").join("a.mp4"),
            media,
            PathBuf::from("out"),
            7,
        )
    }

    fn press(session: &mut Session, code: KeyCode) -> (Option<Leave>, Vec<Effect>, Option<Status>) {
        let mut status = None;
        let mut effects = Vec::new();
        let leave = session.on_key(
            KeyEvent::new(code, KeyModifiers::NONE),
            &mut status,
            &mut effects,
        );
        (leave, effects, status)
    }

    fn add_clip(session: &mut Session, start_secs: i64, end_secs: i64) {
        session.editor.seek_to(Timestamp::ZERO);
        session.editor.seek_by(start_secs * 1_000_000);
        press(session, KeyCode::Char('i'));
        session.editor.seek_by((end_secs - start_secs) * 1_000_000);
        press(session, KeyCode::Char('o'));
    }

    fn frame(at: Timestamp, generation: u64) -> DecodedFrame {
        DecodedFrame {
            generation,
            at,
            size: (320, 180),
            image: Ok(RgbImage::new(320, 180)),
        }
    }

    fn job(seconds: u64) -> ExportJob {
        ExportJob {
            input: PathBuf::from("a.mp4"),
            output: PathBuf::from("out.mp4"),
            clip: Clip::new(Timestamp::ZERO, Timestamp::from_secs(seconds)).unwrap(),
            mode: ExportMode::FastCopy,
        }
    }

    #[test]
    fn marking_in_and_out_adds_a_clip_and_reports_it() {
        let mut session = session();
        session.editor.seek_by(2_000_000);
        let (_, _, status) = press(&mut session, KeyCode::Char('i'));
        assert_eq!(status.unwrap().text, "In point set at 00:00:02.000");
        session.editor.seek_by(3_000_000);
        let (_, _, status) = press(&mut session, KeyCode::Char('o'));
        assert_eq!(
            status.unwrap().text,
            "Added clip 00:00:02.000 – 00:00:05.000 (1 clip in total)"
        );
        assert_eq!(session.editor.clips().len(), 1);
    }

    #[test]
    fn mark_out_errors_are_shown_as_errors() {
        let mut session = session();
        let (_, _, status) = press(&mut session, KeyCode::Char('o'));
        let status = status.unwrap();
        assert!(status.is_error);
        assert_eq!(status.text, "set an in point first (press i)");
    }

    #[test]
    fn leaving_with_unexported_clips_asks_first() {
        let mut session = session();
        add_clip(&mut session, 1, 2);
        let (leave, _, _) = press(&mut session, KeyCode::Char('q'));
        assert_eq!(leave, None);
        assert!(matches!(
            session.overlay,
            Overlay::ConfirmLeave(Leave::Quit)
        ));

        let (leave, _, _) = press(&mut session, KeyCode::Char('n'));
        assert_eq!(leave, None);
        assert!(matches!(session.overlay, Overlay::None));

        press(&mut session, KeyCode::Char('q'));
        let (leave, _, _) = press(&mut session, KeyCode::Char('y'));
        assert_eq!(leave, Some(Leave::Quit));
    }

    #[test]
    fn deleting_the_last_clip_allows_leaving_without_asking() {
        let mut session = session();
        add_clip(&mut session, 1, 2);
        press(&mut session, KeyCode::Char('d'));
        let (leave, _, _) = press(&mut session, KeyCode::Char('q'));
        assert_eq!(leave, Some(Leave::Quit));
    }

    #[test]
    fn export_without_clips_explains_how_to_add_one() {
        let mut session = session();
        let (_, effects, status) = press(&mut session, KeyCode::Char('e'));
        assert!(effects.is_empty());
        assert!(
            status
                .unwrap()
                .text
                .contains("mark one with i (in) and o (out)")
        );
        assert!(matches!(session.overlay, Overlay::None));
    }

    #[test]
    fn export_dialog_toggles_mode_and_starts_the_export() {
        let mut session = session();
        add_clip(&mut session, 1, 3);
        press(&mut session, KeyCode::Char('e'));
        assert!(matches!(session.overlay, Overlay::ExportDialog));
        press(&mut session, KeyCode::Tab);
        let (_, effects, _) = press(&mut session, KeyCode::Enter);
        assert_eq!(
            effects,
            [Effect::StartExport(ExportRequest {
                input: PathBuf::from("v").join("a.mp4"),
                output_dir: PathBuf::from("out"),
                clips: session.editor.clips().to_vec(),
                mode: ExportMode::Precise,
            })]
        );
        assert!(matches!(session.overlay, Overlay::Exporting(_)));
    }

    #[test]
    fn escape_cancels_a_running_export_once() {
        let mut session = session();
        session.overlay = Overlay::Exporting(ExportRun::default());
        let (_, effects, _) = press(&mut session, KeyCode::Esc);
        assert_eq!(effects, [Effect::CancelExport]);
        let (_, effects, _) = press(&mut session, KeyCode::Esc);
        assert!(effects.is_empty(), "cancel is only sent once");
    }

    #[test]
    fn successful_export_reports_and_clears_the_unexported_flag() {
        let mut session = session();
        add_clip(&mut session, 1, 3);
        session.overlay = Overlay::Exporting(ExportRun::default());
        let mut status = None;
        session.on_export_event(ExportEvent::Started(vec![job(2)]), &mut status);
        session.on_export_event(
            ExportEvent::JobDone {
                index: 0,
                result: Ok(ExportOutcome::Completed),
            },
            &mut status,
        );
        session.on_export_event(ExportEvent::Finished, &mut status);
        assert_eq!(
            status.unwrap().text,
            format!("Exported 1 clip to {}", PathBuf::from("out").display())
        );
        let (leave, _, _) = press(&mut session, KeyCode::Char('q'));
        assert_eq!(leave, Some(Leave::Quit), "nothing left to lose");
    }

    #[test]
    fn failed_export_reports_the_first_error() {
        let mut session = session();
        session.overlay = Overlay::Exporting(ExportRun::default());
        let mut status = None;
        session.on_export_event(ExportEvent::Started(vec![job(1), job(1)]), &mut status);
        session.on_export_event(
            ExportEvent::JobDone {
                index: 0,
                result: Err("ffmpeg failed: boom".to_owned()),
            },
            &mut status,
        );
        session.on_export_event(
            ExportEvent::JobDone {
                index: 1,
                result: Ok(ExportOutcome::Completed),
            },
            &mut status,
        );
        session.on_export_event(ExportEvent::Finished, &mut status);
        let status = status.unwrap();
        assert!(status.is_error);
        assert_eq!(
            status.text,
            "Exported 1 of 2 clips; 1 failed: ffmpeg failed: boom"
        );
    }

    #[test]
    fn clip_counts_are_pluralized() {
        assert_eq!(count_clips(1), "1 clip");
        assert_eq!(count_clips(0), "0 clips");
        assert_eq!(count_clips(3), "3 clips");
    }

    #[test]
    fn export_ratio_counts_finished_jobs_and_current_progress() {
        let run = ExportRun {
            jobs: vec![job(2), job(2)],
            current: 1,
            written: Timestamp::from_secs(1),
            ..ExportRun::default()
        };
        assert!((run.ratio() - 0.75).abs() < 1e-9);
        assert!(ExportRun::default().ratio().abs() < f64::EPSILON);
    }

    #[test]
    fn only_one_frame_request_runs_at_a_time() {
        let mut session = session();
        let mut effects = Vec::new();
        session.request_frame_if_needed(BOUNDS, &mut effects);
        session.editor.seek_by(1_000_000);
        session.request_frame_if_needed(BOUNDS, &mut effects);
        assert_eq!(effects.len(), 1);

        session.on_frame(frame(Timestamp::ZERO, 7), &mut None);
        session.request_frame_if_needed(BOUNDS, &mut effects);
        assert_eq!(effects.len(), 2, "the newer position is requested next");
        let Effect::RequestFrame(request) = &effects[1] else {
            panic!("expected a frame request");
        };
        assert_eq!(request.at, Timestamp::from_secs(1));
        assert_eq!(request.size, (320, 180));
    }

    #[test]
    fn frames_from_a_previous_file_are_ignored() {
        let mut session = session();
        session.on_frame(frame(Timestamp::ZERO, 6), &mut None);
        assert!(session.frame.is_none());
    }

    #[test]
    fn a_failed_frame_is_not_retried_until_something_changes() {
        let mut session = session();
        let mut effects = Vec::new();
        session.request_frame_if_needed(BOUNDS, &mut effects);
        let mut status = None;
        session.on_frame(
            DecodedFrame {
                image: Err("decoder exploded".to_owned()),
                ..frame(Timestamp::ZERO, 7)
            },
            &mut status,
        );
        assert_eq!(status.unwrap().text, "Preview failed: decoder exploded");
        session.request_frame_if_needed(BOUNDS, &mut effects);
        assert_eq!(effects.len(), 1, "same position is not retried");
        session.editor.seek_by(40_000);
        session.request_frame_if_needed(BOUNDS, &mut effects);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut session = session();
        press(&mut session, KeyCode::Char('?'));
        assert!(matches!(session.overlay, Overlay::Help));
        press(&mut session, KeyCode::Char('x'));
        assert!(matches!(session.overlay, Overlay::None));
    }
}
