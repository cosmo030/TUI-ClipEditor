//! Running one export job with progress reporting and cancellation.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use super::process::{failure_details, join_reader, kill_and_reap, read_in_background, spawn};
use super::{Ffmpeg, FfmpegError, os_args, program_name};
use crate::export::{ExportJob, ExportMode};
use crate::timecode::Timestamp;

/// How often a running export checks whether it was cancelled.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How an export that did not fail ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportOutcome {
    /// The clip was written completely.
    Completed,
    /// The export was cancelled and its partial file removed.
    Cancelled,
}

impl Ffmpeg {
    /// Exports `job.clip` from `job.input` to `job.output`.
    ///
    /// The output file is first created exclusively, so an existing file is
    /// never overwritten, and a partial file is only ever removed when this
    /// call created it. Missing output directories are created.
    ///
    /// `on_progress` receives the position written so far, relative to the
    /// clip start. Setting `cancel` stops ffmpeg within about 100 ms. There is
    /// no time limit: long clips legitimately take a long time.
    ///
    /// # Errors
    ///
    /// Returns [`FfmpegError::OutputExists`] if the output file already exists,
    /// [`FfmpegError::Failed`] if ffmpeg reports an error, or another
    /// [`FfmpegError`] for missing programs and I/O failures.
    pub fn export(
        &self,
        job: &ExportJob,
        mut on_progress: impl FnMut(Timestamp),
        cancel: &AtomicBool,
    ) -> Result<ExportOutcome, FfmpegError> {
        reserve_output(&job.output)?;
        let result = self.run_export(job, &mut on_progress, cancel);
        if !matches!(result, Ok(ExportOutcome::Completed)) {
            remove_partial_output(&job.output, result)
        } else {
            result
        }
    }

    fn run_export(
        &self,
        job: &ExportJob,
        on_progress: &mut impl FnMut(Timestamp),
        cancel: &AtomicBool,
    ) -> Result<ExportOutcome, FfmpegError> {
        let mut child = spawn(&self.ffmpeg, &export_args(job), Stdio::piped())?;
        let stderr = read_in_background(child.stderr.take());
        let progress = progress_lines(child.stdout.take());

        loop {
            if cancel.load(Ordering::Relaxed) {
                kill_and_reap(&mut child, &self.ffmpeg)?;
                // The reader may report an error once the pipe is cut; that
                // output is irrelevant after a cancel.
                drop(join_reader(stderr, &self.ffmpeg));
                return Ok(ExportOutcome::Cancelled);
            }
            match progress.recv_timeout(CANCEL_POLL_INTERVAL) {
                Ok(ProgressLine::OutTime(written)) => on_progress(written.min(job.clip.length())),
                Ok(ProgressLine::End | ProgressLine::Other) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        let status = child.wait().map_err(|source| FfmpegError::Io {
            action: format!("waiting for {}", program_name(&self.ffmpeg)),
            source,
        })?;
        let stderr = join_reader(stderr, &self.ffmpeg)?;
        if status.success() {
            Ok(ExportOutcome::Completed)
        } else {
            Err(FfmpegError::Failed {
                program: program_name(&self.ffmpeg),
                details: failure_details(status, &stderr),
            })
        }
    }
}

/// Creates the (empty) output file, failing if it exists. Holding the name
/// this way closes the gap between checking for a file and ffmpeg creating it.
fn reserve_output(output: &Path) -> Result<(), FfmpegError> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| FfmpegError::Io {
            action: format!("creating output directory {}", parent.display()),
            source,
        })?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map(drop)
        .map_err(|source| match source.kind() {
            io::ErrorKind::AlreadyExists => FfmpegError::OutputExists {
                path: output.to_path_buf(),
            },
            _ => FfmpegError::Io {
                action: format!("creating {}", output.display()),
                source,
            },
        })
}

/// Removes the file this export created after a cancel or failure, keeping
/// the original result. If removal fails, that is reported instead, because
/// the user must know a broken file was left behind.
fn remove_partial_output(
    output: &Path,
    result: Result<ExportOutcome, FfmpegError>,
) -> Result<ExportOutcome, FfmpegError> {
    match remove_file_when_released(output) {
        Ok(()) => result,
        Err(error) if error.kind() == io::ErrorKind::NotFound => result,
        Err(source) => Err(FfmpegError::Io {
            action: format!(
                "removing incomplete export {} (after: {})",
                output.display(),
                match &result {
                    Ok(_) => "cancel".to_owned(),
                    Err(error) => error.to_string(),
                }
            ),
            source,
        }),
    }
}

/// Deletes `path`, retrying briefly while another process still has it open.
///
/// On Windows a killed process releases its file handles shortly after it
/// exits, so an immediate delete can fail with a sharing violation. Deleting
/// is safe to repeat, and the retries are bounded.
fn remove_file_when_released(path: &Path) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match fs::remove_file(path) {
            Err(error) if is_file_in_use(&error) && attempt < REMOVE_ATTEMPTS => {
                thread::sleep(REMOVE_RETRY_DELAY * attempt);
                attempt += 1;
            }
            result => return result,
        }
    }
}

