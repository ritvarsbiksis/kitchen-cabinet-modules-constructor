//! An interactive kitchen constructor that renders into an HTML canvas with
//! `wgpu`.
//!
//! The web app calls [`start_kitchen`] with a canvas, the wall size the user
//! entered, the placeholder `.glb`, the two skybox images and a callback. Rust
//! builds the room - a tiled floor, the wall, three pendant lamps - and lines the
//! wall with as many placeholder boxes as fit. Hovering one lights it up, and
//! clicking it hands its slot back to JavaScript, which picks a module and calls
//! `placeModule` on the returned handle.
//!
//! `layout`, `camera`, `picking`, `geometry` and `floor` are target independent
//! and unit tested with a plain `cargo test`; the modules below them only exist
//! on `wasm32`.

pub mod camera;
pub mod floor;
pub mod geometry;
pub mod layout;
pub mod picking;

#[cfg(target_arch = "wasm32")]
mod constructor;
#[cfg(target_arch = "wasm32")]
mod renderer;

#[cfg(target_arch = "wasm32")]
pub use constructor::{start_kitchen, KitchenConstructor};

#[cfg(test)]
mod asset_tests {
    //! The three `.glb` files the constructor places, checked against the size
    //! the layout assumes for a slot.

    use scene_assets::model::Model;

    const PLACEHOLDER: &[u8] =
        include_bytes!("../../../apps/web/public/models/kitchen-placeholder-box.glb");
    const POLISHED_STEEL: &[u8] =
        include_bytes!("../../../apps/web/public/models/kitchen-module-1.glb");
    const ALUMINIUM: &[u8] =
        include_bytes!("../../../apps/web/public/models/kitchen-module-1-aluminium.glb");

    fn assert_fits_a_slot(name: &str, bytes: &[u8]) {
        let model = Model::from_glb_in_metres(bytes).expect("the bundled .glb should parse");
        let (min, max) = model.bounds();
        let size = max - min;

        // 80 x 87 x 58 cm, within a centimetre. A stray object in the export -
        // Blender's default 2 m cube, say - would blow straight through this.
        for (axis, actual, expected) in [
            ("width", size.x, crate::layout::MODULE_WIDTH_M),
            ("height", size.y, crate::layout::MODULE_HEIGHT_M),
            ("depth", size.z, crate::layout::MODULE_DEPTH_M),
        ] {
            assert!(
                (actual - expected).abs() < 0.011,
                "{name}: {axis} is {actual} m, expected {expected} m"
            );
        }
        assert!(min.y.abs() < 0.01, "{name} should stand on the floor");
    }

    #[test]
    fn the_placeholder_is_the_size_of_one_slot() {
        assert_fits_a_slot("placeholder", PLACEHOLDER);
    }

    #[test]
    fn both_modules_are_the_size_of_one_slot() {
        assert_fits_a_slot("polished steel", POLISHED_STEEL);
        assert_fits_a_slot("aluminium", ALUMINIUM);
    }

    #[test]
    fn the_placeholder_is_see_through() {
        let model = Model::from_glb_in_metres(PLACEHOLDER).expect("the bundled .glb should parse");
        assert!(model
            .primitives
            .iter()
            .all(|primitive| primitive.material.is_transparent()));
    }
}
