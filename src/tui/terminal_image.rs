//! Preview images in terminal form, and the widget that draws them.
//!
//! `ratatui-image` prefixes every sixel image with commands that erase its
//! whole area. During playback that blanks the preview before each new frame
//! arrives, and terminals that paint in between show a flash at the frame
//! rate. A new frame of the same size fully covers the previous one, so this
//! module encodes sixel itself (with the same encoder `ratatui-image` uses)
//! and erases only when the image moves, changes size, or something else
//! may have been drawn there.

use std::fmt::Write as _;
use std::num::NonZeroU16;

use icy_sixel::{EncodeOptions, sixel_encode};
use image::{DynamicImage, RgbImage};
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Widget;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{FontSize, Image, Resize};

/// The whole escape sequence occupies one cell for ratatui's layout.
const ONE_CELL: CellDiffOption = CellDiffOption::ForcedWidth(NonZeroU16::MIN);

/// A preview frame encoded for the terminal.
pub(super) enum TerminalImage {
    /// Any format, encoded by `ratatui-image`.
    Library(Protocol),
    /// Sixel, encoded here so it can be drawn without erasing first.
    Sixel(SixelImage),
}

/// Sixel data and the cells it covers.
pub(super) struct SixelImage {
    data: String,
    size: Size,
}

impl TerminalImage {
    /// The cells the image covers.
    pub(super) fn size(&self) -> Size {
        match self {
            Self::Library(protocol) => protocol.size(),
            Self::Sixel(sixel) => sixel.size,
        }
    }

    /// Bytes sent to the terminal to draw it, where known (sixel only).
    pub(super) fn byte_len(&self) -> Option<usize> {
        match self {
            Self::Library(_) => None,
            Self::Sixel(sixel) => Some(sixel.data.len()),
        }
    }
}

/// How much detail to spend on an image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Quality {
    /// Full detail, for a still frame.
    #[default]
    Full,
    /// Lighter frames while playing. Grainy footage makes full-detail sixel
    /// frames around 330 KB (about 10 MB/s at 30 fps), more than Windows
    /// Terminal takes in; fewer colours without dithering roughly halves
    /// that. A paused frame is redrawn at full detail.
    Playback,
}

/// Colours in a playback frame (a full frame uses up to 256).
const PLAYBACK_COLORS: u16 = 64;

impl Quality {
    fn sixel_options(self) -> EncodeOptions {
        match self {
            Self::Full => EncodeOptions::default(),
            Self::Playback => EncodeOptions {
                max_colors: PLAYBACK_COLORS,
                diffusion: 0.0,
                ..EncodeOptions::default()
            },
        }
    }
}

/// Encodes `image` to fit in `cells`, in the picker's format. `quality`
/// applies to sixel; other formats have no comparable setting.
///
/// # Errors
///
/// Returns the encoder's error message.
pub(super) fn encode(
    picker: &Picker,
    image: RgbImage,
    cells: Size,
    quality: Quality,
) -> Result<TerminalImage, String> {
    // Inside tmux, sixel must be wrapped in passthrough sequences, which the
    // library handles.
    if picker.protocol_type() == ProtocolType::Sixel && !picker.tmux_detected() {
        let size = cells_covered(image.width(), image.height(), picker.font_size());
        if size.width <= cells.width && size.height <= cells.height {
            return encode_sixel(&image, size, &quality.sixel_options()).map(TerminalImage::Sixel);
        }
    }
    picker
        .new_protocol(DynamicImage::ImageRgb8(image), cells, Resize::Fit(None))
        .map(TerminalImage::Library)
        .map_err(|error| error.to_string())
}

