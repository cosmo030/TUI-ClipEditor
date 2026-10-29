use std::process::ExitCode;

use tui_clip_editor::cli::{self, Command};

/// Exit code for invalid command-line usage (BSD `EX_USAGE` convention).
const EXIT_USAGE: u8 = 64;

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "printing help, version, and errors is the program's command-line output"
)]
fn main() -> ExitCode {
    match cli::parse_args(std::env::args_os().skip(1)) {
        Ok(Command::Help) => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("clipedit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Run(_)) => {
            eprintln!("clipedit: the editor is not available in this build yet");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("clipedit: {error}\n\n{}", cli::USAGE);
            ExitCode::from(EXIT_USAGE)
        }
    }
}
