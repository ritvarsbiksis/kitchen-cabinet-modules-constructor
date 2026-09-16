//! The skybox the model sits in, decoded from two PNGs.
//!
//! Two images rather than one cube map, because they play different parts:
//!
//! - the **background** is a blurred equirectangular panorama of the room. It is
//!   what the camera sees behind the model, and what fills in the broad, soft
//!   half of every reflection.
//! - the **foreground** is the room photograph itself, hung in front of the model
//!   like a studio light card. It covers a small part of the sphere but carries
//!   all the structure - window, hood, worktop - so it is what puts readable
//!   highlights on polished metal instead of a flat grey sheen.
//!
//! Each image is decoded into a mip chain here rather than on the GPU: WebGL2 has
//! no compute and generating mips with render passes would mean a second pipeline,
//! while the environment is uploaded exactly once. Sampling a high mip level is
//! what stands in for the blur a rough surface applies to its reflection.
//!
//! Nothing in this module touches the GPU, so it builds for the host target and
//! is covered by the unit tests at the bottom of the file.

use crate::model::TextureData;

/// Why a skybox image could not be decoded.
#[derive(Debug)]
pub enum EnvironmentError {
    Image(image::ImageError),
    /// A zero-sized image, which has no mip chain to speak of.
    Empty,
}

impl core::fmt::Display for EnvironmentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Image(error) => write!(f, "could not decode a skybox image: {error}"),
            Self::Empty => write!(f, "a skybox image has no pixels"),
        }
    }
}

impl From<image::ImageError> for EnvironmentError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error)
    }
}

/// One environment image with its mip chain: level 0 first, every level half the
/// size of the one before it, down to 1x1.
#[derive(Clone, Debug)]
pub struct EnvironmentMap {
    pub levels: Vec<TextureData>,
}

impl EnvironmentMap {
    /// Decode a PNG and build the mip chain for it.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, EnvironmentError> {
        let decoded =
            image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8();
        let base = TextureData {
            width: decoded.width(),
            height: decoded.height(),
            rgba: decoded.into_raw(),
        };

        if base.width == 0 || base.height == 0 {
            return Err(EnvironmentError::Empty);
        }

        Ok(Self {
            levels: mip_chain(base),
        })
    }

    /// Size of level 0, in pixels.
    pub fn size(&self) -> (u32, u32) {
        let base = &self.levels[0];
        (base.width, base.height)
    }

    /// Index of the smallest level, which is what the shader clamps its
    /// roughness-driven level-of-detail to.
    pub fn max_lod(&self) -> f32 {
        (self.levels.len() - 1) as f32
    }

    /// Every level end to end, in the order `TextureDataOrder::MipMajor` expects.
    pub fn packed(&self) -> Vec<u8> {
        self.levels
            .iter()
            .flat_map(|level| level.rgba.iter().copied())
            .collect()
    }
}

/// Both halves of the skybox.
#[derive(Clone, Debug)]
pub struct Environment {
    pub background: EnvironmentMap,
    pub foreground: EnvironmentMap,
}

impl Environment {
    /// Decode the pair the web app fetched from `public/env`.
    pub fn decode_png(background: &[u8], foreground: &[u8]) -> Result<Self, EnvironmentError> {
        Ok(Self {
            background: EnvironmentMap::decode_png(background)?,
            foreground: EnvironmentMap::decode_png(foreground)?,
        })
    }
}

/// Halve `base` repeatedly until a single pixel is left.
fn mip_chain(base: TextureData) -> Vec<TextureData> {
    let mut levels = vec![base];

    while {
        let last = levels.last().expect("the chain starts with level 0");
        last.width > 1 || last.height > 1
    } {
        let smaller = halve(levels.last().expect("the chain starts with level 0"));
        levels.push(smaller);
    }

    levels
}

/// One mip level: a 2x2 box filter, averaged in linear light.
///
/// Averaging the stored sRGB bytes directly would darken every level, and these
/// levels are what the shader treats as the incoming light for rough surfaces -
/// so the error would show up as reflections that dim as roughness goes up.
fn halve(source: &TextureData) -> TextureData {
    let width = (source.width / 2).max(1);
    let height = (source.height / 2).max(1);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            // Clamped, so an odd-sized level samples its last row or column twice
            // rather than running off the end.
            let x0 = (x * 2).min(source.width - 1);
            let x1 = (x * 2 + 1).min(source.width - 1);
            let y0 = (y * 2).min(source.height - 1);
            let y1 = (y * 2 + 1).min(source.height - 1);

            let mut light = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for (sx, sy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                let offset = ((sy * source.width + sx) * 4) as usize;
                let texel = &source.rgba[offset..offset + 4];
                for (total, channel) in light.iter_mut().zip(texel) {
                    *total += srgb_to_linear(*channel);
                }
                alpha += f32::from(texel[3]);
            }

            rgba.extend(light.iter().map(|total| linear_to_srgb(total / 4.0)));
            rgba.push((alpha / 4.0).round() as u8);
        }
    }

    TextureData {
        width,
        height,
        rgba,
    }
}

