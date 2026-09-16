//! Where everything in the room goes, in metres.
//!
//! The world is Y-up with the floor at `y = 0`. The floor is a 6 x 6 m square
//! centred on the origin; the wall stands along its back edge, facing +Z, centred
//! on X. Modules are 80 cm wide and stand with their backs against the wall in a
//! run that is centred on it too. The three pendant lamps hang in a row across
//! the middle of the floor, parallel to the wall.
//!
//! Nothing here touches the GPU, so the whole arrangement is unit tested.

use core::ops::RangeInclusive;

use glam::Vec3;

use crate::picking::Aabb;

/// Wall widths the constructor accepts, in centimetres.
pub const WALL_WIDTH_CM: RangeInclusive<u32> = 200..=500;
/// Wall heights the constructor accepts, in centimetres.
pub const WALL_HEIGHT_CM: RangeInclusive<u32> = 250..=300;

/// Side of the square floor.
pub const FLOOR_SIZE_M: f32 = 6.0;
/// How far the floor slab extends below `y = 0`, so its edge reads as a slab
/// rather than a sheet of paper.
pub const FLOOR_THICKNESS_M: f32 = 0.04;
pub const WALL_THICKNESS_M: f32 = 0.20;

/// Width of one slot. Kept in whole centimetres so the slot count is exact
/// integer division - `4.0 / 0.8` in floating point is 4.999..., which would
/// quietly lose a module on a 4 m wall.
pub const MODULE_WIDTH_CM: u32 = 80;
pub const MODULE_WIDTH_M: f32 = MODULE_WIDTH_CM as f32 / 100.0;
pub const MODULE_HEIGHT_M: f32 = 0.87;
pub const MODULE_DEPTH_M: f32 = 0.58;
/// Left between a module's back and the wall, so the two faces never z-fight.
const WALL_CLEARANCE_M: f32 = 0.002;

/// Ceiling to the bottom rim of a lamp shade.
pub const LAMP_DROP_M: f32 = 0.7;
/// Lamps are spaced a third of the wall apart, within these limits.
const LAMP_SPACING_M: RangeInclusive<f32> = 0.8..=1.5;

/// Why a wall size was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    WallWidth(u32),
    WallHeight(u32),
}

impl core::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WallWidth(width) => write!(
                f,
                "the wall width must be between {} and {} cm, got {width} cm",
                WALL_WIDTH_CM.start(),
                WALL_WIDTH_CM.end()
            ),
            Self::WallHeight(height) => write!(
                f,
                "the wall height must be between {} and {} cm, got {height} cm",
                WALL_HEIGHT_CM.start(),
                WALL_HEIGHT_CM.end()
            ),
        }
    }
}

/// One place along the wall a module can stand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// X of the slot's centre line.
    pub center_x: f32,
    /// The space a module in this slot occupies; also what picking tests against.
    pub bounds: Aabb,
}

/// A pendant lamp: where its cord meets the ceiling and where its light is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    /// The top of the cord, at ceiling height.
    pub ceiling: Vec3,
    /// The centre of the shade's open bottom, which is where the light source
    /// sits.
    pub light: Vec3,
}

/// The room for one wall size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KitchenLayout {
    wall_width_cm: u32,
    wall_height_cm: u32,
}

impl KitchenLayout {
    /// A layout for a wall of the given size, or why that size is not allowed.
    pub fn new(wall_width_cm: u32, wall_height_cm: u32) -> Result<Self, LayoutError> {
        if !WALL_WIDTH_CM.contains(&wall_width_cm) {
            return Err(LayoutError::WallWidth(wall_width_cm));
        }
        if !WALL_HEIGHT_CM.contains(&wall_height_cm) {
            return Err(LayoutError::WallHeight(wall_height_cm));
        }

        Ok(Self {
            wall_width_cm,
            wall_height_cm,
        })
    }

