//! The orbit camera the pointer and wheel handlers drive.
//!
//! Target independent and free of GPU types, so the interaction maths is unit
//! tested on the host rather than only in a browser.

use glam::{Mat4, Vec3};

/// Vertical field of view, in radians.
const FOV_Y: f32 = 45.0_f32.to_radians();
/// Just shy of a pole, so the view direction never degenerates into the up axis.
const MAX_PITCH: f32 = 1.553_343; // 89 degrees
/// Radians of orbit per pixel of pointer movement.
const ORBIT_SPEED: f32 = 0.007;

/// A camera that looks at the origin from a point on a sphere.
#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    /// Rotation around the Y axis, in radians.
    yaw: f32,
    /// Rotation away from the XZ plane, in radians, clamped short of the poles.
    pitch: f32,
    /// Distance from the origin.
    distance: f32,
    min_distance: f32,
    max_distance: f32,
    aspect: f32,
    initial: InitialView,
}

/// The framing [`OrbitCamera::reset`] returns to.
#[derive(Clone, Copy, Debug)]
struct InitialView {
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl OrbitCamera {
    /// Frame a model of the given bounding radius, with a three-quarter view
    /// that reads better as a first impression than a flat front-on shot.
    pub fn framing(radius: f32) -> Self {
        let radius = radius.max(f32::EPSILON);
        // Distance at which the bounding sphere just fills the vertical FOV. The
        // factor pulls in a little closer than that: the sphere is sized by the
        // widest axis, so a flat model would otherwise sit small in the frame.
        let distance = radius / (FOV_Y * 0.5).sin() * 0.85;
        let initial = InitialView {
            yaw: 0.6,
            pitch: 0.35,
            distance,
        };

        Self {
            yaw: initial.yaw,
            pitch: initial.pitch,
            distance,
            min_distance: radius * 1.1,
            max_distance: radius * 12.0,
            aspect: 1.0,
            initial,
        }
    }

    /// Rotate by a pointer drag, in CSS pixels.
    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw -= delta_x * ORBIT_SPEED;
        self.pitch = (self.pitch + delta_y * ORBIT_SPEED).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Rotate around the Y axis by an exact angle, used by the idle spin.
    pub fn spin(&mut self, radians: f32) {
        self.yaw += radians;
    }

    /// Move in or out. `factor` multiplies the current distance, so equal wheel
    /// ticks feel the same however close the camera already is.
    pub fn zoom_by(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(self.min_distance, self.max_distance);
    }

    /// Set the viewport aspect ratio (width / height).
    pub fn set_aspect(&mut self, width: f32, height: f32) {
        if width > 0.0 && height > 0.0 {
            self.aspect = width / height;
        }
    }

    /// Return to the framing the camera was created with.
    pub fn reset(&mut self) {
        self.yaw = self.initial.yaw;
        self.pitch = self.initial.pitch;
        self.distance = self.initial.distance;
    }

    /// Where the camera currently sits, in world space.
    pub fn eye(&self) -> Vec3 {
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let distance = self.effective_distance();

        Vec3::new(
            distance * cos_pitch * sin_yaw,
            distance * sin_pitch,
            distance * cos_pitch * cos_yaw,
        )
    }

    /// The distance actually used for the view.
    ///
    /// A perspective projection only fits the vertical field of view, so a
    /// viewport that is taller than it is wide - a phone, or the modal on a
    /// narrow window - would crop the model at the sides. Backing off by the
    /// aspect ratio keeps the whole model in frame at any shape.
    fn effective_distance(&self) -> f32 {
        self.distance / self.aspect.min(1.0)
    }

    /// Combined view-projection matrix, with the reversed-depth-free `wgpu`
    /// clip space convention (Z from 0 to 1).
    pub fn view_projection(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), Vec3::ZERO, Vec3::Y);
        let projection = Mat4::perspective_rh(FOV_Y, self.aspect, 0.01, 100.0);
        projection * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_the_model_from_outside_its_bounding_sphere() {
        let camera = OrbitCamera::framing(1.0);
        assert!(camera.eye().length() > 1.0);
    }

    #[test]
    fn orbiting_keeps_the_camera_at_the_same_distance() {
        let mut camera = OrbitCamera::framing(1.0);
        let distance = camera.eye().length();

        camera.orbit(120.0, -45.0);

        assert!((camera.eye().length() - distance).abs() < 1e-4);
    }

    #[test]
    fn pitch_stops_short_of_the_poles() {
        let mut camera = OrbitCamera::framing(1.0);

        camera.orbit(0.0, 100_000.0);
        assert!(camera.pitch < MAX_PITCH + 1e-6 && camera.pitch > 0.0);

        camera.orbit(0.0, -200_000.0);
        assert!(camera.pitch > -MAX_PITCH - 1e-6 && camera.pitch < 0.0);
    }

    #[test]
    fn zoom_is_clamped_to_the_configured_range() {
        let mut camera = OrbitCamera::framing(2.0);

        camera.zoom_by(0.001);
        assert!((camera.distance - camera.min_distance).abs() < 1e-4);

        camera.zoom_by(1000.0);
        assert!((camera.distance - camera.max_distance).abs() < 1e-4);
    }

    #[test]
    fn a_portrait_viewport_pulls_the_camera_back_so_nothing_is_cropped() {
        let mut camera = OrbitCamera::framing(1.0);
        camera.set_aspect(800.0, 800.0);
        let square = camera.eye().length();

        camera.set_aspect(400.0, 800.0);
        let portrait = camera.eye().length();

        assert!(
            (portrait - square * 2.0).abs() < 1e-4,
            "{portrait} vs {square}"
        );

        // A wide viewport already fits the height, so it is left alone.
        camera.set_aspect(1600.0, 800.0);
        assert!((camera.eye().length() - square).abs() < 1e-4);
    }

    #[test]
    fn reset_restores_the_initial_framing() {
        let mut camera = OrbitCamera::framing(1.0);
        let eye = camera.eye();

        camera.orbit(300.0, 120.0);
        camera.zoom_by(0.5);
        camera.reset();

        assert!((camera.eye() - eye).length() < 1e-5);
    }

    #[test]
    fn the_model_origin_projects_to_the_centre_of_the_viewport() {
        let mut camera = OrbitCamera::framing(1.0);
        camera.set_aspect(800.0, 450.0);

        let clip = camera.view_projection() * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;

        assert!(ndc.x.abs() < 1e-5 && ndc.y.abs() < 1e-5);
        // wgpu clip space keeps Z in 0..1, so the origin has to land inside it.
        assert!((0.0..=1.0).contains(&ndc.z));
    }
}
