//! The playback worker: streams frames from one ffmpeg process and releases
//! each one when the wall clock reaches it.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use image::RgbImage;

use crate::ffmpeg::{Ffmpeg, FrameStream};
use crate::media::FrameRate;
use crate::timecode::Timestamp;

/// Start streaming `path` from `start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackRequest {
    /// Identifies this run, so frames from an earlier run are dropped.
    pub(crate) stream_id: u64,
    /// Identifies the opened file (see `FrameRequest::generation`).
    pub(crate) generation: u64,
    pub(crate) path: PathBuf,
    pub(crate) start: Timestamp,
    pub(crate) size: (u32, u32),
    pub(crate) rate: FrameRate,
}

/// One frame from a playback run, or the error that ended it.
#[derive(Debug)]
pub(crate) struct StreamedFrame {
    pub(crate) stream_id: u64,
    pub(crate) generation: u64,
    pub(crate) at: Timestamp,
    pub(crate) size: (u32, u32),
    pub(crate) image: Result<RgbImage, String>,
}

#[derive(Debug)]
pub(super) enum PlaybackCommand {
    Start(PlaybackRequest),
    Stop,
}

/// When to release a decoded frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pace {
    /// The frame is early: wait this long, then show it.
    WaitThenShow(Duration),
    /// The frame is due or late: show it now.
    Show,
}

/// Decides when a frame `frame_offset` into the run is shown, `elapsed`
/// after the run started. Late frames are shown at once; the UI only draws
/// the newest frame it has, so a backlog never builds up on screen.
pub(super) fn pace(frame_offset: Duration, elapsed: Duration) -> Pace {
    match frame_offset.checked_sub(elapsed) {
        Some(wait) if !wait.is_zero() => Pace::WaitThenShow(wait),
        _ => Pace::Show,
    }
}

struct Running {
    request: PlaybackRequest,
    stream: FrameStream,
    started: Instant,
}

/// Starts the worker. Frames go to `deliver`, which returns `false` once the
/// UI is gone; the worker then exits.
pub(super) fn spawn_playback_worker(
    ffmpeg: Ffmpeg,
    deliver: impl Fn(StreamedFrame) -> bool + Send + 'static,
) -> Sender<PlaybackCommand> {
    let (commands, inbox) = mpsc::channel();
    thread::spawn(move || run_worker(&ffmpeg, &inbox, &deliver));
    commands
}

fn run_worker(
    ffmpeg: &Ffmpeg,
    inbox: &Receiver<PlaybackCommand>,
    deliver: &impl Fn(StreamedFrame) -> bool,
) {
    let mut running: Option<Running> = None;
    loop {
        let command = if running.is_some() {
            match inbox.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match inbox.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            }
        };
        if let Some(command) = command {
            // Only the newest command matters (fast seeking sends many).
            let latest = inbox.try_iter().last().unwrap_or(command);
            running = match latest {
                PlaybackCommand::Stop => None,
                PlaybackCommand::Start(request) => start(ffmpeg, request, deliver),
            };
            continue;
        }
        if let Some(current) = &mut running {
            match step(current, deliver) {
                Ok(true) => {}
                Ok(false) => running = None,
                Err(()) => return,
            }
        }
    }
}

fn start(
    ffmpeg: &Ffmpeg,
    request: PlaybackRequest,
    deliver: &impl Fn(StreamedFrame) -> bool,
) -> Option<Running> {
    match ffmpeg.stream_frames(&request.path, request.start, request.size, request.rate) {
        Ok(stream) => Some(Running {
            request,
            stream,
            started: Instant::now(),
        }),
        Err(error) => {
            deliver(failure(&request, request.start, error.to_string()));
            None
        }
    }
}