    pub fn wall_width(&self) -> f32 {
        self.wall_width_cm as f32 / 100.0
    }

    pub fn wall_height(&self) -> f32 {
        self.wall_height_cm as f32 / 100.0
    }

    /// The floor slab: its top face is `y = 0`.
    pub fn floor_bounds(&self) -> Aabb {
        let half = FLOOR_SIZE_M * 0.5;
        Aabb::new(
            Vec3::new(-half, -FLOOR_THICKNESS_M, -half),
            Vec3::new(half, 0.0, half),
        )
    }

    /// Z of the wall's front face, the one the modules stand against.
    pub fn wall_front_z(&self) -> f32 {
        -FLOOR_SIZE_M * 0.5 + WALL_THICKNESS_M
    }

    /// The wall, standing on the floor with its back flush with the floor's
    /// back edge.
    pub fn wall_bounds(&self) -> Aabb {
        let half_width = self.wall_width() * 0.5;
        Aabb::new(
            Vec3::new(-half_width, 0.0, -FLOOR_SIZE_M * 0.5),
            Vec3::new(half_width, self.wall_height(), self.wall_front_z()),
        )
    }

    /// How many modules fit side by side without running past the wall.
    pub fn slot_count(&self) -> usize {
        (self.wall_width_cm / MODULE_WIDTH_CM) as usize
    }

    /// Every slot, left to right, as a run centred on the wall.
    pub fn slots(&self) -> Vec<Slot> {
        let count = self.slot_count();
        let run_start = -(count as f32) * MODULE_WIDTH_M * 0.5;
        let back = self.wall_front_z() + WALL_CLEARANCE_M;

        (0..count)
            .map(|index| {
                let left = run_start + index as f32 * MODULE_WIDTH_M;
                Slot {
                    center_x: left + MODULE_WIDTH_M * 0.5,
                    bounds: Aabb::new(
                        Vec3::new(left, 0.0, back),
                        Vec3::new(
                            left + MODULE_WIDTH_M,
                            MODULE_HEIGHT_M,
                            back + MODULE_DEPTH_M,
                        ),
                    ),
                }
            })
            .collect()
    }

    /// The translation that stands a model with the given authored bounds in
    /// `slot`: centred on the slot, on the floor, back against the wall.
    ///
    /// Going by the bounds rather than the model's origin means an asset
    /// exported a little off-centre still lines up with its neighbours.
    pub fn placement(&self, slot: &Slot, model_min: Vec3, model_max: Vec3) -> Vec3 {
        Vec3::new(
            slot.center_x - (model_min.x + model_max.x) * 0.5,
            -model_min.y,
            slot.bounds.min.z - model_min.z,
        )
    }

