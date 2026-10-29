//! Deciding what to export where: modes, output file names, and jobs.
//!
//! Running the export is the ffmpeg adapter's job; this module only plans it.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::clip::Clip;

/// Container used for precise exports, which are re-encoded to H.264/AAC.
pub const PRECISE_EXTENSION: &str = "mp4";
/// Container used for fast copies when the input has no file extension.
const FALLBACK_COPY_EXTENSION: &str = "mkv";
/// Stem used when the input file name has no usable stem.
const FALLBACK_STEM: &str = "clip";

/// How clips are cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExportMode {
    /// Copy the streams without re-encoding: instant and lossless, but the
    /// clip starts at the keyframe at or before the in point.
    #[default]
    FastCopy,
    /// Re-encode: frame-accurate, slower, and lossy (high quality).
    Precise,
}

impl ExportMode {
    /// The other mode.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::FastCopy => Self::Precise,
            Self::Precise => Self::FastCopy,
        }
    }

    /// One-line explanation for the export dialog.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::FastCopy => "no re-encode, original quality; may start up to a few seconds early",
            Self::Precise => "re-encodes to H.264/AAC MP4; frame-accurate, slower",
        }
    }
}

impl fmt::Display for ExportMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FastCopy => "Fast copy",
            Self::Precise => "Precise",
        })
    }
}

/// One clip to export: everything the ffmpeg adapter needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportJob {
    /// The source video.
    pub input: PathBuf,
    /// The file to create. It did not exist when the job was planned.
    pub output: PathBuf,
    /// The range to export.
    pub clip: Clip,
    /// How to cut it.
    pub mode: ExportMode,
}

/// Plans one export job per clip, choosing output names in `output_dir`
/// that do not collide with existing files or with each other.
///
/// Names look like `holiday_00-01-20.000_00-01-25.500.mp4`; a collision adds
/// ` (2)`, ` (3)`, … before the extension. `exists` reports whether a path is
/// taken, so callers pass `Path::exists` and tests pass a fake.
#[must_use]
pub fn plan_exports(
    input: &Path,
    output_dir: &Path,
    clips: &[Clip],
    mode: ExportMode,
    exists: impl Fn(&Path) -> bool,
) -> Vec<ExportJob> {
    let stem = file_stem(input);
    let extension = output_extension(input, mode);
    let mut planned = HashSet::new();

    clips
        .iter()
        .map(|&clip| {
            let base = format!(
                "{stem}_{}_{}",
                clip.start().to_file_name_part(),
                clip.end().to_file_name_part()
            );
            let output = (1..)
                .map(|attempt| output_dir.join(numbered_file_name(&base, attempt, &extension)))
                .find(|candidate| !planned.contains(candidate) && !exists(candidate))
                .unwrap_or_else(|| unreachable!("the attempt counter is unbounded"));
            planned.insert(output.clone());
            ExportJob {
                input: input.to_path_buf(),
                output,
                clip,
                mode,
            }
        })
        .collect()
}

