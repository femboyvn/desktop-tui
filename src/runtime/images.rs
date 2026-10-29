use anyhow::{Context, bail};
use appcui::graphics::{Color, Image, Pixel, RenderOptionsBuilder, Surface};
use appcui::graphics::image::{CharacterSet, ColorSchema};
use image::{ImageDecoder, ImageReader, Limits, RgbaImage};
use std::io::{Cursor, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub(crate) const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;

pub(crate) fn validate_dimensions(width: u32, height: u32) -> anyhow::Result<()> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        bail!("Image exceeds the 16 megapixel limit or has invalid dimensions ({width} × {height}).");
    }
    Ok(())
}

/// WebP's image::ImageDecoder implementation does not enforce max_alloc.
/// Validate chunk storage and embedded frame dimensions before it can allocate
/// metadata or lossy frame buffers from untrusted length/header fields.
fn validate_webp(bytes: &[u8]) -> anyhow::Result<()> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        bail!("Invalid WebP container.");
    }
    let declared = u32::from_le_bytes(bytes[4..8].try_into()?) as u64 + 8;
    if declared != bytes.len() as u64 { bail!("WebP container length does not match the file."); }
    let mut canvas = None;
    validate_webp_chunks(&bytes[12..], &mut canvas, false)
}
fn webp_u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}
fn validate_webp_chunks(mut chunks: &[u8], canvas: &mut Option<(u32, u32)>, nested: bool) -> anyhow::Result<()> {
    while !chunks.is_empty() {
        if chunks.len() < 8 { bail!("Truncated WebP chunk header."); }
        let kind = &chunks[..4];
        let length = u32::from_le_bytes(chunks[4..8].try_into()?) as u64;
        let stored = 8 + length + (length & 1);
        if stored > chunks.len() as u64 { bail!("WebP chunk extends beyond its bounded input."); }
        let data = &chunks[8..8 + length as usize];
        let dimensions = match kind {
            b"VP8X" if !nested && data.len() == 10 => {
                let dimensions = (webp_u24(&data[4..7]) + 1, webp_u24(&data[7..10]) + 1);
                validate_dimensions(dimensions.0, dimensions.1)?;
                *canvas = Some(dimensions);
                None
            }
            b"VP8 " => {
                if data.len() < 10 || &data[3..6] != b"\x9d\x01\x2a" { bail!("Invalid WebP VP8 frame header."); }
                Some((u32::from(u16::from_le_bytes(data[6..8].try_into()?) & 0x3fff), u32::from(u16::from_le_bytes(data[8..10].try_into()?) & 0x3fff)))
            }
            b"VP8L" => {
                if data.len() < 5 || data[0] != 0x2f { bail!("Invalid WebP lossless frame header."); }
                let bits = u32::from_le_bytes(data[1..5].try_into()?);
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            b"ANMF" if !nested => {
                if data.len() < 16 { bail!("Truncated WebP animation frame."); }
                let frame = (webp_u24(&data[6..9]) + 1, webp_u24(&data[9..12]) + 1);
                validate_dimensions(frame.0, frame.1)?;
                let Some(bounds) = canvas else { bail!("WebP animation has no canvas."); };
                if webp_u24(&data[..3]) * 2 + frame.0 > bounds.0 || webp_u24(&data[3..6]) * 2 + frame.1 > bounds.1 {
                    bail!("WebP animation frame exceeds its canvas.");
                }
                validate_webp_chunks(&data[16..], &mut Some(frame), true)?;
                None
            }
            b"VP8X" | b"ANMF" => bail!("Invalid nested WebP header."),
            _ => None,
        };
        if let Some(dimensions) = dimensions {
            validate_dimensions(dimensions.0, dimensions.1)?;
            if canvas.is_some_and(|expected| expected != dimensions) { bail!("WebP frame dimensions do not match its canvas."); }
        }
        chunks = &chunks[stored as usize..];
    }
    Ok(())
}

pub(crate) fn decode(path: &Path) -> anyhow::Result<RgbaImage> {
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() { bail!("Choose a regular image file."); }
    if metadata.len() > MAX_INPUT_BYTES { bail!("Image input exceeds the 32 MiB limit."); }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES { bail!("Image input exceeds the 32 MiB limit."); }
    if bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") { validate_webp(&bytes)?; }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(reader.format(), Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP | image::ImageFormat::Gif | image::ImageFormat::Bmp)) {
        bail!("Unsupported image. Choose PNG, JPEG, WebP, GIF, or BMP.");
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PIXELS as u32);
    limits.max_image_height = Some(MAX_PIXELS as u32);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().context("Image header is corrupt or exceeds decoder limits")?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    let orientation = decoder.orientation().context("Cannot read image orientation")?;
    let mut image = image::DynamicImage::from_decoder(decoder).context("Cannot decode image; the file may be corrupt")?;
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Zoom { Fit, Scale(f64), FitScale(f64) }
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderRequest {
    pub size: (u32, u32),
    pub mode: Zoom,
    /// Offset of the viewport center from the image center, in source pixels.
    pub pan: (f64, f64),
    pub background: [u8; 3],
}
pub(crate) struct Geometry { pub scale: f64, pub pan: (f64, f64) }
pub(crate) struct Frame {
    pub surface: Surface,
    pub dimensions: (u32, u32),
    pub geometry: Geometry,
}
pub(crate) fn geometry(dimensions: (u32, u32), request: RenderRequest) -> Geometry {
    let (width, height) = (f64::from(dimensions.0.max(1)), f64::from(dimensions.1.max(1)));
    let (view_width, view_height) = (f64::from(request.size.0.max(1)), f64::from(request.size.1.max(1)) * 2.0);
    let scale = match request.mode {
        Zoom::Fit => (view_width / width).min(view_height / height).min(4.0),
        Zoom::Scale(scale) => if scale.is_finite() { scale.clamp(0.05, 4.0) } else { 1.0 },
        Zoom::FitScale(factor) => (view_width / width).min(view_height / height).min(4.0)
            * if factor.is_finite() { factor.clamp(0.25, 16.0) } else { 1.0 },
    };
    let max_x = ((width - view_width / scale) / 2.0).max(0.0);
    let max_y = ((height - view_height / scale) / 2.0).max(0.0);
    Geometry { scale, pan: (request.pan.0.clamp(-max_x, max_x), request.pan.1.clamp(-max_y, max_y)) }
}

/// Prepares only the viewport, then delegates half-block rendering to AppCUI.
pub(crate) fn render(source: &RgbaImage, request: RenderRequest) -> anyhow::Result<Frame> {
    if request.size.0 == 0 || request.size.1 == 0 || request.size.0 > 2048 || request.size.1 > 1024 {
        bail!("Image viewport exceeds the supported terminal bounds (2048 × 1024 cells).");
    }
    let geometry = geometry(source.dimensions(), request);
    let (width, height) = request.size;
    let mut viewport = Image::new(width, height * 2).context("Cannot allocate image viewport")?;
    let background = request.background;
    let scaled_width = (f64::from(source.width()) * geometry.scale).round().max(1.0);
    let scaled_height = (f64::from(source.height()) * geometry.scale).round().max(1.0);
    let left = ((f64::from(width) - scaled_width) / 2.0).floor() - geometry.pan.0 * geometry.scale;
    let top = ((f64::from(height * 2) - scaled_height) / 2.0).floor() - geometry.pan.1 * geometry.scale;
    for y in 0..height * 2 {
        for x in 0..width {
            let px = f64::from(x) - left;
            let py = f64::from(y) - top;
            let rgb = if px >= 0.0 && py >= 0.0 && px < scaled_width && py < scaled_height {
                let sx = ((px + 0.5) * f64::from(source.width()) / scaled_width).min(f64::from(source.width() - 1));
                let sy = ((py + 0.5) * f64::from(source.height()) / scaled_height).min(f64::from(source.height() - 1));
                let pixel = source.get_pixel(sx as u32, sy as u32).0;
                let alpha = u32::from(pixel[3]);
                std::array::from_fn(|i| ((u32::from(pixel[i]) * alpha + u32::from(background[i]) * (255 - alpha) + 127) / 255) as u8)
            } else { background };
            viewport.set_pixel(x, y, Pixel::with_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
    let mut surface = Surface::new(width, height);
    surface.clear(appcui::graphics::Character::new(' ', Color::Black, Color::RGB(background[0], background[1], background[2]), appcui::graphics::CharFlags::None));
    surface.draw_image(0, 0, &viewport, &RenderOptionsBuilder::new().character_set(CharacterSet::SmallBlocks).color_schema(ColorSchema::TrueColors).build());
    Ok(Frame { surface, dimensions: source.dimensions(), geometry })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    #[test]
    fn webp_preflight_rejects_truncated_chunks_and_embedded_oversized_frames() {
        fn container(chunks: &[(&[u8;4], Vec<u8>)]) -> Vec<u8> {
            let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
            for (kind, data) in chunks {
                bytes.extend_from_slice(*kind);
                bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
                bytes.extend_from_slice(data);
                if data.len() % 2 == 1 { bytes.push(0); }
            }
            let size = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&size.to_le_bytes());
            bytes
        }
        let canvas = vec![0;10]; // VP8X canvas 1 by 1.
        let mut truncated = container(&[(b"VP8X", canvas.clone()), (b"EXIF", vec![])]);
        let end = truncated.len();
        truncated[end-4..].copy_from_slice(&(256u32 * 1024 * 1024).to_le_bytes());
        assert!(validate_webp(&truncated).is_err());
        let frame = vec![0,0,0,0x9d,1,0x2a,0xff,0x3f,0xff,0x3f];
        let oversized = container(&[(b"VP8X", canvas.clone()), (b"VP8 ", frame.clone())]);
        assert!(validate_webp(&oversized).is_err());
        let mismatch = vec![0,0,0,0x9d,1,0x2a,2,0,2,0];
        assert!(validate_webp(&container(&[(b"VP8X", canvas.clone()), (b"VP8 ", mismatch)])).is_err());
        let mut animated = vec![0;16]; // ANMF frame 1 by 1, then nested oversized VP8.
        animated.extend_from_slice(&container(&[(b"VP8 ", frame)])[12..]);
        assert!(validate_webp(&container(&[(b"VP8X", canvas), (b"ANMF", animated)])).is_err());
    }
    #[test]
    fn respects_all_exif_orientations() {
        let dir = TestDir::new();
        let path = dir.path().join("oriented.jpg");
        let source = image::RgbImage::from_fn(4, 6, |x, y| image::Rgb([(x * 50) as u8, (y * 40) as u8, 100]));
        source.save(&path).unwrap();
        let original = decode(&path).unwrap();
        let jpeg = std::fs::read(&path).unwrap();
        for (orientation, corner) in [(1, (0,0)), (2, (3,0)), (3, (3,5)), (4, (0,5)), (5, (0,0)), (6, (0,5)), (7, (3,5)), (8, (3,0))] {
            let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
            exif.extend_from_slice(&[orientation, 0, 0, 0, 0, 0, 0, 0]);
            let mut encoded = jpeg[..2].to_vec();
            encoded.extend_from_slice(&[0xff, 0xe1]);
            encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
            encoded.extend(exif);
            encoded.extend_from_slice(&jpeg[2..]);
            std::fs::write(&path, encoded).unwrap();
            let oriented = decode(&path).unwrap();
            assert_eq!(oriented.dimensions(), if orientation >= 5 { (6,4) } else { (4,6) });
            assert_eq!(oriented.get_pixel(0,0), original.get_pixel(corner.0,corner.1), "orientation {orientation}");
        }
    }
    #[test]
    fn animated_gif_displays_only_the_first_frame() {
        let dir = TestDir::new();
        let path = dir.path().join("animated.gif");
        let mut encoder = image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
        encoder.encode_frames([[255,0,0,255], [0,0,255,255]].into_iter().map(|rgba| image::Frame::new(image::RgbaImage::from_pixel(2,2,image::Rgba(rgba))))).unwrap();
        drop(encoder);
        assert_eq!(decode(&path).unwrap().get_pixel(0,0).0, [255,0,0,255]);
    }
    #[test]
    fn rejects_oversized_bmp_header_without_pixel_storage() {
        let dir = TestDir::new();
        let path = dir.path().join("oversized.bmp");
        let mut header = vec![0u8;54];
        header[..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&54u32.to_le_bytes());
        header[10..14].copy_from_slice(&54u32.to_le_bytes());
        header[14..18].copy_from_slice(&40u32.to_le_bytes());
        header[18..22].copy_from_slice(&4001i32.to_le_bytes());
        header[22..26].copy_from_slice(&4000i32.to_le_bytes());
        header[26..28].copy_from_slice(&1u16.to_le_bytes());
        header[28..30].copy_from_slice(&32u16.to_le_bytes());
        std::fs::write(&path, header).unwrap();
        assert!(format!("{:#}", decode(&path).unwrap_err()).contains("16 megapixel"));
    }
    #[test]
    fn decodes_each_enabled_format_and_rejects_invalid_inputs() {
        let dir = TestDir::new();
        for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg, image::ImageFormat::WebP, image::ImageFormat::Gif, image::ImageFormat::Bmp] {
            let path = dir.path().join(format!("test.{format:?}"));
            image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30])).save_with_format(&path, format).unwrap();
            let decoded = decode(&path).unwrap();
            assert_eq!(decoded.dimensions(), (3, 2));
        }
        let path = dir.path().join("broken.png");
        std::fs::write(&path, b"not an image").unwrap();
        assert!(decode(&path).is_err());
        assert!(decode(dir.path()).is_err());
        assert!(decode(&dir.path().join("missing")).is_err());
    }
    #[test]
    fn rejects_input_and_pixel_limits_before_full_decode() {
        let dir = TestDir::new();
        let path = dir.path().join("large.png");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_INPUT_BYTES + 1).unwrap();
        assert!(format!("{:#}", decode(&path).unwrap_err()).contains("32 MiB"));
        assert!(validate_dimensions(4000, 4000).is_ok());
        assert!(validate_dimensions(4001, 4000).is_err());
        assert!(validate_dimensions(u32::MAX, u32::MAX).is_err());
        assert!(validate_dimensions(0, 1).is_err());
    }
    #[test]
    fn fit_zoom_pan_and_transparency_use_half_cell_geometry() {
        let image = image::RgbaImage::from_pixel(100, 80, image::Rgba([200, 100, 0, 128]));
        let request = RenderRequest { size: (50, 20), mode: Zoom::Fit, pan: (0.0, 0.0), background: [20, 40, 60] };
        let fitted = geometry(image.dimensions(), request);
        assert_eq!(fitted.scale, 0.5);
        let frame = render(&image, request).unwrap();
        let cell = frame.surface.char(0, 0).unwrap();
        assert_eq!(cell.background, appcui::graphics::Color::RGB(110, 70, 30));
        let extreme = RenderRequest { mode: Zoom::Scale(20.0), pan: (1e9, -1e9), ..request };
        let g = geometry(image.dimensions(), extreme);
        assert_eq!(g.scale, 4.0);
        assert!(g.pan.0 < 100.0 && g.pan.1 > -80.0);
        assert_eq!(geometry((100, 80), RenderRequest { mode: Zoom::Scale(0.001), ..request }).scale, 0.05);
    }
}
