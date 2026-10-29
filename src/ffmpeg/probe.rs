//! Reading a file's duration and streams with `ffprobe`.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::process::run_with_timeout;
use super::{Ffmpeg, FfmpegError, os_args};
use crate::media::{FrameRate, MediaInfo, VideoStream};
use crate::timecode::Timestamp;

/// How long probing one file may take (network drives can be slow).
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Why ffprobe's description of a file cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// ffprobe printed something that is not the expected JSON.
    #[error("ffprobe output could not be read ({0})")]
    InvalidJson(String),
    /// The file has no video stream (for example, an audio file or cover art only).
    #[error("no video stream found")]
    NoVideoStream,
    /// The video stream does not report its size.
    #[error("the video stream has no width or height")]
    MissingDimensions,
    /// The video stream does not report a usable frame rate.
    #[error("the video stream has no frame rate")]
    MissingFrameRate,
    /// Neither the file nor the video stream reports a duration.
    #[error("the file has no known duration")]
    MissingDuration,
}

impl Ffmpeg {
    /// Reads the duration and video properties of `path`.
    ///
    /// # Errors
    ///
    /// Returns [`FfmpegError::Probe`] if the file is not a usable video, or
    /// another [`FfmpegError`] if ffprobe fails, is missing, or times out.
    pub fn probe(&self, path: &Path) -> Result<MediaInfo, FfmpegError> {
        let output = run_with_timeout(&self.ffprobe, &probe_args(path), PROBE_TIMEOUT)?
            .into_success(&self.ffprobe)?;
        parse_probe_json(&String::from_utf8_lossy(&output.stdout)).map_err(|reason| {
            FfmpegError::Probe {
                path: path.to_path_buf(),
                reason,
            }
        })
    }
}

fn probe_args(path: &Path) -> Vec<OsString> {
    let mut args = os_args(&[
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
    ]);
    args.push(path.as_os_str().to_owned());
    args
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    disposition: Disposition,
    #[serde(default)]
    side_data_list: Vec<SideData>,
    #[serde(default)]
    tags: StreamTags,
}

#[derive(Debug, Default, Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Debug, Deserialize)]
struct SideData {
    rotation: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamTags {
    rotate: Option<String>,
}

impl ProbeStream {
    fn is_video(&self) -> bool {
        self.codec_type.as_deref() == Some("video") && self.disposition.attached_pic == 0
    }

    fn is_audio(&self) -> bool {
        self.codec_type.as_deref() == Some("audio")
    }

