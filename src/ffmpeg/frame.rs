//! Decoding a single preview frame as raw RGB pixels.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use image::RgbImage;

use super::process::run_with_timeout;
use super::{Ffmpeg, FfmpegError, os_args, program_name};
use crate::timecode::Timestamp;

/// How long decoding one preview frame may take.
const FRAME_TIMEOUT: Duration = Duration::from_secs(15);
const BYTES_PER_RGB_PIXEL: usize = 3;

impl Ffmpeg {
    /// Decodes the frame shown at `at` in `path`, scaled to exactly `size`
    /// (`width`, `height`) pixels.
    ///
    /// Use [`fit_within`] to pick a `size` that keeps the aspect ratio.
    ///
    /// # Errors
    ///
    /// Returns [`FfmpegError::InvalidOutput`] if ffmpeg produced no frame (for
    /// example, `at` is past the last frame), or another [`FfmpegError`] if
    /// ffmpeg fails, is missing, or times out.
    pub fn extract_frame(
        &self,
        path: &Path,
        at: Timestamp,
        size: (u32, u32),
    ) -> Result<RgbImage, FfmpegError> {
        let output = run_with_timeout(&self.ffmpeg, &frame_args(path, at, size), FRAME_TIMEOUT)?
            .into_success(&self.ffmpeg)?;
        decode_rgb(output.stdout, size).map_err(|reason| FfmpegError::InvalidOutput {
            program: program_name(&self.ffmpeg),
            reason: format!("{reason} at {at}"),
        })
    }
}

/// The largest size that fits inside `bounds`, keeps the aspect ratio of
/// `source`, never upscales, and has even sides of at least 2 pixels (some
/// scalers and encoders reject odd sizes).
#[must_use]
pub fn fit_within(source: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    let (source_width, source_height) = (f64::from(source.0.max(1)), f64::from(source.1.max(1)));
    let scale = (f64::from(bounds.0) / source_width)
        .min(f64::from(bounds.1) / source_height)
        .min(1.0);
    // Both products are within u32 range because scale <= 1.
    let even = |length: f64| (((length * scale) as u32) & !1).max(2);
    (even(source_width), even(source_height))
}

fn frame_args(path: &Path, at: Timestamp, (width, height): (u32, u32)) -> Vec<OsString> {
    let position = at.to_ffmpeg_seconds();
    let scale = format!("scale={width}:{height}");
    // `-ss` before `-i` seeks in the input; when decoding, ffmpeg still
    // discards frames up to the exact position, so the frame is accurate.
    let mut args = os_args(&[
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-ss",
        &position,
        "-i",
    ]);
    args.push(path.as_os_str().to_owned());
    args.extend(os_args(&[
        "-frames:v",
        "1",
        "-an",
        "-sn",
        "-vf",
        &scale,
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "pipe:1",
    ]));
    args
}

fn decode_rgb(bytes: Vec<u8>, (width, height): (u32, u32)) -> Result<RgbImage, String> {
    if bytes.is_empty() {
        return Err("no frame was decoded".to_owned());
    }
    let expected = usize::try_from(u64::from(width) * u64::from(height))
        .ok()
        .and_then(|pixels| pixels.checked_mul(BYTES_PER_RGB_PIXEL));
    if expected != Some(bytes.len()) {
        return Err(format!(
            "expected a {width}x{height} RGB frame but got {} bytes",
            bytes.len()
        ));
    }
    RgbImage::from_raw(width, height, bytes).ok_or_else(|| "frame buffer size mismatch".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_keeps_aspect_ratio() {
        assert_eq!(fit_within((1920, 1080), (640, 640)), (640, 360));
        assert_eq!(fit_within((1080, 1920), (640, 640)), (360, 640));
    }

    #[test]
    fn fit_within_never_upscales() {
        assert_eq!(fit_within((320, 240), (1920, 1080)), (320, 240));
    }

    #[test]
    fn fit_within_produces_even_sides_of_at_least_two() {
        assert_eq!(fit_within((1001, 999), (1001, 999)), (1000, 998));
        assert_eq!(fit_within((4000, 10), (100, 100)), (100, 2));
        assert_eq!(fit_within((0, 0), (0, 0)), (2, 2));
    }

    #[test]
    fn frame_args_seek_scale_and_request_raw_rgb() {
        let args = frame_args(Path::new("in.mp4"), Timestamp::from_millis(1_500), (64, 36));
        let text: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let position = |flag: &str| text.iter().position(|arg| arg == flag).unwrap();
        assert_eq!(text[position("-ss") + 1], "1.500000");
        assert!(position("-ss") < position("-i"), "-ss must come before -i");
        assert_eq!(text[position("-i") + 1], "in.mp4");
        assert_eq!(text[position("-vf") + 1], "scale=64:36");
        assert_eq!(text[position("-pix_fmt") + 1], "rgb24");
        assert_eq!(text.last().unwrap(), "pipe:1");
    }

    #[test]
    fn decode_rgb_accepts_exactly_one_frame() {
        let image = decode_rgb(vec![7; 4 * 2 * 3], (4, 2)).unwrap();
        assert_eq!(image.dimensions(), (4, 2));
        assert_eq!(image.get_pixel(3, 1).0, [7, 7, 7]);
    }

    #[test]
    fn decode_rgb_rejects_empty_and_wrongly_sized_output() {
        assert_eq!(
            decode_rgb(Vec::new(), (4, 2)),
            Err("no frame was decoded".to_owned())
        );
        assert_eq!(
            decode_rgb(vec![0; 10], (4, 2)),
            Err("expected a 4x2 RGB frame but got 10 bytes".to_owned())
        );
    }
}