    /// The three pendant lamps, left to right, over the middle of the floor.
    pub fn lamps(&self) -> [Lamp; 3] {
        let spacing =
            (self.wall_width() / 3.0).clamp(*LAMP_SPACING_M.start(), *LAMP_SPACING_M.end());
        let ceiling = self.wall_height();

        [-spacing, 0.0, spacing].map(|x| Lamp {
            ceiling: Vec3::new(x, ceiling, 0.0),
            light: Vec3::new(x, ceiling - LAMP_DROP_M, 0.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(width: u32, height: u32) -> KitchenLayout {
        KitchenLayout::new(width, height).expect("a valid wall size")
    }

    #[test]
    fn rejects_walls_outside_the_allowed_range() {
        assert_eq!(
            KitchenLayout::new(199, 260),
            Err(LayoutError::WallWidth(199))
        );
        assert_eq!(
            KitchenLayout::new(501, 260),
            Err(LayoutError::WallWidth(501))
        );
        assert_eq!(
            KitchenLayout::new(300, 249),
            Err(LayoutError::WallHeight(249))
        );
        assert_eq!(
            KitchenLayout::new(300, 301),
            Err(LayoutError::WallHeight(301))
        );

        assert!(KitchenLayout::new(200, 250).is_ok());
        assert!(KitchenLayout::new(500, 300).is_ok());
    }

    #[test]
    fn fits_as_many_whole_modules_as_the_wall_allows() {
        for (width, expected) in [(200, 2), (239, 2), (240, 3), (360, 4), (400, 5), (500, 6)] {
            assert_eq!(layout(width, 260).slot_count(), expected, "{width} cm");
            assert_eq!(layout(width, 260).slots().len(), expected, "{width} cm");
        }
    }

    #[test]
    fn the_run_is_centred_on_the_wall_and_never_wider_than_it() {
        for width in [200, 330, 400, 500] {
            let layout = layout(width, 270);
            let slots = layout.slots();
            let wall = layout.wall_bounds();

            let left = slots.first().expect("at least two slots").bounds.min.x;
            let right = slots.last().expect("at least two slots").bounds.max.x;

            assert!((left + right).abs() < 1e-4, "{width} cm: not centred");
            assert!(left >= wall.min.x - 1e-4 && right <= wall.max.x + 1e-4);
        }
    }

    #[test]
    fn slots_sit_side_by_side_against_the_wall() {
        let layout = layout(400, 260);
        let slots = layout.slots();

        for pair in slots.windows(2) {
            assert!((pair[0].bounds.max.x - pair[1].bounds.min.x).abs() < 1e-5);
        }
        for slot in &slots {
            assert!(slot.bounds.min.z > layout.wall_front_z());
            assert!(slot.bounds.min.z - layout.wall_front_z() < 0.01);
            assert_eq!(slot.bounds.min.y, 0.0);
        }
    }

    #[test]
    fn the_wall_stands_on_the_back_edge_of_the_floor() {
        let layout = layout(360, 280);
        let wall = layout.wall_bounds();
        let floor = layout.floor_bounds();

        assert_eq!(wall.min.z, floor.min.z);
        assert!((wall.size() - Vec3::new(3.6, 2.8, WALL_THICKNESS_M)).length() < 1e-4);
        assert_eq!(floor.max.y, 0.0);
    }

    #[test]
    fn placement_centres_an_off_centre_model_in_its_slot() {
        let layout = layout(300, 260);
        let slot = layout.slots()[1];
        // Like the placeholder export: shifted 3 cm to the right, its front at
        // z = 0 and its back 58 cm behind that.
        let min = Vec3::new(-0.37, 0.0, -0.58);
        let max = Vec3::new(0.43, 0.87, 0.0);

        let translation = layout.placement(&slot, min, max);
        let placed_min = min + translation;
        let placed_max = max + translation;

        assert!(((placed_min.x + placed_max.x) * 0.5 - slot.center_x).abs() < 1e-5);
        assert!(placed_min.y.abs() < 1e-5);
        assert!((placed_min.z - slot.bounds.min.z).abs() < 1e-5);
    }

    #[test]
    fn lamps_hang_in_a_row_over_the_middle_of_the_floor() {
        let layout = layout(300, 270);
        let lamps = layout.lamps();

        assert_eq!(lamps[1].light.x, 0.0);
        assert!((lamps[0].light.x + lamps[2].light.x).abs() < 1e-5);
        for lamp in lamps {
            assert_eq!(lamp.light.z, 0.0);
            assert!((lamp.ceiling.y - 2.7).abs() < 1e-5);
            assert!((lamp.ceiling.y - lamp.light.y - LAMP_DROP_M).abs() < 1e-5);
        }
    }

    #[test]
    fn lamp_spacing_follows_the_wall_within_limits() {
        assert!((layout(200, 250).lamps()[2].light.x - 0.8).abs() < 1e-5);
        assert!((layout(360, 250).lamps()[2].light.x - 1.2).abs() < 1e-5);
        assert!((layout(500, 250).lamps()[2].light.x - 1.5).abs() < 1e-5);
    }
}
