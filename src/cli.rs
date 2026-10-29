//! Command-line argument parsing.
//!
//! The interface is small enough that a hand-written parser is clearer than
//! pulling in an argument-parsing framework.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// Usage text shown for `--help` and after argument errors.
pub const USAGE: &str = "\
Usage: clipedit [OPTIONS] [VIDEO]

Open VIDEO in the clip editor. Without VIDEO, a file browser opens in the
current directory.

Options:
  -o, --output-dir <DIR>  Write exported clips to DIR (default: the video's directory)
  -p, --preview <MODE>    How to draw the preview: auto, blocks, sixel, kitty, or iterm2
                          (default: auto, which falls back to blocks unless the terminal
                          confirms image support)
  -h, --help              Print this help and exit
  -V, --version           Print the version and exit";

/// How the preview image is drawn in the terminal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PreviewMode {
    /// Use an image protocol only when the terminal confirms it; else blocks.
    #[default]
    Auto,
    /// Coloured half-block characters; works in every terminal.
    Blocks,
    /// The sixel graphics protocol.
    Sixel,
    /// The kitty graphics protocol.
    Kitty,
    /// iTerm2 inline images (also used by WezTerm and VS Code with images enabled).
    Iterm2,
}

impl PreviewMode {
    const NAMES: [(&'static str, Self); 5] = [
        ("auto", Self::Auto),
        ("blocks", Self::Blocks),
        ("sixel", Self::Sixel),
        ("kitty", Self::Kitty),
        ("iterm2", Self::Iterm2),
    ];

    fn parse(value: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(value))
            .map(|&(_, mode)| mode)
    }
}

/// What the user asked the program to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start the editor.
    Run(RunOptions),
    /// Print usage and exit.
    Help,
    /// Print the version and exit.
    Version,
}

/// Options for starting the editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// Video to open immediately; `None` opens the file browser.
    pub video: Option<PathBuf>,
    /// Directory for exported clips; `None` uses the video's directory.
    pub output_dir: Option<PathBuf>,
    /// How to draw the preview.
    pub preview: PreviewMode,
}

/// An invalid command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// An option that takes a value was given without one.
    MissingValue(&'static str),
    /// An option that is not recognized.
    UnknownOption(String),
    /// More than one video path was given.
    UnexpectedArgument(String),
    /// `--preview` was given a value it does not accept.
    InvalidPreviewMode(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(option) => write!(f, "option {option} requires a value"),
            Self::UnknownOption(option) => write!(f, "unknown option: {option}"),
            Self::UnexpectedArgument(argument) => {
                write!(
                    f,
                    "unexpected argument: {argument} (only one VIDEO can be opened)"
                )
            }
            Self::InvalidPreviewMode(value) => write!(
                f,
                "invalid preview mode: {value} (expected auto, blocks, sixel, kitty, or iterm2)"
            ),
        }
    }
}

impl std::error::Error for CliError {}

/// Parses the program arguments, excluding the program name.
///
/// `--help` and `--version` win over everything else so that they work even
/// when other arguments are wrong.
///
/// # Errors
///
/// Returns a [`CliError`] for unknown options, missing option values, or
/// more than one video path.
pub fn parse_args<I>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut options = RunOptions::default();
    let mut args = args.into_iter();
    let mut first_error = None;

    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "-o" | "--output-dir" => match args.next() {
                Some(dir) => options.output_dir = Some(PathBuf::from(dir)),
                None => {
                    first_error.get_or_insert(CliError::MissingValue("--output-dir"));
                }
            },
            "-p" | "--preview" => match parse_preview(args.next()) {
                Ok(mode) => options.preview = mode,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            },
            option if option.starts_with('-') => {
                first_error.get_or_insert_with(|| CliError::UnknownOption(option.to_owned()));
            }
            _ if options.video.is_some() => {
                first_error.get_or_insert_with(|| CliError::UnexpectedArgument(text.into_owned()));
            }
            _ => options.video = Some(PathBuf::from(arg)),
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(Command::Run(options)),
    }
}

fn parse_preview(value: Option<OsString>) -> Result<PreviewMode, CliError> {
    let value = value.ok_or(CliError::MissingValue("--preview"))?;
    let text = value.to_string_lossy();
    PreviewMode::parse(&text).ok_or_else(|| CliError::InvalidPreviewMode(text.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, CliError> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_runs_with_file_browser() {
        assert_eq!(parse(&[]), Ok(Command::Run(RunOptions::default())));
    }

    #[test]
    fn positional_argument_is_the_video() {
        assert_eq!(
            parse(&["holiday.mp4"]),
            Ok(Command::Run(RunOptions {
                video: Some(PathBuf::from("holiday.mp4")),
                ..RunOptions::default()
            }))
        );
    }

    #[test]
    fn output_dir_accepts_short_and_long_forms() {
        let expected = Ok(Command::Run(RunOptions {
            video: Some(PathBuf::from("in.mkv")),
            output_dir: Some(PathBuf::from("clips")),
            ..RunOptions::default()
        }));
        assert_eq!(parse(&["-o", "clips", "in.mkv"]), expected);
        assert_eq!(parse(&["in.mkv", "--output-dir", "clips"]), expected);
    }

    #[test]
    fn preview_defaults_to_auto_and_accepts_every_mode() {
        assert_eq!(RunOptions::default().preview, PreviewMode::Auto);
        for (name, mode) in PreviewMode::NAMES {
            let Ok(Command::Run(options)) = parse(&["--preview", name]) else {
                panic!("--preview {name} was rejected");
            };
            assert_eq!(options.preview, mode);
        }
        let Ok(Command::Run(options)) = parse(&["-p", "BLOCKS"]) else {
            panic!("-p BLOCKS was rejected");
        };
        assert_eq!(options.preview, PreviewMode::Blocks, "case-insensitive");
    }

    #[test]
    fn preview_rejects_unknown_or_missing_modes() {
        assert_eq!(
            parse(&["--preview", "ascii"]),
            Err(CliError::InvalidPreviewMode("ascii".to_owned()))
        );
        assert_eq!(parse(&["-p"]), Err(CliError::MissingValue("--preview")));
        assert_eq!(
            CliError::InvalidPreviewMode("ascii".to_owned()).to_string(),
            "invalid preview mode: ascii (expected auto, blocks, sixel, kitty, or iterm2)"
        );
    }

    #[test]
    fn help_and_version_win_over_invalid_arguments() {
        assert_eq!(parse(&["--bogus", "-h"]), Ok(Command::Help));
        assert_eq!(
            parse(&["a.mp4", "b.mp4", "--version"]),
            Ok(Command::Version)
        );
        assert_eq!(parse(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn output_dir_without_value_is_an_error() {
        assert_eq!(
            parse(&["clip.mp4", "-o"]),
            Err(CliError::MissingValue("--output-dir"))
        );
    }

    #[test]
    fn unknown_option_is_an_error() {
        assert_eq!(
            parse(&["--fast"]),
            Err(CliError::UnknownOption("--fast".to_owned()))
        );
    }

    #[test]
    fn second_video_is_an_error() {
        assert_eq!(
            parse(&["a.mp4", "b.mp4"]),
            Err(CliError::UnexpectedArgument("b.mp4".to_owned()))
        );
    }

    #[test]
    fn error_messages_name_the_problem() {
        assert_eq!(
            CliError::MissingValue("--output-dir").to_string(),
            "option --output-dir requires a value"
        );
        assert_eq!(
            CliError::UnexpectedArgument("b.mp4".to_owned()).to_string(),
            "unexpected argument: b.mp4 (only one VIDEO can be opened)"
        );
    }
}
