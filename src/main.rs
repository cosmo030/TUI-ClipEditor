use std::path::{self, Path, PathBuf};
use std::process::ExitCode;

use tui_clip_editor::cli::{self, Command, RunOptions};
use tui_clip_editor::ffmpeg::Ffmpeg;
use tui_clip_editor::tui::{self, Start};

/// Exit code for invalid command-line usage (BSD `EX_USAGE` convention).
const EXIT_USAGE: u8 = 64;

fn main() -> ExitCode {
    match cli::parse_args(std::env::args_os().skip(1)) {
        Ok(Command::Help) => print_out(cli::USAGE),
        Ok(Command::Version) => print_out(&format!("clipedit {}", env!("CARGO_PKG_VERSION"))),
        Ok(Command::Run(options)) => run(options),
        Err(error) => {
            print_error(&format!("{error}\n\n{}", cli::USAGE));
            ExitCode::from(EXIT_USAGE)
        }
    }
}

/// Checks ffmpeg, decides the first screen, and runs the UI.
fn run(options: RunOptions) -> ExitCode {
    let ffmpeg = Ffmpeg::from_path();
    if let Err(error) = ffmpeg.verify() {
        print_error(&error.to_string());
        return ExitCode::FAILURE;
    }
    let start = match first_screen(&ffmpeg, options.video) {
        Ok(start) => start,
        Err(message) => {
            print_error(&message);
            return ExitCode::FAILURE;
        }
    };
    match tui::run(start, options.output_dir, options.preview, ffmpeg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            print_error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

/// A video argument is probed before the UI starts, so a bad path is
/// reported on the command line instead of inside the UI.
fn first_screen(ffmpeg: &Ffmpeg, video: Option<PathBuf>) -> Result<Start, String> {
    let target = video.unwrap_or_else(|| PathBuf::from("."));
    let absolute = absolute(&target)?;
    if absolute.is_dir() {
        return Ok(Start::Browse(absolute));
    }
    if !absolute.exists() {
        return Err(format!("{}: no such file or folder", target.display()));
    }
    let media = ffmpeg.probe(&absolute).map_err(|error| error.to_string())?;
    Ok(Start::Edit {
        path: absolute,
        media,
    })
}

/// Makes `path` absolute without resolving links (unlike `canonicalize`,
/// which on Windows adds a `\\?\` prefix that is confusing to read).
fn absolute(path: &Path) -> Result<PathBuf, String> {
    path::absolute(path).map_err(|error| format!("cannot resolve {}: {error}", path.display()))
}

#[expect(
    clippy::print_stdout,
    reason = "help and version text is the program's command-line output"
)]
fn print_out(text: &str) -> ExitCode {
    println!("{text}");
    ExitCode::SUCCESS
}

#[expect(
    clippy::print_stderr,
    reason = "errors before or after the UI are reported on stderr"
)]
fn print_error(message: &str) {
    eprintln!("clipedit: {message}");
}