/// The directory exports go to by default: the input's own directory.
#[must_use]
pub fn default_output_dir(input: &Path) -> PathBuf {
    match input.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn numbered_file_name(base: &str, attempt: u32, extension: &str) -> String {
    if attempt == 1 {
        format!("{base}.{extension}")
    } else {
        format!("{base} ({attempt}).{extension}")
    }
}

fn file_stem(input: &Path) -> String {
    input
        .file_stem()
        .map(OsStr::to_string_lossy)
        .filter(|stem| !stem.trim().is_empty())
        .map_or_else(|| FALLBACK_STEM.to_owned(), |stem| stem.into_owned())
}

fn output_extension(input: &Path, mode: ExportMode) -> String {
    match mode {
        ExportMode::Precise => PRECISE_EXTENSION.to_owned(),
        ExportMode::FastCopy => input
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .filter(|extension| !extension.is_empty())
            .unwrap_or_else(|| FALLBACK_COPY_EXTENSION.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timecode::Timestamp;

    fn clip(start_ms: u64, end_ms: u64) -> Clip {
        Clip::new(
            Timestamp::from_millis(start_ms),
            Timestamp::from_millis(end_ms),
        )
        .unwrap()
    }

    fn nothing_exists(_: &Path) -> bool {
        false
    }

    fn output_names(jobs: &[ExportJob]) -> Vec<String> {
        jobs.iter()
            .map(|job| {
                job.output
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn fast_copy_keeps_the_input_container() {
        let jobs = plan_exports(
            Path::new("videos/Holiday.MKV"),
            Path::new("out"),
            &[clip(80_000, 85_500)],
            ExportMode::FastCopy,
            nothing_exists,
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].output,
            Path::new("out").join("Holiday_00-01-20.000_00-01-25.500.mkv")
        );
        assert_eq!(jobs[0].input, Path::new("videos/Holiday.MKV"));
        assert_eq!(jobs[0].mode, ExportMode::FastCopy);
    }

    #[test]
    fn precise_always_writes_mp4() {
        let jobs = plan_exports(
            Path::new("a.webm"),
            Path::new("."),
            &[clip(0, 1_000)],
            ExportMode::Precise,
            nothing_exists,
        );
        assert_eq!(output_names(&jobs), ["a_00-00-00.000_00-00-01.000.mp4"]);
    }

    #[test]
    fn existing_files_are_never_reused() {
        let taken = Path::new("out").join("a_00-00-00.000_00-00-01.000.mp4");
        let taken_2 = Path::new("out").join("a_00-00-00.000_00-00-01.000 (2).mp4");
        let jobs = plan_exports(
            Path::new("a.mp4"),
            Path::new("out"),
            &[clip(0, 1_000)],
            ExportMode::FastCopy,
            |path| path == taken || path == taken_2,
        );
        assert_eq!(output_names(&jobs), ["a_00-00-00.000_00-00-01.000 (3).mp4"]);
    }

    #[test]
    fn clips_with_the_same_name_in_one_plan_get_distinct_files() {
        // Two ranges that differ only below millisecond precision share a name.
        let first =
            Clip::new(Timestamp::from_micros(0), Timestamp::from_micros(1_000_100)).unwrap();
        let second =
            Clip::new(Timestamp::from_micros(0), Timestamp::from_micros(1_000_200)).unwrap();
        let jobs = plan_exports(
            Path::new("a.mp4"),
            Path::new("."),
            &[first, second],
            ExportMode::FastCopy,
            nothing_exists,
        );
        assert_eq!(
            output_names(&jobs),
            [
                "a_00-00-00.000_00-00-01.000.mp4",
                "a_00-00-00.000_00-00-01.000 (2).mp4"
            ]
        );
    }

    #[test]
    fn input_without_extension_copies_to_mkv() {
        let jobs = plan_exports(
            Path::new("recording"),
            Path::new("."),
            &[clip(0, 500)],
            ExportMode::FastCopy,
            nothing_exists,
        );
        assert_eq!(
            output_names(&jobs),
            ["recording_00-00-00.000_00-00-00.500.mkv"]
        );
    }

    #[test]
    fn no_clips_plans_no_jobs() {
        let jobs = plan_exports(
            Path::new("a.mp4"),
            Path::new("."),
            &[],
            ExportMode::Precise,
            nothing_exists,
        );
        assert!(jobs.is_empty());
    }

    #[test]
    fn default_output_dir_is_the_input_directory() {
        assert_eq!(
            default_output_dir(Path::new("videos/a.mp4")),
            Path::new("videos")
        );
        assert_eq!(default_output_dir(Path::new("a.mp4")), Path::new("."));
    }

    #[test]
    fn mode_toggles_and_describes_itself() {
        assert_eq!(ExportMode::FastCopy.toggled(), ExportMode::Precise);
        assert_eq!(ExportMode::Precise.toggled(), ExportMode::FastCopy);
        assert_eq!(ExportMode::Precise.to_string(), "Precise");
        assert!(ExportMode::FastCopy.description().contains("no re-encode"));
    }
}
