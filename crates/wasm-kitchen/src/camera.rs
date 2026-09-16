//! The camera the pointer and wheel handlers drive around the room.
//!
//! It orbits a point in front of the middle of the wall, but only through the
//! half of the room a visitor could actually stand in: it never swings behind
//! the wall or dips under the floor. Target independent and free of GPU types,
//! so the maths - including the picking ray - is unit tested on the host.

use glam::{Mat4, Vec3, Vec4Swizzles};

use crate::layout::{KitchenLayout, MODULE_DEPTH_M};
use crate::picking::Ray;

/// Vertical field of view, in radians.
const FOV_Y: f32 = 45.0_f32.to_radians();
const NEAR: f32 = 0.05;
const FAR: f32 = 80.0;

/// Left and right of straight-on the camera may swing: far enough to look along
/// the run of modules, not so far it ends up looking at the wall edge-on.
const MAX_YAW: f32 = 80.0_f32.to_radians();
/// Kept above the floor, and short of looking straight down.
const MIN_PITCH: f32 = 5.0_f32.to_radians();
const MAX_PITCH: f32 = 70.0_f32.to_radians();
/// Closest the camera may get to its target, in metres.
const MIN_DISTANCE: f32 = 1.2;
/// Furthest out the camera may zoom, as a multiple of the distance that frames
/// the whole wall.
const MAX_ZOOM_OUT: f32 = 2.5;
/// Radians of orbit per CSS pixel of pointer movement.
const ORBIT_SPEED: f32 = 0.006;

/// The first view: a little to the right and from about eye height, which reads
/// as a room at a glance where straight-on would look like an elevation drawing.
/// Any higher and the pendant lamps hang in front of the modules.
const INITIAL_YAW: f32 = 0.3;
const INITIAL_PITCH: f32 = 0.16;
/// Space left around the wall when framing it, in metres. Generous, so the
/// camera starts back beyond the lamps hanging in the middle of the room.
const FRAME_MARGIN_X: f32 = 1.3;
const FRAME_MARGIN_Y: f32 = 0.8;

#[derive(Clone, Copy, Debug)]
pub struct RoomCamera {
    /// The point orbited around and looked at.
    focus: Vec3,
    /// Rotation around Y away from straight-on to the wall, in radians.
    yaw: f32,
    /// Elevation above the horizontal, in radians.
    pitch: f32,
    /// Multiplies the framing distance: below 1 is zoomed in.
    zoom: f32,
    aspect: f32,
    /// Half the width and height the camera should keep in frame at zoom 1.
    subject: (f32, f32),
}

impl RoomCamera {
    /// A camera framing the wall of `layout`, with the run of modules in the
    /// middle of the view.
    pub fn framing(layout: &KitchenLayout) -> Self {
        let focus = Vec3::new(
            0.0,
            layout.wall_height() * 0.5,
            layout.wall_front_z() + MODULE_DEPTH_M,
        );

        Self {
            focus,
            yaw: INITIAL_YAW,
            pitch: INITIAL_PITCH,
            zoom: 1.0,
            aspect: 16.0 / 9.0,
            subject: (
                layout.wall_width() * 0.5 + FRAME_MARGIN_X,
                layout.wall_height() * 0.5 + FRAME_MARGIN_Y,
            ),
        }
    }

    /// Rotate by a pointer drag, in CSS pixels.
    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw = (self.yaw - delta_x * ORBIT_SPEED).clamp(-MAX_YAW, MAX_YAW);
        self.pitch = (self.pitch + delta_y * ORBIT_SPEED).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Move in or out. `factor` multiplies the current distance, so equal wheel
    /// ticks feel the same however close the camera already is.
    pub fn zoom_by(&mut self, factor: f32) {
        let framing = self.framing_distance();
        let min_zoom = (MIN_DISTANCE / framing).min(1.0);
        self.zoom = (self.zoom * factor).clamp(min_zoom, MAX_ZOOM_OUT);
    }

    /// Set the viewport aspect ratio (width / height).
    pub fn set_aspect(&mut self, width: f32, height: f32) {
        if width > 0.0 && height > 0.0 {
            self.aspect = width / height;
        }
    }

    /// Return to the initial framing.
    pub fn reset(&mut self) {
        self.yaw = INITIAL_YAW;
        self.pitch = INITIAL_PITCH;
        self.zoom = 1.0;
    }

    /// The point the camera looks at.
    pub fn focus(&self) -> Vec3 {
        self.focus
    }

    /// Distance from the focus at zoom 1: close enough to fill the view, far
    /// enough that the whole wall fits across the width and up the height at
    /// the current aspect ratio.
    fn framing_distance(&self) -> f32 {
        let tan_half_fov = (FOV_Y * 0.5).tan();
        let (half_width, half_height) = self.subject;
        let vertical = half_height / tan_half_fov;
        let horizontal = half_width / (tan_half_fov * self.aspect);
        vertical.max(horizontal)
    }