/// The sRGB electro-optical transfer function, on a 0-255 byte.
fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Its inverse, back to a 0-255 byte.
fn linear_to_srgb(value: f32) -> u8 {
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };

    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A solid image of one colour, as raw RGBA.
    fn solid(width: u32, height: u32, color: [u8; 4]) -> TextureData {
        TextureData {
            width,
            height,
            rgba: color
                .iter()
                .copied()
                .cycle()
                .take((width * height * 4) as usize)
                .collect(),
        }
    }

    /// PNG-encode an image so the decoding path can be exercised.
    fn encode_png(texture: &TextureData) -> Vec<u8> {
        let buffer: image::RgbaImage =
            image::ImageBuffer::from_raw(texture.width, texture.height, texture.rgba.clone())
                .expect("the buffer matches the dimensions");
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(
                &mut std::io::Cursor::new(&mut encoded),
                image::ImageFormat::Png,
            )
            .expect("encoding a PNG in memory cannot fail");
        encoded
    }

    #[test]
    fn the_chain_halves_down_to_a_single_pixel() {
        let chain = mip_chain(solid(8, 4, [10, 20, 30, 255]));

        let sizes: Vec<(u32, u32)> = chain
            .iter()
            .map(|level| (level.width, level.height))
            .collect();
        assert_eq!(sizes, [(8, 4), (4, 2), (2, 1), (1, 1)]);
    }

    #[test]
    fn a_non_square_chain_ends_at_one_pixel_too() {
        let chain = mip_chain(solid(1, 6, [0, 0, 0, 255]));

        let last = chain.last().expect("a chain always has a last level");
        assert_eq!((last.width, last.height), (1, 1));
    }

    #[test]
    fn a_flat_colour_survives_every_level_unchanged() {
        let chain = mip_chain(solid(4, 4, [200, 120, 60, 255]));

        for level in &chain {
            // Round-tripping through linear light is allowed to move a byte by
            // one, but not to drift as the chain goes down.
            assert!((i16::from(level.rgba[0]) - 200).abs() <= 1, "red drifted");
            assert!((i16::from(level.rgba[1]) - 120).abs() <= 1, "green drifted");
            assert!((i16::from(level.rgba[2]) - 60).abs() <= 1, "blue drifted");
        }
    }

    #[test]
    fn black_and_white_average_in_linear_light_not_in_srgb() {
        let mut checker = solid(2, 2, [255, 255, 255, 255]);
        // Two of the four texels black, two white.
        for index in 0..8 {
            checker.rgba[index] = 0;
        }

        let level = halve(&checker);

        // Half the light back is 188 in sRGB; averaging the bytes would give 128.
        assert_eq!((level.width, level.height), (1, 1));
        assert!(
            (i16::from(level.rgba[0]) - 188).abs() <= 1,
            "expected ~188, got {}",
            level.rgba[0]
        );
    }

    #[test]
    fn decoding_a_png_reports_the_size_and_the_deepest_level() {
        let map = EnvironmentMap::decode_png(&encode_png(&solid(16, 8, [90, 90, 90, 255])))
            .expect("a valid PNG decodes");

        assert_eq!(map.size(), (16, 8));
        // 16 -> 8 -> 4 -> 2 -> 1, so five levels and a deepest index of 4.
        assert_eq!(map.max_lod(), 4.0);
        assert_eq!(
            map.packed().len(),
            map.levels.iter().map(|l| l.rgba.len()).sum::<usize>()
        );
    }

    #[test]
    fn bytes_that_are_not_a_png_are_rejected() {
        assert!(matches!(
            EnvironmentMap::decode_png(b"not a png at all"),
            Err(EnvironmentError::Image(_))
        ));
    }

    #[test]
    fn both_halves_decode_together() {
        let background = encode_png(&solid(4, 2, [30, 40, 50, 255]));
        let foreground = encode_png(&solid(2, 2, [60, 70, 80, 255]));

        let environment =
            Environment::decode_png(&background, &foreground).expect("both PNGs are valid");

        assert_eq!(environment.background.size(), (4, 2));
        assert_eq!(environment.foreground.size(), (2, 2));
    }
}