/// Decodes and releases one frame. `Ok(false)` when the run ended,
/// `Err(())` when the UI is gone.
fn step(running: &mut Running, deliver: &impl Fn(StreamedFrame) -> bool) -> Result<bool, ()> {
    let request = &running.request;
    match running.stream.next_frame() {
        Ok(Some((at, image))) => {
            let offset = Duration::from_micros(at.saturating_sub(request.start).as_micros());
            if let Pace::WaitThenShow(wait) = pace(offset, running.started.elapsed()) {
                thread::sleep(wait);
            }
            let frame = StreamedFrame {
                stream_id: request.stream_id,
                generation: request.generation,
                at,
                size: request.size,
                image: Ok(image),
            };
            if deliver(frame) { Ok(true) } else { Err(()) }
        }
        Ok(None) => Ok(false),
        Err(error) => {
            let delivered = deliver(failure(request, request.start, error.to_string()));
            if delivered { Ok(false) } else { Err(()) }
        }
    }
}

fn failure(request: &PlaybackRequest, at: Timestamp, error: String) -> StreamedFrame {
    StreamedFrame {
        stream_id: request.stream_id,
        generation: request.generation,
        at,
        size: request.size,
        image: Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    fn millis(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn early_frames_wait_for_the_clock() {
        assert_eq!(
            pace(millis(100), millis(40)),
            Pace::WaitThenShow(millis(60))
        );
    }

    #[test]
    fn due_and_late_frames_show_at_once() {
        assert_eq!(pace(millis(100), millis(100)), Pace::Show);
        assert_eq!(pace(millis(100), millis(900)), Pace::Show);
    }

    /// Writes a 2 s synthetic video with ffmpeg (required for the test suite).
    fn make_video(dir: &Path) -> PathBuf {
        let path = dir.join("in.mkv");
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-nostdin", "-v", "error", "-f", "lavfi"])
            .args([
                "-i",
                "testsrc=duration=2:size=160x120:rate=25",
                "-c:v",
                "mpeg4",
            ])
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    fn request(path: PathBuf, stream_id: u64) -> PlaybackRequest {
        PlaybackRequest {
            stream_id,
            generation: 1,
            path,
            start: Timestamp::from_millis(500),
            size: (80, 60),
            rate: FrameRate::new(20, 1).unwrap(),
        }
    }

    #[test]
    fn worker_streams_frames_in_real_time_until_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let video = make_video(dir.path());
        let frames = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&frames);
        let (done_sender, done) = mpsc::channel();
        let worker = spawn_playback_worker(Ffmpeg::from_path(), move |frame| {
            let mut frames = sink.lock().unwrap();
            frames.push((
                frame.stream_id,
                frame.at,
                frame.image.is_ok(),
                Instant::now(),
            ));
            if frames.len() == 25 {
                let _ = done_sender.send(()); // The test may have stopped waiting.
            }
            true
        });

        let started = Instant::now();
        worker
            .send(PlaybackCommand::Start(request(video, 7)))
            .unwrap();
        done.recv_timeout(Duration::from_secs(20))
            .expect("25 frames within 20 s");

        let frames = frames.lock().unwrap();
        assert!(frames.iter().all(|(id, _, ok, _)| *id == 7 && *ok));
        assert_eq!(frames[0].1, Timestamp::from_millis(500));
        assert_eq!(frames[1].1, Timestamp::from_millis(550));
        // Frame 25 is 1.2 s into the run; paced to the clock, it cannot be
        // released much earlier, however fast ffmpeg decodes.
        let last_shown = frames[24].3 - started;
        assert!(
            last_shown >= millis(1_150),
            "frames were not paced: {last_shown:?}"
        );
    }

    #[test]
    fn a_missing_file_is_reported_as_a_failed_frame() {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&frames);
        let (done_sender, done) = mpsc::channel();
        let worker = spawn_playback_worker(Ffmpeg::from_path(), move |frame| {
            sink.lock().unwrap().push(frame.image.is_err());
            let _ = done_sender.send(()); // The test may have stopped waiting.
            true
        });
        worker
            .send(PlaybackCommand::Start(request(
                PathBuf::from("missing-video.mkv"),
                1,
            )))
            .unwrap();
        done.recv_timeout(Duration::from_secs(20))
            .expect("a result within 20 s");
        assert_eq!(*frames.lock().unwrap(), [true]);
    }
}
