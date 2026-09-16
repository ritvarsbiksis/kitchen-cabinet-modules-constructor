//! The browser-facing half of the viewer: the exported entry point, the pointer
//! and wheel handlers, and the animation frame loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::camera::OrbitCamera;
use crate::environment::Environment;
use crate::model::Model;
use crate::renderer::Renderer;

/// Radians per frame the model turns by before the user first touches it.
const AUTO_ROTATE_SPEED: f32 = 0.0035;
/// Zoom applied per pixel of wheel delta, as an exponent - so a tick feels the
/// same whether the camera is close in or far out.
const WHEEL_ZOOM_SPEED: f32 = 0.0015;
/// A wheel event reporting lines or pages rather than pixels is scaled by this.
const LINE_HEIGHT: f32 = 16.0;

/// Routes Rust panics to the browser console instead of an opaque trap.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Everything a frame needs, shared between the event listeners and the loop.
struct State {
    renderer: Renderer,
    camera: OrbitCamera,
    canvas: web_sys::HtmlCanvasElement,
    /// Active pointers by id, with their last position in CSS pixels. More than
    /// one means a pinch gesture rather than a drag.
    pointers: HashMap<i32, (f32, f32)>,
    /// Distance between the two pinching pointers on the previous move.
    pinch_distance: Option<f32>,
    /// Turns the model until the first interaction, so it reads as 3D at a glance.
    auto_rotate: bool,
    /// Set whenever the camera or the canvas size changed; keeps the loop from
    /// re-rendering an identical frame.
    dirty: bool,
    running: bool,
    frame_handle: Option<i32>,
}

impl State {
    /// Advance and draw a single frame. Returns an error message if the frame
    /// could not be drawn, in which case the loop stops.
    fn tick(&mut self) -> Result<(), String> {
        if self.auto_rotate {
            self.camera.spin(AUTO_ROTATE_SPEED);
            self.dirty = true;
        }

        if !self.dirty {
            return Ok(());
        }

        self.dirty = false;
        self.renderer.render(&self.camera)
    }

    /// Match the drawing buffer to the canvas' CSS size at the current device
    /// pixel ratio, so the render stays sharp on high-DPI displays.
    fn sync_size(&mut self) {
        let ratio = window().device_pixel_ratio().clamp(1.0, 2.0);
        let width = (f64::from(self.canvas.client_width()) * ratio)
            .round()
            .max(1.0) as u32;
        let height = (f64::from(self.canvas.client_height()) * ratio)
            .round()
            .max(1.0) as u32;

        if (width, height) == self.renderer.size() {
            return;
        }

        // The canvas attributes are the drawing buffer size; its CSS size is
        // left to the stylesheet.
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.renderer.resize(width, height);
        self.camera.set_aspect(width as f32, height as f32);
        self.dirty = true;
    }
}

/// The animation frame closure, owned by the cell that re-arms it each frame.
type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

/// A running viewer. Dropping the JS handle does not stop it - call `destroy()`,
/// which the React component does when the modal closes.
#[wasm_bindgen]
pub struct Viewer {
    state: Rc<RefCell<State>>,
    /// Kept alive for as long as the viewer runs, then dropped in `destroy`.
    listeners: Vec<Listener>,
    observer: Option<web_sys::ResizeObserver>,
    frame_callback: FrameCallback,
    backend: String,
    triangles: u32,
}

/// An event listener, kept together with what it is attached to so it can be
/// removed again.
struct Listener {
    target: web_sys::EventTarget,
    event: &'static str,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}

impl Listener {
    fn remove(&self) {
        let _ = self.target.remove_event_listener_with_callback(
            self.event,
            self.callback.as_ref().unchecked_ref(),
        );
    }
}

#[wasm_bindgen]
impl Viewer {
    /// Which wgpu backend the browser gave us: `WebGPU` or `WebGL2`.
    #[wasm_bindgen(getter)]
    pub fn backend(&self) -> String {
        self.backend.clone()
    }

    /// Triangles in the loaded model.
    #[wasm_bindgen(getter, js_name = triangleCount)]
    pub fn triangle_count(&self) -> u32 {
        self.triangles
    }

    /// Return the camera to the framing it started with.
    #[wasm_bindgen(js_name = resetView)]
    pub fn reset_view(&self) {
        let mut state = self.state.borrow_mut();
        state.camera.reset();
        state.auto_rotate = false;
        state.dirty = true;
    }

