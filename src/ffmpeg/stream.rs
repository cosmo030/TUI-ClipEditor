//! Decoding a run of frames with one long-running ffmpeg process.
//!
//! Seeking costs hundreds of milliseconds per frame on large videos, because
//! ffmpeg decodes from the previous keyframe each time. For playback, one
//! process decodes forward from the start position instead and writes raw
//! RGB frames back to back.

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::thread::JoinHandle;

use image::RgbImage;

use super::process::{failure_details, join_reader, kill_and_reap, read_in_background, spawn};
use super::{Ffmpeg, FfmpegError, os_args, program_name};
use crate::media::FrameRate;
use crate::timecode::Timestamp;

const BYTES_PER_RGB_PIXEL: usize = 3;

/// A running ffmpeg process that yields consecutive frames.
///
/// Dropping it stops ffmpeg.
#[derive(Debug)]
pub struct FrameStream {
    program: PathBuf,
    child: Child,
    stdout: ChildStdout,
    stderr: Option<JoinHandle<io::Result<Vec<u8>>>>,
    size: (u32, u32),
    frame_bytes: usize,
    start: Timestamp,
    rate: FrameRate,
    next_index: u64,
    finished: bool,
}

impl Ffmpeg {
    /// Starts decoding `path` from `start` at `rate` frames per second, each
    /// frame scaled to exactly `size` pixels.
    ///
    /// # Errors
    ///
    /// Returns [`FfmpegError::NotFound`] or [`FfmpegError::Spawn`] if ffmpeg
    /// cannot be started.
    pub fn stream_frames(
        &self,
        path: &Path,
        start: Timestamp,
        size: (u32, u32),
        rate: FrameRate,
    ) -> Result<FrameStream, FfmpegError> {
        let frame_bytes = usize::try_from(u64::from(size.0) * u64::from(size.1))
            .ok()
            .and_then(|pixels| pixels.checked_mul(BYTES_PER_RGB_PIXEL))
            .ok_or_else(|| FfmpegError::InvalidOutput {
                program: program_name(&self.ffmpeg),
                reason: format!("frame size {}x{} is too large", size.0, size.1),
            })?;
        let mut child = spawn(
            &self.ffmpeg,
            &stream_args(path, start, size, rate),
            Stdio::piped(),
        )?;
        let stderr = Some(read_in_background(child.stderr.take()));
        let stdout = child.stdout.take().ok_or_else(|| FfmpegError::Io {
            action: format!("reading output of {}", program_name(&self.ffmpeg)),
            source: io::Error::other("stdout was not captured"),
        })?;
        Ok(FrameStream {
            program: self.ffmpeg.clone(),
            child,
            stdout,
            stderr,
            size,
            frame_bytes,
            start,
            rate,
            next_index: 0,
            finished: false,
        })
    }
}

impl FrameStream {
    /// Blocks until the next frame is decoded. Returns the frame and its
    /// position in the video, or `None` once the video has ended.
    ///
    /// # Errors
    ///
    /// Returns [`FfmpegError::Failed`] if ffmpeg exits with an error, or
    /// [`FfmpegError::InvalidOutput`] if it stops in the middle of a frame.
    pub fn next_frame(&mut self) -> Result<Option<(Timestamp, RgbImage)>, FfmpegError> {
        if self.finished {
            return Ok(None);
        }
        let mut buffer = vec![0; self.frame_bytes];
        let filled = fill(&mut self.stdout, &mut buffer).map_err(|source| FfmpegError::Io {
            action: format!("reading frames from {}", program_name(&self.program)),
            source,
        })?;
        if filled == 0 {
            self.finished = true;
            return self.finish().map(|()| None);
        }
        if filled < self.frame_bytes {
            self.finished = true;
            return Err(FfmpegError::InvalidOutput {
                program: program_name(&self.program),
                reason: format!(
                    "stream ended inside a frame ({filled} of {} bytes)",
                    self.frame_bytes
                ),
            });
        }
        let at = self
            .start
            .saturating_add(self.rate.frame_offset(self.next_index));
        self.next_index += 1;
        let image = RgbImage::from_raw(self.size.0, self.size.1, buffer).ok_or_else(|| {
            FfmpegError::InvalidOutput {
                program: program_name(&self.program),
                reason: "frame buffer size mismatch".to_owned(),
            }
        })?;
        Ok(Some((at, image)))
    }