fn encode_sixel(
    image: &RgbImage,
    size: Size,
    options: &EncodeOptions,
) -> Result<SixelImage, String> {
    let rgba = DynamicImage::ImageRgb8(image.clone()).to_rgba8();
    let width = usize::try_from(image.width()).map_err(|error| error.to_string())?;
    let height = usize::try_from(image.height()).map_err(|error| error.to_string())?;
    let data = sixel_encode(rgba.as_raw(), width, height, options)
        .map_err(|error| format!("sixel encoding error: {error}"))?;
    Ok(SixelImage { data, size })
}

/// The cells an image of `width` × `height` pixels covers.
fn cells_covered(width: u32, height: u32, font: FontSize) -> Size {
    let cells = |pixels: u32, cell: u16| {
        u16::try_from(pixels.div_ceil(u32::from(cell.max(1)))).unwrap_or(u16::MAX)
    };
    Size::new(cells(width, font.width), cells(height, font.height))
}

/// Escape sequence that blanks `width` × `height` cells from the cursor and
/// returns the cursor to where it started (the same one `ratatui-image` sends).
fn erase_sequence(width: u16, height: u16) -> String {
    let mut sequence = String::new();
    for _ in 0..height {
        // Writing to a String cannot fail.
        let _ = write!(sequence, "\x1b[{width}X\x1b[1B");
    }
    let _ = write!(sequence, "\x1b[{height}A");
    sequence
}

/// Draws a [`TerminalImage`] at the top-left of its area.
pub(super) struct PreviewImage<'a> {
    pub(super) image: &'a TerminalImage,
    /// Blank the image's cells first. Needed when the cells may hold
    /// something else (text, another image size); not needed when the same
    /// area showed a sixel image of the same size last frame.
    pub(super) erase: bool,
}

impl Widget for PreviewImage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.image {
            TerminalImage::Library(protocol) => Image::new(protocol).render(area, buf),
            TerminalImage::Sixel(sixel) => render_sixel(sixel, self.erase, area, buf),
        }
    }
}