    /// Stop the frame loop, detach every listener and release the GPU resources.
    /// Safe to call more than once.
    pub fn destroy(&mut self) {
        {
            let mut state = self.state.borrow_mut();
            state.running = false;
            if let Some(handle) = state.frame_handle.take() {
                let _ = window().cancel_animation_frame(handle);
            }
        }

        for listener in self.listeners.drain(..) {
            listener.remove();
        }

        if let Some(observer) = self.observer.take() {
            observer.disconnect();
        }

        // Drops the closure that holds the last reference to the loop.
        self.frame_callback.borrow_mut().take();
    }
}

/// Load `model_bytes` and start rendering them into `canvas`.
///
/// `background_png` and `foreground_png` are the two skybox images. They are
/// what the model stands in and reflects, but they are not essential to seeing
/// it: empty or undecodable bytes leave the shader on its procedural gradient
/// and only cost a warning in the console.
///
/// Resolves once the first frame is on screen, so the caller can keep a loading
/// state up until the model is actually visible.
#[wasm_bindgen(js_name = startViewer)]
pub async fn start_viewer(
    canvas: web_sys::HtmlCanvasElement,
    model_bytes: Vec<u8>,
    background_png: Vec<u8>,
    foreground_png: Vec<u8>,
) -> Result<Viewer, JsValue> {
    let model =
        Model::from_glb(&model_bytes).map_err(|error| JsValue::from_str(&error.to_string()))?;
    let triangles = model.triangle_count() as u32;
    let environment = decode_environment(&background_png, &foreground_png);

    let ratio = window().device_pixel_ratio().clamp(1.0, 2.0);
    let width = (f64::from(canvas.client_width()) * ratio).round().max(1.0) as u32;
    let height = (f64::from(canvas.client_height()) * ratio).round().max(1.0) as u32;
    canvas.set_width(width);
    canvas.set_height(height);

    let renderer = Renderer::new(canvas.clone(), &model, environment.as_ref(), width, height)
        .await
        .map_err(|error| JsValue::from_str(&error))?;
    let backend = renderer.backend().to_owned();

    let mut camera = OrbitCamera::framing(model.radius);
    camera.set_aspect(width as f32, height as f32);

    let state = Rc::new(RefCell::new(State {
        renderer,
        camera,
        canvas: canvas.clone(),
        pointers: HashMap::new(),
        pinch_distance: None,
        auto_rotate: true,
        dirty: true,
        running: true,
        frame_handle: None,
    }));

    // Draw once up front so the promise resolves on a visible model rather than
    // on an empty canvas.
    state
        .borrow_mut()
        .tick()
        .map_err(|error| JsValue::from_str(&error))?;

    let listeners = attach_listeners(&canvas, &state);
    let observer = observe_resizes(&canvas, &state);
    let frame_callback = spawn_frame_loop(&state);

    Ok(Viewer {
        state,
        listeners,
        observer,
        frame_callback,
        backend,
        triangles,
    })
}

/// Decode the skybox, or explain in the console why the viewer is going without
/// it. A missing backdrop is worth a warning; it is not worth refusing to show
/// the model over.
fn decode_environment(background_png: &[u8], foreground_png: &[u8]) -> Option<Environment> {
    if background_png.is_empty() || foreground_png.is_empty() {
        web_sys::console::warn_1(&JsValue::from_str(
            "viewer: no skybox images were supplied, falling back to the built-in gradient",
        ));
        return None;
    }

    match Environment::decode_png(background_png, foreground_png) {
        Ok(environment) => Some(environment),
        Err(error) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "viewer: {error}, falling back to the built-in gradient"
            )));
            None
        }
    }
}

/// Wire up dragging to orbit, pinching and the wheel to zoom.
fn attach_listeners(
    canvas: &web_sys::HtmlCanvasElement,
    state: &Rc<RefCell<State>>,
) -> Vec<Listener> {
    let mut listeners = Vec::new();

    let target: web_sys::EventTarget = canvas.clone().into();

    listeners.push(listen(&target, "pointerdown", state, |state, event| {
        let Some(event) = event.dyn_ref::<web_sys::PointerEvent>() else {
            return;
        };
        // Capturing means a drag keeps working when the pointer leaves the canvas.
        let _ = state.canvas.set_pointer_capture(event.pointer_id());
        state.pointers.insert(event.pointer_id(), position(event));
        state.pinch_distance = None;
        state.auto_rotate = false;
    }));

    listeners.push(listen(&target, "pointermove", state, |state, event| {
        let Some(event) = event.dyn_ref::<web_sys::PointerEvent>() else {
            return;
        };
        if !state.pointers.contains_key(&event.pointer_id()) {
            return;
        }

        let current = position(event);
        let previous = state
            .pointers
            .insert(event.pointer_id(), current)
            .unwrap_or(current);

        match state.pointers.len() {
            1 => {
                state
                    .camera
                    .orbit(current.0 - previous.0, current.1 - previous.1);
                state.dirty = true;
            }
            // Two fingers: the change in their separation drives the zoom.
            2 => {
                let mut positions = state.pointers.values();
                let (Some(&a), Some(&b)) = (positions.next(), positions.next()) else {
                    return;
                };
                let distance = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();

                if let Some(previous_distance) = state.pinch_distance {
                    if distance > 1.0 && previous_distance > 1.0 {
                        state.camera.zoom_by(previous_distance / distance);
                        state.dirty = true;
                    }
                }
                state.pinch_distance = Some(distance);
            }
            _ => {}
        }
    }));

    for event in ["pointerup", "pointercancel"] {
        listeners.push(listen(&target, event, state, |state, event| {
            let Some(event) = event.dyn_ref::<web_sys::PointerEvent>() else {
                return;
            };
            state.pointers.remove(&event.pointer_id());
            if state.pointers.len() < 2 {
                state.pinch_distance = None;
            }
        }));
    }

    listeners.push(listen_non_passive(
        &target,
        "wheel",
        state,
        |state, event| {
            let Some(event) = event.dyn_ref::<web_sys::WheelEvent>() else {
                return;
            };
            // Without this the page behind the modal scrolls as well.
            event.prevent_default();
            state.auto_rotate = false;

            let delta = match event.delta_mode() {
                web_sys::WheelEvent::DOM_DELTA_PIXEL => event.delta_y() as f32,
                _ => event.delta_y() as f32 * LINE_HEIGHT,
            };
            state.camera.zoom_by((delta * WHEEL_ZOOM_SPEED).exp());
            state.dirty = true;
        },
    ));

    listeners
}

