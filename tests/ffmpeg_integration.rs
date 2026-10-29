//! Integration tests against the real ffmpeg and ffprobe on `PATH`.
//!
//! Each test generates its own small synthetic video in a temporary
//! directory, so no media files are stored in the repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;
use tui_clip_editor::clip::Clip;
use tui_clip_editor::export::{ExportJob, ExportMode};
use tui_clip_editor::ffmpeg::{ExportOutcome, Ffmpeg, FfmpegError, ProbeError};
use tui_clip_editor::media::FrameRate;
use tui_clip_editor::timecode::Timestamp;

const SOURCE_SECONDS: u64 = 4;
const SOURCE_SIZE: (u32, u32) = (320, 240);
/// Duration tolerance for exports: one frame at 25 fps plus audio padding.
const PRECISE_TOLERANCE_MICROS: u64 = 100_000;

/// Writes a 4 s, 320x240, 25 fps test video with a 440 Hz tone and a
/// keyframe every second. Uses encoders built into every ffmpeg.
fn make_test_video(dir: &Path) -> PathBuf {
    let path = dir.join("source.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(format!(
            "testsrc=duration={SOURCE_SECONDS}:size={}x{}:rate=25",
            SOURCE_SIZE.0, SOURCE_SIZE.1
        ))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={SOURCE_SECONDS}"))
        .args([
            "-c:v",
            "mpeg4",
            "-q:v",
            "5",
            "-g",
            "25",
            "-c:a",
            "flac",
            "-shortest",
        ])
        .arg(&path)
        .stdin(Stdio::null())
        .status()
        .expect("ffmpeg must be installed and on PATH to run these tests");
    assert!(status.success(), "generating the test video failed");
    path
}

fn fixture() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("creating a temporary directory");
    let video = make_test_video(dir.path());
    (dir, video)
}

fn clip(start_ms: u64, end_ms: u64) -> Clip {
    Clip::new(
        Timestamp::from_millis(start_ms),
        Timestamp::from_millis(end_ms),
    )
    .expect("test clips have end after start")
}

fn job(input: &Path, output: PathBuf, clip: Clip, mode: ExportMode) -> ExportJob {
    ExportJob {
        input: input.to_path_buf(),
        output,
        clip,
        mode,
    }
}

fn not_cancelled() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn verify_succeeds_with_installed_ffmpeg() {
    Ffmpeg::from_path().verify().unwrap();
}

#[test]
fn verify_reports_missing_programs_by_name() {
    let ffmpeg = Ffmpeg::with_programs("clipedit-missing-ffmpeg", "ffprobe");
    let error = ffmpeg.verify().unwrap_err();
    assert!(
        matches!(&error, FfmpegError::NotFound { program } if program == "clipedit-missing-ffmpeg"),
        "got: {error:?}"
    );
}

#[test]
fn probe_reports_duration_size_rate_and_audio() {
    let (_dir, video) = fixture();
    let info = Ffmpeg::from_path().probe(&video).unwrap();
    let duration_error = info
        .duration
        .as_micros()
        .abs_diff(SOURCE_SECONDS * 1_000_000);
    assert!(duration_error < 50_000, "duration was {}", info.duration);
    assert_eq!((info.video.width, info.video.height), SOURCE_SIZE);
    assert_eq!(
        info.video.frame_rate.frame_duration(),
        Timestamp::from_millis(40)
    );
    assert!(info.has_audio);
}

#[test]
fn probe_rejects_a_file_that_is_not_a_video() {
    let dir = tempfile::tempdir().unwrap();
    let text_file = dir.path().join("notes.txt");
    fs::write(&text_file, "not a video").unwrap();
    let error = Ffmpeg::from_path().probe(&text_file).unwrap_err();
    assert!(
        matches!(
            error,
            FfmpegError::Failed { .. } | FfmpegError::Probe { .. }
        ),
        "got: {error:?}"
    );
}

#[test]
fn probe_rejects_audio_only_files() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("tone.flac");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("sine=frequency=440:duration=1")
        .arg(&audio)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let error = Ffmpeg::from_path().probe(&audio).unwrap_err();
    assert!(
        matches!(
            error,
            FfmpegError::Probe {
                reason: ProbeError::NoVideoStream,
                ..
            }
        ),
        "got: {error:?}"
    );
}

#[test]
fn extract_frame_returns_an_rgb_image_of_the_requested_size() {
    let (_dir, video) = fixture();
    let frame = Ffmpeg::from_path()
        .extract_frame(&video, Timestamp::from_millis(1_000), (160, 120))
        .unwrap();
    assert_eq!(frame.dimensions(), (160, 120));
    // testsrc is colourful; an all-black frame would mean decoding failed.
    assert!(frame.pixels().any(|pixel| pixel.0 != [0, 0, 0]));
}

#[test]
fn extract_frame_past_the_end_reports_no_frame() {
    let (_dir, video) = fixture();
    let error = Ffmpeg::from_path()
        .extract_frame(&video, Timestamp::from_secs(60), (160, 120))
        .unwrap_err();
    assert!(
        matches!(&error, FfmpegError::InvalidOutput { reason, .. } if reason.contains("no frame")),
        "got: {error:?}"
    );
}