fn render_sixel(sixel: &SixelImage, erase: bool, area: Rect, buf: &mut Buffer) {
    if sixel.size.width > area.width || sixel.size.height > area.height {
        return;
    }
    let image_area = Rect::new(area.x, area.y, sixel.size.width, sixel.size.height);
    let symbol = if erase {
        erase_sequence(sixel.size.width, sixel.size.height) + &sixel.data
    } else {
        sixel.data.clone()
    };
    for position in image_area.positions() {
        let cell = &mut buf[position];
        if position == image_area.as_position() {
            cell.set_symbol(&symbol).set_diff_option(ONE_CELL);
        } else {
            // The terminal draws these cells from the escape sequence.
            cell.set_diff_option(CellDiffOption::Skip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sixel_picker() -> Picker {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Sixel);
        picker
    }

    fn rendered(image: &TerminalImage, erase: bool, area: Rect) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 20));
        PreviewImage { image, erase }.render(area, &mut buffer);
        buffer
    }

    #[test]
    fn sixel_images_are_encoded_without_an_erase_prefix() {
        let font = sixel_picker().font_size();
        let image = RgbImage::new(u32::from(font.width) * 4, u32::from(font.height) * 3);
        let encoded = encode(&sixel_picker(), image, Size::new(10, 10), Quality::Full).unwrap();
        let TerminalImage::Sixel(sixel) = &encoded else {
            panic!("expected our own sixel encoding");
        };
        assert!(
            sixel.data.starts_with("\x1bP"),
            "starts with the sixel introducer"
        );
        assert_eq!(encoded.size(), Size::new(4, 3));
    }

    #[test]
    fn images_too_big_for_the_cells_use_the_library_resize() {
        let image = RgbImage::new(1_000, 1_000);
        let encoded = encode(&sixel_picker(), image, Size::new(5, 5), Quality::Full).unwrap();
        assert!(matches!(encoded, TerminalImage::Library(_)));
        let size = encoded.size();
        assert!(size.width <= 5 && size.height <= 5);
    }

    #[test]
    fn other_formats_use_the_library() {
        let image = RgbImage::new(20, 20);
        let encoded = encode(
            &Picker::halfblocks(),
            image,
            Size::new(10, 10),
            Quality::Full,
        )
        .unwrap();
        assert!(matches!(encoded, TerminalImage::Library(_)));
    }

    #[test]
    fn sixel_is_drawn_without_erasing_when_not_needed() {
        let sixel = TerminalImage::Sixel(SixelImage {
            data: "\x1bPq#0~-\x1b\\".to_owned(),
            size: Size::new(3, 2),
        });
        let buffer = rendered(&sixel, false, Rect::new(5, 4, 10, 10));
        assert_eq!(buffer[(5, 4)].symbol(), "\x1bPq#0~-\x1b\\");
        assert_eq!(buffer[(5, 4)].diff_option, ONE_CELL);
        assert_eq!(buffer[(6, 4)].diff_option, CellDiffOption::Skip);
        assert_eq!(buffer[(7, 5)].diff_option, CellDiffOption::Skip);
        assert_eq!(
            buffer[(8, 4)].diff_option,
            CellDiffOption::None,
            "outside the image"
        );
    }

    #[test]
    fn sixel_is_erased_first_when_asked() {
        let sixel = TerminalImage::Sixel(SixelImage {
            data: "\x1bPq\x1b\\".to_owned(),
            size: Size::new(3, 2),
        });
        let buffer = rendered(&sixel, true, Rect::new(0, 0, 10, 10));
        assert_eq!(
            buffer[(0, 0)].symbol(),
            "\x1b[3X\x1b[1B\x1b[3X\x1b[1B\x1b[2A\x1bPq\x1b\\"
        );
    }

    #[test]
    fn sixel_larger_than_the_area_is_not_drawn() {
        let sixel = TerminalImage::Sixel(SixelImage {
            data: "\x1bPq\x1b\\".to_owned(),
            size: Size::new(30, 2),
        });
        let buffer = rendered(&sixel, false, Rect::new(0, 0, 10, 10));
        assert_eq!(buffer[(0, 0)].symbol(), " ");
    }

    /// A deterministic, noisy image (like grainy video), which is what makes
    /// full-detail sixel frames large.
    fn noisy_image(width: u32, height: u32) -> RgbImage {
        let mut state: u32 = 12_345;
        RgbImage::from_fn(width, height, |x, y| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let noise = (state >> 24) as u8 / 4;
            image::Rgb([
                (x * 255 / width) as u8 / 2 + noise,
                (y * 255 / height) as u8 / 2 + noise,
                128 + noise / 2,
            ])
        })
    }

    #[test]
    fn playback_quality_sends_much_less_sixel_data() {
        let font = sixel_picker().font_size();
        let (width, height) = (u32::from(font.width) * 30, u32::from(font.height) * 10);
        let cells = Size::new(30, 10);
        let full = encode(
            &sixel_picker(),
            noisy_image(width, height),
            cells,
            Quality::Full,
        )
        .unwrap();
        let light = encode(
            &sixel_picker(),
            noisy_image(width, height),
            cells,
            Quality::Playback,
        )
        .unwrap();
        let (full, light) = (full.byte_len().unwrap(), light.byte_len().unwrap());
        assert!(light * 10 < full * 8, "playback {light} B vs full {full} B");
    }

    #[test]
    fn only_sixel_reports_its_size_in_bytes() {
        let blocks = encode(
            &Picker::halfblocks(),
            RgbImage::new(20, 20),
            Size::new(10, 10),
            Quality::Full,
        )
        .unwrap();
        assert_eq!(blocks.byte_len(), None);
    }

    #[test]
    fn cells_covered_rounds_up() {
        let font = FontSize::new(10, 20);
        assert_eq!(cells_covered(100, 40, font), Size::new(10, 2));
        assert_eq!(cells_covered(101, 41, font), Size::new(11, 3));
    }
}