/// Pointer position in CSS pixels, relative to the viewport. Only differences
/// between successive events are used, so the origin does not matter.
fn position(event: &web_sys::PointerEvent) -> (f32, f32) {
    (event.client_x() as f32, event.client_y() as f32)
}

/// Add a listener that borrows the shared state for the duration of the call.
fn listen(
    target: &web_sys::EventTarget,
    event: &'static str,
    state: &Rc<RefCell<State>>,
    handler: impl Fn(&mut State, &web_sys::Event) + 'static,
) -> Listener {
    let callback = state_callback(state, handler);
    let _ = target.add_event_listener_with_callback(event, callback.as_ref().unchecked_ref());

    Listener {
        target: target.clone(),
        event,
        callback,
    }
}

/// As [`listen`], but registers the listener as non-passive so it is allowed to
/// call `preventDefault()` - browsers default `wheel` to passive.
fn listen_non_passive(
    target: &web_sys::EventTarget,
    event: &'static str,
    state: &Rc<RefCell<State>>,
    handler: impl Fn(&mut State, &web_sys::Event) + 'static,
) -> Listener {
    let callback = state_callback(state, handler);
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false);

    let _ = target.add_event_listener_with_callback_and_add_event_listener_options(
        event,
        callback.as_ref().unchecked_ref(),
        &options,
    );

    Listener {
        target: target.clone(),
        event,
        callback,
    }
}

fn state_callback(
    state: &Rc<RefCell<State>>,
    handler: impl Fn(&mut State, &web_sys::Event) + 'static,
) -> Closure<dyn FnMut(web_sys::Event)> {
    let state = Rc::downgrade(state);

    Closure::wrap(Box::new(move |event: web_sys::Event| {
        let Some(state) = state.upgrade() else {
            return;
        };
        // A listener firing while a frame is mid-render would re-enter the
        // borrow; skipping the event is better than panicking.
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        if state.running {
            handler(&mut state, &event);
        }
    }) as Box<dyn FnMut(web_sys::Event)>)
}

/// Keep the drawing buffer in step with the element's CSS size.
fn observe_resizes(
    canvas: &web_sys::HtmlCanvasElement,
    state: &Rc<RefCell<State>>,
) -> Option<web_sys::ResizeObserver> {
    let weak = Rc::downgrade(state);

    let callback = Closure::wrap(Box::new(move |_: JsValue, _: JsValue| {
        let Some(state) = weak.upgrade() else {
            return;
        };
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        if state.running {
            state.sync_size();
        }
    }) as Box<dyn FnMut(JsValue, JsValue)>);

    let observer = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()).ok()?;
    observer.observe(canvas);
    // The observer owns the callback from here on.
    callback.forget();

    Some(observer)
}

/// Start the `requestAnimationFrame` loop, returning the cell that owns it.
fn spawn_frame_loop(state: &Rc<RefCell<State>>) -> FrameCallback {
    let callback: FrameCallback = Rc::new(RefCell::new(None));
    let scheduler = Rc::clone(&callback);
    let weak = Rc::downgrade(state);

    *callback.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        let Some(state) = weak.upgrade() else {
            return;
        };

        {
            let mut state = state.borrow_mut();
            if !state.running {
                return;
            }
            if let Err(error) = state.tick() {
                web_sys::console::error_1(&JsValue::from_str(&error));
                state.running = false;
                return;
            }
        }

        // Re-arm for the next frame. The handle is stored so `destroy` can
        // cancel a frame that is already queued.
        let handle = scheduler
            .borrow()
            .as_ref()
            .and_then(|callback| request_frame(callback).ok());
        state.borrow_mut().frame_handle = handle;
    }) as Box<dyn FnMut()>));

    let handle = callback
        .borrow()
        .as_ref()
        .and_then(|callback| request_frame(callback).ok());
    state.borrow_mut().frame_handle = handle;

    callback
}

fn request_frame(callback: &Closure<dyn FnMut()>) -> Result<i32, JsValue> {
    window().request_animation_frame(callback.as_ref().unchecked_ref())
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a viewer only runs in a browser, where `window` exists")
}
