//! The floor tiles, generated rather than shipped as an image.
//!
//! One texture holds a 4 x 4 block of 60 cm ceramic tiles with the grout between
//! them, so it repeats every 2.4 m. Each tile gets a slightly different tone and
//! a soft cloudy grain, which is what stops a large floor looking like a
//! spreadsheet. Grout is split across the texture's edges, so the block tiles
//! seamlessly.
//!
//! Nothing here touches the GPU, so it is unit tested on the host; the renderer
//! uploads the mip chain [`tile_texture`] returns.

use scene_assets::environment::mip_chain;
use scene_assets::model::TextureData;

/// Tiles along each side of the texture.
pub const TILES_PER_SIDE: u32 = 4;
/// Real size of one tile, in metres.
pub const TILE_SIZE_M: f32 = 0.6;
/// The distance the texture repeats over, in metres.
pub const TEXTURE_PERIOD_M: f32 = TILES_PER_SIDE as f32 * TILE_SIZE_M;

/// A light, warm-grey porcelain, in sRGB.
const TILE_COLOR: [f32; 3] = [0.87, 0.855, 0.83];
/// Cement-grey grout, in sRGB.
const GROUT_COLOR: [f32; 3] = [0.6, 0.585, 0.56];
/// Grout width as a fraction of a tile: 0.9 cm on a 60 cm tile.
const GROUT_FRACTION: f32 = 0.015;
/// How far the tone of a whole tile may drift from `TILE_COLOR`.
const TILE_TONE_VARIATION: f32 = 0.035;
/// Strength of the cloudy grain across a tile, and of the per-pixel speckle.
const GRAIN_STRENGTH: f32 = 0.03;
const SPECKLE_STRENGTH: f32 = 0.012;
/// Size of a grain cell, in pixels.
const GRAIN_CELL: u32 = 24;

/// The tile texture at `size` x `size` pixels, with its full mip chain - level
/// 0 first - ready for upload as sRGB.
pub fn tile_texture(size: u32) -> Vec<TextureData> {
    mip_chain(tile_image(size.max(TILES_PER_SIDE * 8)))
}

/// Level 0 of [`tile_texture`].
fn tile_image(size: u32) -> TextureData {
    let tile_pixels = size as f32 / TILES_PER_SIDE as f32;
    let half_grout = (tile_pixels * GROUT_FRACTION * 0.5).max(1.0);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);

    for y in 0..size {
        for x in 0..size {
            let tile_x = (x as f32 / tile_pixels) as u32;
            let tile_y = (y as f32 / tile_pixels) as u32;

            // Distance to the nearest tile edge, measured from pixel centres.
            let local_x = x as f32 + 0.5 - tile_x as f32 * tile_pixels;
            let local_y = y as f32 + 0.5 - tile_y as f32 * tile_pixels;
            let edge = local_x
                .min(tile_pixels - local_x)
                .min(local_y)
                .min(tile_pixels - local_y);

            let color = if edge <= half_grout {
                GROUT_COLOR
                    .map(|channel| channel * (1.0 + speckle(x, y, 7) * 2.0 * SPECKLE_STRENGTH))
            } else {
                let tone = 1.0 + hash_signed(tile_x, tile_y, 1) * TILE_TONE_VARIATION;
                let grain = value_noise(x, y, size, tile_x * 31 + tile_y) * GRAIN_STRENGTH;
                let speckle = speckle(x, y, 3) * SPECKLE_STRENGTH;
                // A slightly rounded tile edge falls into shadow before the grout.
                let bevel = 1.0 - 0.1 * (1.0 - ((edge - half_grout) / 2.0).clamp(0.0, 1.0));
                TILE_COLOR.map(|channel| channel * (tone + grain + speckle) * bevel)
            };

            rgba.extend(color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8));
            rgba.push(255);
        }
    }

    TextureData {
        width: size,
        height: size,
        rgba,
    }
}

/// A cheap integer hash mapped to -1..1.
fn hash_signed(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Per-pixel noise, -1..1.
fn speckle(x: u32, y: u32, seed: u32) -> f32 {
    hash_signed(x, y, seed)
}

/// Smooth value noise on a `GRAIN_CELL` grid that wraps at `size`, so the
/// grain is seamless across the texture edge too. -1..1.
fn value_noise(x: u32, y: u32, size: u32, seed: u32) -> f32 {
    let cells = (size / GRAIN_CELL).max(1);
    let fx = x as f32 / GRAIN_CELL as f32;
    let fy = y as f32 / GRAIN_CELL as f32;
    let (cx, cy) = (fx.floor() as u32, fy.floor() as u32);
    let (tx, ty) = (smooth(fx.fract()), smooth(fy.fract()));

    let corner = |dx: u32, dy: u32| hash_signed((cx + dx) % cells, (cy + dy) % cells, seed);
    let top = lerp(corner(0, 0), corner(1, 0), tx);
    let bottom = lerp(corner(0, 1), corner(1, 1), tx);
    lerp(top, bottom, ty)
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(texture: &TextureData, x: u32, y: u32) -> [u8; 3] {
        let offset = ((y * texture.width + x) * 4) as usize;
        [
            texture.rgba[offset],
            texture.rgba[offset + 1],
            texture.rgba[offset + 2],
        ]
    }

    fn brightness(color: [u8; 3]) -> u32 {
        color.iter().map(|&channel| u32::from(channel)).sum()
    }

    #[test]
    fn builds_a_full_mip_chain_down_to_one_pixel() {
        let levels = tile_texture(256);

        assert_eq!((levels[0].width, levels[0].height), (256, 256));
        assert_eq!(levels[0].rgba.len(), 256 * 256 * 4);
        let last = levels.last().expect("a chain always has a last level");
        assert_eq!((last.width, last.height), (1, 1));
    }

    #[test]
    fn grout_is_darker_than_the_tiles_it_separates() {
        let image = tile_image(512);
        let tile = 512 / TILES_PER_SIDE;

        let grout = pixel(&image, tile, tile / 2);
        let middle = pixel(&image, tile / 2, tile / 2);
        assert!(
            brightness(grout) + 60 < brightness(middle),
            "{grout:?} vs {middle:?}"
        );
    }

    #[test]
    fn grout_runs_along_every_edge_so_the_texture_repeats_seamlessly() {
        let image = tile_image(512);
        let middle = brightness(pixel(&image, 64, 64));

        for (x, y) in [(0, 64), (511, 64), (64, 0), (64, 511)] {
            assert!(
                brightness(pixel(&image, x, y)) + 60 < middle,
                "no grout at ({x}, {y})"
            );
        }
    }

    #[test]
    fn tiles_are_not_all_the_same_tone() {
        let image = tile_image(512);
        let tile = 512 / TILES_PER_SIDE;

        let averages: Vec<u32> = (0..TILES_PER_SIDE * TILES_PER_SIDE)
            .map(|index| {
                let (tx, ty) = (index % TILES_PER_SIDE, index / TILES_PER_SIDE);
                let mut total = 0;
                for y in (ty * tile + 20..(ty + 1) * tile - 20).step_by(7) {
                    for x in (tx * tile + 20..(tx + 1) * tile - 20).step_by(7) {
                        total += brightness(pixel(&image, x, y));
                    }
                }
                total
            })
            .collect();

        let min = averages.iter().min().expect("sixteen tiles");
        let max = averages.iter().max().expect("sixteen tiles");
        assert!(max > min, "every tile came out identical");
    }

    #[test]
    fn the_texture_is_generated_the_same_way_every_time() {
        assert_eq!(tile_image(64).rgba, tile_image(64).rgba);
    }
}