#[test]
fn precise_export_has_the_exact_length_and_reports_progress() {
    let (dir, video) = fixture();
    let output = dir.path().join("clips").join("precise.mp4");
    let mut progress = Vec::new();
    let outcome = Ffmpeg::from_path()
        .export(
            &job(
                &video,
                output.clone(),
                clip(1_200, 2_600),
                ExportMode::Precise,
            ),
            |written| progress.push(written),
            &not_cancelled(),
        )
        .unwrap();

    assert_eq!(outcome, ExportOutcome::Completed);
    let info = Ffmpeg::from_path().probe(&output).unwrap();
    let error = info.duration.as_micros().abs_diff(1_400_000);
    assert!(
        error <= PRECISE_TOLERANCE_MICROS,
        "duration was {}",
        info.duration
    );
    assert!(!progress.is_empty(), "no progress was reported");
    assert!(
        progress
            .iter()
            .all(|written| *written <= Timestamp::from_millis(1_400))
    );
}

#[test]
fn fast_copy_export_keeps_the_container_and_roughly_the_length() {
    let (dir, video) = fixture();
    let output = dir.path().join("copy.mkv");
    // Starts on a keyframe (every 1 s), so stream copy can cut exactly here.
    let outcome = Ffmpeg::from_path()
        .export(
            &job(
                &video,
                output.clone(),
                clip(1_000, 3_000),
                ExportMode::FastCopy,
            ),
            |_| {},
            &not_cancelled(),
        )
        .unwrap();

    assert_eq!(outcome, ExportOutcome::Completed);
    let info = Ffmpeg::from_path().probe(&output).unwrap();
    let error = info.duration.as_micros().abs_diff(2_000_000);
    assert!(error <= 250_000, "duration was {}", info.duration);
}

#[test]
fn export_never_overwrites_an_existing_file() {
    let (dir, video) = fixture();
    let output = dir.path().join("taken.mp4");
    fs::write(&output, "precious").unwrap();

    let error = Ffmpeg::from_path()
        .export(
            &job(&video, output.clone(), clip(0, 1_000), ExportMode::Precise),
            |_| {},
            &not_cancelled(),
        )
        .unwrap_err();

    assert!(
        matches!(error, FfmpegError::OutputExists { .. }),
        "got: {error:?}"
    );
    assert_eq!(fs::read_to_string(&output).unwrap(), "precious");
}

#[test]
fn cancelled_export_removes_its_partial_file() {
    let (dir, video) = fixture();
    let output = dir.path().join("cancelled.mp4");
    let outcome = Ffmpeg::from_path()
        .export(
            &job(&video, output.clone(), clip(0, 4_000), ExportMode::Precise),
            |_| {},
            &AtomicBool::new(true),
        )
        .unwrap();

    assert_eq!(outcome, ExportOutcome::Cancelled);
    assert!(!output.exists(), "partial output was left behind");
}

#[test]
fn failed_export_removes_its_file_and_reports_ffmpeg_errors() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_video = dir.path().join("broken.mkv");
    fs::write(&not_a_video, "garbage").unwrap();
    let output = dir.path().join("out.mkv");

    let error = Ffmpeg::from_path()
        .export(
            &job(
                &not_a_video,
                output.clone(),
                clip(0, 1_000),
                ExportMode::FastCopy,
            ),
            |_| {},
            &not_cancelled(),
        )
        .unwrap_err();

    assert!(
        matches!(&error, FfmpegError::Failed { program, .. } if program == "ffmpeg"),
        "got: {error:?}"
    );
    assert!(!output.exists(), "the placeholder file was left behind");
}

#[test]
fn frame_stream_yields_consecutive_frames_until_the_end() {
    let (_dir, video) = fixture();
    let rate = FrameRate::new(10, 1).unwrap();
    let mut stream = Ffmpeg::from_path()
        .stream_frames(&video, Timestamp::from_secs(1), (160, 120), rate)
        .unwrap();

    let mut positions = Vec::new();
    while let Some((at, frame)) = stream.next_frame().unwrap() {
        assert_eq!(frame.dimensions(), (160, 120));
        positions.push(at);
    }

    // 3 s remain after the start at 10 fps; allow one frame either way.
    assert!(
        (29..=31).contains(&positions.len()),
        "got {} frames",
        positions.len()
    );
    assert_eq!(positions[0], Timestamp::from_secs(1));
    assert_eq!(positions[1], Timestamp::from_millis(1_100));
    assert!(stream.next_frame().unwrap().is_none(), "stays ended");
}

#[test]
fn dropping_a_frame_stream_stops_ffmpeg_promptly() {
    let (_dir, video) = fixture();
    let rate = FrameRate::new(25, 1).unwrap();
    let mut stream = Ffmpeg::from_path()
        .stream_frames(&video, Timestamp::ZERO, (160, 120), rate)
        .unwrap();
    assert!(stream.next_frame().unwrap().is_some());

    let started = std::time::Instant::now();
    drop(stream);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "stopping the stream took too long"
    );
}