    /// Whether the picture is displayed rotated by a quarter turn, which
    /// swaps width and height (typical for phone videos). ffmpeg rotates
    /// decoded frames automatically, so the preview must use display size.
    fn is_quarter_turned(&self) -> bool {
        let side_data_rotation = self.side_data_list.iter().find_map(|data| data.rotation);
        let tag_rotation = self
            .tags
            .rotate
            .as_deref()
            .and_then(|text| text.trim().parse::<f64>().ok());
        side_data_rotation
            .or(tag_rotation)
            .is_some_and(|degrees| (degrees.abs() % 180.0 - 90.0).abs() < 1.0)
    }
}

/// Turns ffprobe's JSON (`-show_format -show_streams`) into [`MediaInfo`].
fn parse_probe_json(json: &str) -> Result<MediaInfo, ProbeError> {
    let output: ProbeOutput =
        serde_json::from_str(json).map_err(|error| ProbeError::InvalidJson(error.to_string()))?;
    let video = output
        .streams
        .iter()
        .find(|stream| stream.is_video())
        .ok_or(ProbeError::NoVideoStream)?;

    let (Some(stored_width), Some(stored_height)) = (video.width, video.height) else {
        return Err(ProbeError::MissingDimensions);
    };
    if stored_width == 0 || stored_height == 0 {
        return Err(ProbeError::MissingDimensions);
    }
    let (width, height) = if video.is_quarter_turned() {
        (stored_height, stored_width)
    } else {
        (stored_width, stored_height)
    };

    let frame_rate = [&video.avg_frame_rate, &video.r_frame_rate]
        .into_iter()
        .find_map(|rate| rate.as_deref().and_then(parse_frame_rate))
        .ok_or(ProbeError::MissingFrameRate)?;

    let duration = output
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .or(video.duration.as_deref())
        .and_then(parse_seconds)
        .filter(|duration| *duration > Timestamp::ZERO)
        .ok_or(ProbeError::MissingDuration)?;

    Ok(MediaInfo {
        duration,
        video: VideoStream {
            width,
            height,
            frame_rate,
        },
        has_audio: output.streams.iter().any(ProbeStream::is_audio),
    })
}

/// Parses ffprobe's `"num/den"` notation; `"0/0"` means unknown.
fn parse_frame_rate(text: &str) -> Option<FrameRate> {
    let (numerator, denominator) = text.split_once('/')?;
    FrameRate::new(
        numerator.trim().parse().ok()?,
        denominator.trim().parse().ok()?,
    )
}

fn parse_seconds(text: &str) -> Option<Timestamp> {
    Timestamp::from_secs_f64(text.trim().parse().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed real ffprobe output for a 1080p H.264 file with AAC audio.
    const TYPICAL: &str = r#"{
        "streams": [
            { "index": 0, "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
              "r_frame_rate": "30000/1001", "avg_frame_rate": "30000/1001", "duration": "12.345000",
              "disposition": { "default": 1, "attached_pic": 0 } },
            { "index": 1, "codec_type": "audio", "codec_name": "aac", "duration": "12.300000" }
        ],
        "format": { "filename": "a.mp4", "duration": "12.400000" }
    }"#;

    #[test]
    fn typical_file_is_parsed() {
        let info = parse_probe_json(TYPICAL).unwrap();
        assert_eq!(info.duration, Timestamp::from_millis(12_400));
        assert_eq!(info.video.width, 1920);
        assert_eq!(info.video.height, 1080);
        assert_eq!(
            info.video.frame_rate,
            FrameRate::new(30_000, 1_001).unwrap()
        );
        assert!(info.has_audio);
    }

    #[test]
    fn stream_duration_is_used_when_the_container_has_none() {
        let json = r#"{ "streams": [ { "codec_type": "video", "width": 640, "height": 360,
            "avg_frame_rate": "25/1", "duration": "3.5" } ], "format": {} }"#;
        let info = parse_probe_json(json).unwrap();
        assert_eq!(info.duration, Timestamp::from_millis(3_500));
        assert!(!info.has_audio);
    }

    #[test]
    fn unknown_average_frame_rate_falls_back_to_r_frame_rate() {
        let json = r#"{ "streams": [ { "codec_type": "video", "width": 640, "height": 360,
            "avg_frame_rate": "0/0", "r_frame_rate": "24/1" } ], "format": { "duration": "1.0" } }"#;
        let info = parse_probe_json(json).unwrap();
        assert_eq!(info.video.frame_rate, FrameRate::new(24, 1).unwrap());
    }

    #[test]
    fn cover_art_is_not_treated_as_video() {
        let json = r#"{ "streams": [
            { "codec_type": "audio" },
            { "codec_type": "video", "width": 500, "height": 500, "avg_frame_rate": "0/0",
              "r_frame_rate": "90000/1", "disposition": { "attached_pic": 1 } }
        ], "format": { "duration": "200.0" } }"#;
        assert_eq!(parse_probe_json(json), Err(ProbeError::NoVideoStream));
    }

    #[test]
    fn quarter_turned_video_reports_display_size() {
        let side_data = r#"{ "streams": [ { "codec_type": "video", "width": 1920, "height": 1080,
            "avg_frame_rate": "30/1", "side_data_list": [ { "side_data_type": "Display Matrix", "rotation": -90 } ] } ],
            "format": { "duration": "5.0" } }"#;
        let info = parse_probe_json(side_data).unwrap();
        assert_eq!((info.video.width, info.video.height), (1080, 1920));

        let legacy_tag = r#"{ "streams": [ { "codec_type": "video", "width": 1920, "height": 1080,
            "avg_frame_rate": "30/1", "tags": { "rotate": "270" } } ], "format": { "duration": "5.0" } }"#;
        let info = parse_probe_json(legacy_tag).unwrap();
        assert_eq!((info.video.width, info.video.height), (1080, 1920));
    }

    #[test]
    fn half_turned_video_keeps_its_size() {
        let json = r#"{ "streams": [ { "codec_type": "video", "width": 1920, "height": 1080,
            "avg_frame_rate": "30/1", "side_data_list": [ { "rotation": 180 } ] } ],
            "format": { "duration": "5.0" } }"#;
        let info = parse_probe_json(json).unwrap();
        assert_eq!((info.video.width, info.video.height), (1920, 1080));
    }

    #[test]
    fn missing_pieces_are_reported_specifically() {
        let no_size = r#"{ "streams": [ { "codec_type": "video", "avg_frame_rate": "25/1" } ],
            "format": { "duration": "1" } }"#;
        assert_eq!(
            parse_probe_json(no_size),
            Err(ProbeError::MissingDimensions)
        );

        let zero_size = r#"{ "streams": [ { "codec_type": "video", "width": 0, "height": 0,
            "avg_frame_rate": "25/1" } ], "format": { "duration": "1" } }"#;
        assert_eq!(
            parse_probe_json(zero_size),
            Err(ProbeError::MissingDimensions)
        );

        let no_rate = r#"{ "streams": [ { "codec_type": "video", "width": 2, "height": 2,
            "avg_frame_rate": "0/0", "r_frame_rate": "0/0" } ], "format": { "duration": "1" } }"#;
        assert_eq!(parse_probe_json(no_rate), Err(ProbeError::MissingFrameRate));

        let no_duration = r#"{ "streams": [ { "codec_type": "video", "width": 2, "height": 2,
            "avg_frame_rate": "25/1" } ], "format": { "duration": "N/A" } }"#;
        assert_eq!(
            parse_probe_json(no_duration),
            Err(ProbeError::MissingDuration)
        );
    }

    #[test]
    fn non_json_output_is_an_error() {
        assert!(matches!(
            parse_probe_json("Invalid data found when processing input"),
            Err(ProbeError::InvalidJson(_))
        ));
    }

    #[test]
    fn frame_rate_notation_is_validated() {
        assert_eq!(parse_frame_rate("25/1"), FrameRate::new(25, 1));
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("25"), None);
        assert_eq!(parse_frame_rate("a/b"), None);
    }

    #[test]
    fn probe_arguments_end_with_the_path() {
        let args = probe_args(Path::new("my video.mp4"));
        assert_eq!(args.last(), Some(&OsString::from("my video.mp4")));
        assert!(args.contains(&OsString::from("-show_streams")));
    }
}
