//! Encoding preview frames for the terminal on a background thread.
//!
//! Turning a frame into sixel (or kitty/iTerm2) data takes tens of
//! milliseconds for large previews. Doing it on the UI thread would delay
//! key presses, so a worker encodes the newest frame and skips any that went
//! stale while it was busy.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use image::{DynamicImage, RgbImage};
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

/// Identifies a decoded frame: (session generation, frame serial).
pub(super) type FrameKey = (u64, u64);

/// Encode `image` to fit `cells`.
#[derive(Debug)]
pub(super) struct EncodeRequest {
    pub(super) key: FrameKey,
    pub(super) cells: Size,
    pub(super) image: RgbImage,
}

/// An encoded frame, ready to draw.
pub(super) struct Encoded {
    pub(super) key: FrameKey,
    pub(super) cells: Size,
    pub(super) protocol: Result<Protocol, String>,
}

impl std::fmt::Debug for Encoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encoded")
            .field("key", &self.key)
            .field("cells", &self.cells)
            .field("ok", &self.protocol.is_ok())
            .finish()
    }
}

/// Starts the encoder. Results go to `deliver`, which returns `false` once
/// the UI is gone; the worker then exits.
pub(super) fn spawn_encoder(
    picker: Picker,
    deliver: impl Fn(Encoded) -> bool + Send + 'static,
) -> Sender<EncodeRequest> {
    let (requests, inbox) = mpsc::channel::<EncodeRequest>();
    thread::spawn(move || {
        while let Ok(first) = inbox.recv() {
            // Frames queued while the last one was encoding are already old.
            let request = inbox.try_iter().last().unwrap_or(first);
            let protocol = picker
                .new_protocol(
                    DynamicImage::ImageRgb8(request.image),
                    request.cells,
                    Resize::Fit(None),
                )
                .map_err(|error| error.to_string());
            let encoded = Encoded {
                key: request.key,
                cells: request.cells,
                protocol,
            };
            if !deliver(encoded) {
                return;
            }
        }
    });
    requests
}

/// Counts how many preview frames were shown in the last second.
#[derive(Debug, Default)]
pub(super) struct FrameRateMeter {
    shown: VecDeque<Instant>,
}

impl FrameRateMeter {
    const WINDOW: Duration = Duration::from_secs(1);

    pub(super) fn record(&mut self, at: Instant) {
        self.shown.push_back(at);
        self.forget_before(at);
    }

    /// Frames shown during the second before `now`.
    pub(super) fn per_second(&mut self, now: Instant) -> usize {
        self.forget_before(now);
        self.shown.len()
    }

    fn forget_before(&mut self, now: Instant) {
        while self
            .shown
            .front()
            .is_some_and(|&shown| now.duration_since(shown) >= Self::WINDOW)
        {
            self.shown.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_counts_only_the_last_second() {
        let start = Instant::now();
        let mut meter = FrameRateMeter::default();
        for index in 0..10 {
            meter.record(start + Duration::from_millis(index * 100));
        }
        assert_eq!(meter.per_second(start + Duration::from_millis(950)), 10);
        assert_eq!(meter.per_second(start + Duration::from_millis(1_450)), 5);
        assert_eq!(meter.per_second(start + Duration::from_secs(5)), 0);
    }

    #[test]
    fn encoder_returns_a_protocol_that_fits_the_cells() {
        let picker = Picker::halfblocks();
        let (sender, results) = mpsc::channel();
        let encoder = spawn_encoder(picker, move |encoded| sender.send(encoded).is_ok());
        encoder
            .send(EncodeRequest {
                key: (1, 1),
                cells: Size::new(20, 10),
                image: RgbImage::new(100, 100),
            })
            .unwrap();
        let encoded = results.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(encoded.key, (1, 1));
        let size = encoded.protocol.unwrap().size();
        assert!(size.width <= 20 && size.height <= 10, "got {size:?}");
    }
}