    /// Waits for ffmpeg after its output ended and reports a failed exit.
    fn finish(&mut self) -> Result<(), FfmpegError> {
        let status = self.child.wait().map_err(|source| FfmpegError::Io {
            action: format!("waiting for {}", program_name(&self.program)),
            source,
        })?;
        let stderr = match self.stderr.take() {
            Some(reader) => join_reader(reader, &self.program)?,
            None => Vec::new(),
        };
        if status.success() {
            Ok(())
        } else {
            Err(FfmpegError::Failed {
                program: program_name(&self.program),
                details: failure_details(status, &stderr),
            })
        }
    }
}

impl Drop for FrameStream {
    fn drop(&mut self) {
        if !self.finished {
            // Nothing can be reported from `drop`; if stopping fails, ffmpeg
            // still exits on its own when it next writes to the closed pipe.
            drop(kill_and_reap(&mut self.child, &self.program));
        }
    }
}

/// Reads until `buffer` is full or the input ends; returns the bytes read.
fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn stream_args(
    path: &Path,
    start: Timestamp,
    (width, height): (u32, u32),
    rate: FrameRate,
) -> Vec<OsString> {
    let position = start.to_ffmpeg_seconds();
    // Playback favours speed: the fast bilinear scaler, and the GPU decoder
    // where one is available. Software decoding of large, high-bitrate video
    // keeps many CPU cores busy, which slows encoding frames for the terminal.
    // `-hwaccel auto` falls back to software decoding by itself.
    let filters = format!(
        "fps={},scale={width}:{height}:flags=fast_bilinear",
        rate.to_ffmpeg_rate()
    );
    let mut args = os_args(&[
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-hwaccel",
        "auto",
        "-ss",
        &position,
        "-i",
    ]);
    args.push(path.as_os_str().to_owned());
    args.extend(os_args(&[
        "-an", "-sn", "-vf", &filters, "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
    ]));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_args_seek_resample_and_scale() {
        let rate = FrameRate::new(30, 1).unwrap();
        let args: Vec<String> = stream_args(
            Path::new("in.mp4"),
            Timestamp::from_millis(2_500),
            (640, 268),
            rate,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        let value_after = |flag: &str| {
            let index = args.iter().position(|arg| arg == flag).unwrap();
            args[index + 1].clone()
        };
        assert_eq!(value_after("-ss"), "2.500000");
        assert_eq!(value_after("-i"), "in.mp4");
        assert_eq!(
            value_after("-vf"),
            "fps=30/1,scale=640:268:flags=fast_bilinear"
        );
        assert_eq!(value_after("-hwaccel"), "auto");
        let position = |flag: &str| args.iter().position(|arg| arg == flag).unwrap();
        assert!(
            position("-hwaccel") < position("-i"),
            "-hwaccel is an input option"
        );
        assert_eq!(value_after("-pix_fmt"), "rgb24");
        assert!(
            !args.contains(&"-frames:v".to_owned()),
            "streams until the end"
        );
    }

    #[test]
    fn fill_reads_across_short_reads_and_stops_at_the_end() {
        let mut input: &[u8] = &[1, 2, 3, 4, 5];
        let mut buffer = [0; 4];
        assert_eq!(fill(&mut input, &mut buffer).unwrap(), 4);
        assert_eq!(buffer, [1, 2, 3, 4]);
        let mut rest = [0; 4];
        assert_eq!(fill(&mut input, &mut rest).unwrap(), 1);
        assert_eq!(fill(&mut input, &mut rest).unwrap(), 0);
    }
}