    /// Distance from the focus actually used for the view.
    pub fn distance(&self) -> f32 {
        (self.framing_distance() * self.zoom).max(MIN_DISTANCE)
    }

    /// Where the camera currently sits, in world space.
    pub fn eye(&self) -> Vec3 {
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();

        self.focus
            + self.distance() * Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw)
    }

    /// Combined view-projection matrix, in wgpu's clip space (Z from 0 to 1).
    pub fn view_projection(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), self.focus, Vec3::Y);
        let projection = Mat4::perspective_rh(FOV_Y, self.aspect, NEAR, FAR);
        projection * view
    }

    /// The world-space ray through a point on the viewport, given in normalised
    /// device coordinates: X from -1 (left) to 1 (right), Y from -1 (bottom) to
    /// 1 (top).
    pub fn ray_from_ndc(&self, x: f32, y: f32) -> Ray {
        let inverse = self.view_projection().inverse();
        let near = inverse * glam::Vec4::new(x, y, 0.0, 1.0);
        let far = inverse * glam::Vec4::new(x, y, 1.0, 1.0);
        let near = near.xyz() / near.w;
        let far = far.xyz() / far.w;

        Ray::new(near, far - near)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(width: u32, height: u32) -> (KitchenLayout, RoomCamera) {
        let layout = KitchenLayout::new(width, height).expect("a valid wall size");
        (layout, RoomCamera::framing(&layout))
    }

    #[test]
    fn starts_in_front_of_the_wall_and_above_the_floor() {
        let (layout, camera) = camera(360, 270);
        let eye = camera.eye();

        assert!(eye.z > layout.wall_front_z() + 1.0);
        assert!(eye.y > 1.0);
    }

    #[test]
    fn cannot_orbit_behind_the_wall_or_under_the_floor() {
        let (layout, mut camera) = camera(500, 300);

        for (dx, dy) in [
            (100_000.0, 0.0),
            (-100_000.0, 0.0),
            (0.0, 100_000.0),
            (0.0, -100_000.0),
        ] {
            camera.orbit(dx, dy);
            let eye = camera.eye();
            assert!(
                eye.z > layout.wall_front_z(),
                "behind the wall after ({dx}, {dy})"
            );
            assert!(eye.y > 0.0, "under the floor after ({dx}, {dy})");
        }
    }

    #[test]
    fn zoom_is_clamped_both_ways() {
        let (_, mut camera) = camera(300, 260);

        camera.zoom_by(0.0001);
        assert!((camera.distance() - MIN_DISTANCE).abs() < 1e-3);

        let framing = camera.framing_distance();
        camera.zoom_by(10_000.0);
        assert!((camera.distance() - framing * MAX_ZOOM_OUT).abs() < 1e-3);
    }

    #[test]
    fn a_wider_wall_is_framed_from_further_away() {
        let (_, narrow) = camera(200, 260);
        let (_, wide) = camera(500, 260);

        assert!(wide.distance() > narrow.distance());
    }

    #[test]
    fn a_portrait_viewport_backs_off_so_the_wall_still_fits() {
        let (_, mut camera) = camera(400, 260);
        camera.set_aspect(1600.0, 900.0);
        let landscape = camera.distance();

        camera.set_aspect(400.0, 800.0);
        assert!(camera.distance() > landscape * 2.0);
    }

    #[test]
    fn reset_restores_the_initial_view() {
        let (_, mut camera) = camera(300, 250);
        let eye = camera.eye();

        camera.orbit(250.0, -80.0);
        camera.zoom_by(0.4);
        camera.reset();

        assert!((camera.eye() - eye).length() < 1e-4);
    }

    #[test]
    fn the_ray_through_the_middle_of_the_view_passes_through_the_focus() {
        let (_, mut camera) = camera(360, 270);
        camera.set_aspect(1280.0, 720.0);
        camera.orbit(40.0, 25.0);

        let ray = camera.ray_from_ndc(0.0, 0.0);
        let to_focus = camera.focus() - ray.origin;
        let along = to_focus.dot(ray.direction);

        assert!(along > 0.0);
        assert!((ray.at(along) - camera.focus()).length() < 1e-3);
    }

    #[test]
    fn a_ray_through_a_projected_point_leads_back_to_it() {
        let (_, mut camera) = camera(360, 270);
        camera.set_aspect(1000.0, 700.0);

        let point = Vec3::new(1.1, 0.5, -2.0);
        let clip = camera.view_projection() * point.extend(1.0);
        let ndc = clip.xyz() / clip.w;

        let ray = camera.ray_from_ndc(ndc.x, ndc.y);
        let along = (point - ray.origin).dot(ray.direction);
        assert!((ray.at(along) - point).length() < 1e-3);
    }
}
