//! Running a short-lived program with a time limit and captured output.

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{FfmpegError, program_name};

/// How often a running program is checked for completion.
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// How many lines of error output are kept for error messages.
const STDERR_SUMMARY_LINES: usize = 5;

/// Output of a finished program.
#[derive(Debug)]
pub(super) struct ProcessOutput {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

impl ProcessOutput {
    /// Returns the output if the program succeeded, or a [`FfmpegError::Failed`]
    /// carrying the end of its error output.
    pub(super) fn into_success(self, program: &Path) -> Result<Self, FfmpegError> {
        if self.status.success() {
            Ok(self)
        } else {
            Err(FfmpegError::Failed {
                program: program_name(program),
                details: failure_details(self.status, &self.stderr),
            })
        }
    }
}

/// Starts `program` with `args`, waits at most `timeout`, and returns its
/// output. The program is killed if it runs too long.
pub(super) fn run_with_timeout(
    program: &Path,
    args: &[OsString],
    timeout: Duration,
) -> Result<ProcessOutput, FfmpegError> {
    let mut child = spawn(program, args, Stdio::piped())?;
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());

    let status = wait_with_timeout(&mut child, program, timeout)?;
    Ok(ProcessOutput {
        status,
        stdout: join_reader(stdout, program)?,
        stderr: join_reader(stderr, program)?,
    })
}

/// Starts `program` with no stdin, the given stdout, and piped stderr.
pub(super) fn spawn(
    program: &Path,
    args: &[OsString],
    stdout: Stdio,
) -> Result<Child, FfmpegError> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => FfmpegError::NotFound {
                program: program_name(program),
            },
            _ => FfmpegError::Spawn {
                program: program_name(program),
                source,
            },
        })
}

/// Reads a pipe to the end on a separate thread so the child never blocks on
/// a full pipe buffer.
pub(super) fn read_in_background<R>(pipe: Option<R>) -> JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut pipe) = pipe {
            pipe.read_to_end(&mut buffer)?;
        }
        Ok(buffer)
    })
}

/// Waits for a reader thread and returns what it read.
pub(super) fn join_reader(
    reader: JoinHandle<io::Result<Vec<u8>>>,
    program: &Path,
) -> Result<Vec<u8>, FfmpegError> {
    let action = || format!("reading output of {}", program_name(program));
    reader
        .join()
        .map_err(|_| FfmpegError::Io {
            action: action(),
            source: io::Error::other("output reader thread panicked"),
        })?
        .map_err(|source| FfmpegError::Io {
            action: action(),
            source,
        })
}

/// Stops a child process (and, on Windows, its child processes) and reaps it.
pub(super) fn kill_and_reap(child: &mut Child, program: &Path) -> Result<(), FfmpegError> {
    kill_process_tree(child);
    // `kill` fails with InvalidInput if the process already exited, which is
    // the outcome we want, so only other errors are reported.
    match child.kill() {
        Err(error) if error.kind() != io::ErrorKind::InvalidInput => {
            return Err(FfmpegError::Io {
                action: format!("stopping {}", program_name(program)),
                source: error,
            });
        }
        _ => {}
    }
    child.wait().map_err(|source| FfmpegError::Io {
        action: format!("waiting for {} to stop", program_name(program)),
        source,
    })?;
    Ok(())
}

/// On Windows, package managers such as Chocolatey and Scoop install
/// `ffmpeg.exe` as a small launcher ("shim") that starts the real ffmpeg as
/// its own child. Killing only the shim leaves ffmpeg running, still writing
/// the file and holding our pipes open, so the whole tree is stopped.
///
/// Failure is not reported here: the caller's `Child::kill` that follows
/// still stops the direct child, and its error handling applies.
#[cfg(windows)]
fn kill_process_tree(child: &Child) {
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let taskkill = Path::new(&system_root)
        .join("System32")
        .join("taskkill.exe");
    // The result is deliberately ignored, as explained above.
    drop(
        Command::new(taskkill)
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    );
}

/// On other platforms, launchers replace themselves with ffmpeg (`exec`), so
/// killing the direct child is enough.
#[cfg(not(windows))]
const fn kill_process_tree(_child: &Child) {}

fn wait_with_timeout(
    child: &mut Child,
    program: &Path,
    timeout: Duration,
) -> Result<ExitStatus, FfmpegError> {
    let started = Instant::now();
    loop {
        let finished = child.try_wait().map_err(|source| FfmpegError::Io {
            action: format!("waiting for {}", program_name(program)),
            source,
        })?;
        if let Some(status) = finished {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            kill_and_reap(child, program)?;
            return Err(FfmpegError::Timeout {
                program: program_name(program),
                seconds: timeout.as_secs(),
            });
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// A one-line description of a failed run: the exit status plus the last
/// non-empty lines of error output.
pub(super) fn failure_details(status: ExitStatus, stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = &lines[lines.len().saturating_sub(STDERR_SUMMARY_LINES)..];
    if tail.is_empty() {
        format!("exited with {status} and no error output")
    } else {
        format!("{} ({status})", tail.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real non-zero exit status (`ExitStatus` cannot be built portably).
    /// `cargo` is always present when tests run and rejects this flag.
    fn failed_status() -> ExitStatus {
        Command::new("cargo")
            .arg("--definitely-not-a-flag")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
    }

    #[test]
    fn failure_details_keeps_only_the_last_lines() {
        let stderr = b"line 1\n\nline 2\nline 3\nline 4\nline 5\n  line 6  \n";
        let details = failure_details(failed_status(), stderr);
        assert!(
            details.starts_with("line 2; line 3; line 4; line 5; line 6 ("),
            "got: {details}"
        );
    }

    #[test]
    fn failure_details_without_output_says_so() {
        let details = failure_details(failed_status(), b"  \n");
        assert!(details.ends_with("and no error output"), "got: {details}");
    }

    #[test]
    fn missing_program_is_reported_as_not_found() {
        let result = run_with_timeout(
            Path::new("clipedit-test-program-that-does-not-exist"),
            &[],
            Duration::from_secs(1),
        );
        assert!(
            matches!(result, Err(FfmpegError::NotFound { ref program }) if program == "clipedit-test-program-that-does-not-exist"),
            "got: {result:?}"
        );
    }

    #[test]
    fn successful_program_output_is_captured() {
        let output = run_with_timeout(
            Path::new("cargo"),
            &["--version".into()],
            Duration::from_secs(30),
        )
        .unwrap()
        .into_success(Path::new("cargo"))
        .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
    }

    #[test]
    fn program_running_past_the_timeout_is_stopped() {
        // Decodes an endless synthetic source; only the timeout can end it.
        let args: Vec<OsString> = [
            "-v", "error", "-f", "lavfi", "-i", "nullsrc", "-f", "null", "-",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let started = Instant::now();
        let result = run_with_timeout(Path::new("ffmpeg"), &args, Duration::from_millis(300));
        assert!(
            matches!(result, Err(FfmpegError::Timeout { ref program, seconds: 0 }) if program == "ffmpeg"),
            "got: {result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the child was not stopped promptly"
        );
    }
}
