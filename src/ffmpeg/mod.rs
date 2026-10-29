//! Adapter for the external `ffmpeg` and `ffprobe` programs.
//!
//! Everything ffmpeg-specific (command lines, output formats, error text)
//! stays in this module and is translated into the application's own types.
//! Programs are always started with an argument list, never through a shell.

mod export;
mod frame;
mod probe;
mod process;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use export::ExportOutcome;
pub use frame::fit_within;
pub use probe::ProbeError;

use process::run_with_timeout;

/// How long `-version` may take before ffmpeg is considered unusable.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// Errors from running ffmpeg or ffprobe.
#[derive(Debug, thiserror::Error)]
pub enum FfmpegError {
    /// The program is not installed or not on `PATH`.
    #[error("{program} was not found; install ffmpeg and make sure `{program}` is on PATH")]
    NotFound {
        /// Program that was looked for.
        program: String,
    },
    /// The program could not be started for another reason.
    #[error("could not start {program}: {source}")]
    Spawn {
        /// Program that failed to start.
        program: String,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// The program ran longer than allowed and was stopped.
    #[error("{program} did not finish within {seconds} s and was stopped")]
    Timeout {
        /// Program that timed out.
        program: String,
        /// The limit that was exceeded.
        seconds: u64,
    },
    /// The program exited with an error.
    #[error("{program} failed: {details}")]
    Failed {
        /// Program that failed.
        program: String,
        /// The last lines of its error output.
        details: String,
    },
    /// The program's output could not be understood.
    #[error("unexpected output from {program}: {reason}")]
    InvalidOutput {
        /// Program whose output was invalid.
        program: String,
        /// What was wrong.
        reason: String,
    },
    /// The file is not a video the editor can open.
    #[error("cannot open {}: {reason}", path.display())]
    Probe {
        /// File that was probed.
        path: PathBuf,
        /// Why it cannot be used.
        reason: ProbeError,
    },
    /// The export target already exists.
    #[error("{} already exists; refusing to overwrite it", path.display())]
    OutputExists {
        /// The existing file.
        path: PathBuf,
    },
    /// A file-system or pipe operation failed.
    #[error("{action}: {source}")]
    Io {
        /// What was being done.
        action: String,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
}

/// Handle to the ffmpeg and ffprobe programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ffmpeg {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

impl Default for Ffmpeg {
    fn default() -> Self {
        Self::from_path()
    }
}

impl Ffmpeg {
    /// Uses `ffmpeg` and `ffprobe` from `PATH`.
    #[must_use]
    pub fn from_path() -> Self {
        Self::with_programs("ffmpeg", "ffprobe")
    }

    /// Uses specific program paths (mainly for tests).
    #[must_use]
    pub fn with_programs(ffmpeg: impl Into<PathBuf>, ffprobe: impl Into<PathBuf>) -> Self {
        Self {
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
        }
    }

    /// Checks that both programs start and report a version, so a missing
    /// install fails at startup with a clear message.
    ///
    /// # Errors
    ///
    /// Returns [`FfmpegError::NotFound`] if a program is missing, or another
    /// [`FfmpegError`] if it does not run correctly.
    pub fn verify(&self) -> Result<(), FfmpegError> {
        for program in [&self.ffmpeg, &self.ffprobe] {
            let output = run_with_timeout(program, &["-version".into()], VERSION_TIMEOUT)?;
            output.into_success(program)?;
        }
        Ok(())
    }
}

/// Converts string arguments for `Command::args`.
fn os_args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

/// The name used for a program in messages: its file name, not the full path.
fn program_name(program: &Path) -> String {
    program
        .file_name()
        .unwrap_or(program.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_name_drops_the_directory() {
        assert_eq!(
            program_name(Path::new("/usr/local/bin/ffmpeg")),
            "ffmpeg".to_owned()
        );
        assert_eq!(program_name(Path::new("ffprobe")), "ffprobe".to_owned());
    }

    #[test]
    fn not_found_message_tells_the_user_what_to_do() {
        let error = FfmpegError::NotFound {
            program: "ffprobe".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "ffprobe was not found; install ffmpeg and make sure `ffprobe` is on PATH"
        );
    }
}