/// Windows `ERROR_SHARING_VIOLATION`: the file is open in another process.
const ERROR_SHARING_VIOLATION: i32 = 32;
/// Total attempts to delete a partial export (about 2 s of waiting in all).
const REMOVE_ATTEMPTS: u32 = 8;
/// Delay before the first retry; each later retry waits one step longer.
const REMOVE_RETRY_DELAY: Duration = Duration::from_millis(70);

fn is_file_in_use(error: &io::Error) -> bool {
    cfg!(windows) && error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
}

/// Builds the ffmpeg command line for a job.
///
/// The output was already created by [`reserve_output`], so `-y` only ever
/// overwrites that empty placeholder.
fn export_args(job: &ExportJob) -> Vec<OsString> {
    let start = job.clip.start().to_ffmpeg_seconds();
    let length = job.clip.length().to_ffmpeg_seconds();
    let mut args = os_args(&[
        "-hide_banner",
        "-nostdin",
        "-y",
        "-v",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
        "-ss",
        &start,
        "-i",
    ]);
    args.push(job.input.as_os_str().to_owned());
    args.extend(os_args(&["-t", &length]));
    args.extend(os_args(match job.mode {
        ExportMode::FastCopy => &["-c", "copy", "-avoid_negative_ts", "make_zero"],
        ExportMode::Precise => &[
            "-sn",
            "-dn",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
        ],
    }));
    args.push(job.output.as_os_str().to_owned());
    args
}

/// One `key=value` line of ffmpeg's `-progress` output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressLine {
    /// How far into the output ffmpeg has written.
    OutTime(Timestamp),
    /// ffmpeg finished writing.
    End,
    /// Any other key, or a value that is not available yet.
    Other,
}

fn parse_progress_line(line: &str) -> ProgressLine {
    match line.trim().split_once('=') {
        Some(("out_time_us", micros)) => {
            micros.parse::<u64>().map_or(ProgressLine::Other, |micros| {
                ProgressLine::OutTime(Timestamp::from_micros(micros))
            })
        }
        Some(("progress", "end")) => ProgressLine::End,
        _ => ProgressLine::Other,
    }
}

/// Parses progress lines on a background thread. The channel disconnects
/// when ffmpeg closes its output, which is how the caller learns it exited.
fn progress_lines(stdout: Option<ChildStdout>) -> mpsc::Receiver<ProgressLine> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let Some(stdout) = stdout else { return };
        for line in BufReader::new(stdout).lines() {
            // A read error means the pipe broke, which ends the stream just
            // like a normal exit; the exit status reports what happened.
            let Ok(line) = line else { return };
            if sender.send(parse_progress_line(&line)).is_err() {
                return;
            }
        }
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Clip;
    use std::path::PathBuf;

    fn job(mode: ExportMode) -> ExportJob {
        ExportJob {
            input: PathBuf::from("in put.mov"),
            output: PathBuf::from("out.mov"),
            clip: Clip::new(Timestamp::from_millis(1_500), Timestamp::from_millis(4_000)).unwrap(),
            mode,
        }
    }

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn value_after(args: &[String], flag: &str) -> String {
        let index = args.iter().position(|arg| arg == flag).unwrap();
        args[index + 1].clone()
    }

    #[test]
    fn fast_copy_seeks_then_copies_streams() {
        let args = text(&export_args(&job(ExportMode::FastCopy)));
        assert_eq!(value_after(&args, "-ss"), "1.500000");
        assert_eq!(value_after(&args, "-i"), "in put.mov");
        assert_eq!(value_after(&args, "-t"), "2.500000");
        assert_eq!(value_after(&args, "-c"), "copy");
        assert_eq!(value_after(&args, "-progress"), "pipe:1");
        assert_eq!(args.last().unwrap(), "out.mov");
        assert!(!args.contains(&"libx264".to_owned()));
    }

    #[test]
    fn precise_re_encodes_to_h264_and_aac() {
        let args = text(&export_args(&job(ExportMode::Precise)));
        assert_eq!(value_after(&args, "-c:v"), "libx264");
        assert_eq!(value_after(&args, "-c:a"), "aac");
        assert_eq!(value_after(&args, "-pix_fmt"), "yuv420p");
        assert!(!args.contains(&"copy".to_owned()));
    }

    #[test]
    fn seek_comes_before_input_and_length_after() {
        let args = text(&export_args(&job(ExportMode::Precise)));
        let position = |flag: &str| args.iter().position(|arg| arg == flag).unwrap();
        assert!(position("-ss") < position("-i"));
        assert!(position("-i") < position("-t"));
    }

    #[test]
    fn progress_lines_are_parsed() {
        assert_eq!(
            parse_progress_line("out_time_us=2500000"),
            ProgressLine::OutTime(Timestamp::from_micros(2_500_000))
        );
        assert_eq!(parse_progress_line("progress=end\r"), ProgressLine::End);
        assert_eq!(
            parse_progress_line("progress=continue"),
            ProgressLine::Other
        );
        assert_eq!(parse_progress_line("out_time_us=N/A"), ProgressLine::Other);
        assert_eq!(
            parse_progress_line("out_time_us=-9223372036854775807"),
            ProgressLine::Other
        );
        assert_eq!(parse_progress_line("frame=12"), ProgressLine::Other);
    }
}
