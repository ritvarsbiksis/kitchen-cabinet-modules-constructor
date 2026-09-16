//! The browser-facing half of the constructor: the exported entry point, the
//! pointer handlers that orbit, zoom, hover and click, and the animation frame
//! loop. The listener, resize and frame-loop plumbing follows
//! `crates/wasm-viewer/src/viewer.rs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use glam::Vec3;
use scene_assets::environment::Environment;
use scene_assets::model::Model;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::camera::RoomCamera;
use crate::layout::{KitchenLayout, Slot};
use crate::picking::{pick_nearest, Aabb};
use crate::renderer::{Renderer, SlotView};

/// Zoom applied per pixel of wheel delta, as an exponent.
const WHEEL_ZOOM_SPEED: f32 = 0.0015;
/// A wheel event reporting lines or pages rather than pixels is scaled by this.
const LINE_HEIGHT: f32 = 16.0;
/// A press that moves further than this, in CSS pixels, is a drag, not a click.
const CLICK_SLOP: f32 = 6.0;
/// Time constant of the hover glow easing in and out, in milliseconds.
const HIGHLIGHT_EASING_MS: f64 = 70.0;

/// Routes Rust panics to the browser console instead of an opaque trap.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// One slot along the wall.
struct SlotState {
    slot: Slot,
    /// The module standing in it, by id, or `None` for the placeholder.
    module: Option<String>,
    /// Where the current model stands, worked out from its bounds.
    translation: Vec3,
    /// How lit up it is right now, easing towards 1 while hovered.
    highlight: f32,
}

/// A press that may still turn out to be a click.
struct Press {
    pointer_id: i32,
    start: (f32, f32),
    dragged: bool,
}

/// Everything a frame needs, shared between the event listeners and the loop.
struct State {
    renderer: Renderer,
    camera: RoomCamera,
    layout: KitchenLayout,
    slots: Vec<SlotState>,
    canvas: web_sys::HtmlCanvasElement,
    /// Called with `(slot, moduleId | null)` when a slot is clicked.
    on_slot_click: js_sys::Function,
    /// The slot under the pointer.
    hovered: Option<usize>,
    /// Active pointers by id, with their last position in CSS pixels.
    pointers: HashMap<i32, (f32, f32)>,
    press: Option<Press>,
    pinch_distance: Option<f32>,
    /// A click to report once the state is no longer borrowed, so JavaScript
    /// is free to call straight back into the handle.
    pending_click: Option<usize>,
    last_frame_ms: Option<f64>,
    dirty: bool,
    running: bool,
    frame_handle: Option<i32>,
}

impl State {
    /// Advance the hover animation and draw a frame if anything changed.
    fn tick(&mut self, now_ms: f64) -> Result<(), String> {
        let elapsed = self
            .last_frame_ms
            .map_or(16.0, |last| (now_ms - last).clamp(0.0, 100.0));
        self.last_frame_ms = Some(now_ms);

        let blend = (1.0 - (-elapsed / HIGHLIGHT_EASING_MS).exp()) as f32;
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let target = if self.hovered == Some(index) {
                1.0
            } else {
                0.0
            };
            let difference = target - slot.highlight;
            if difference.abs() < 0.002 {
                if slot.highlight != target {
                    slot.highlight = target;
                    self.dirty = true;
                }
            } else {
                slot.highlight += difference * blend;
                self.dirty = true;
            }
        }

        if !self.dirty {
            return Ok(());
        }
        self.dirty = false;
        self.draw()
    }

    fn draw(&mut self) -> Result<(), String> {
        let views: Vec<SlotView<'_>> = self
            .slots
            .iter()
            .map(|slot| SlotView {
                module: slot.module.as_deref(),
                translation: slot.translation,
                highlight: slot.highlight,
                bounds: slot.slot.bounds,
            })
            .collect();
        self.renderer.render(&self.camera, &views)
    }

    /// Match the drawing buffer to the canvas' CSS size at the current device
    /// pixel ratio.
    fn sync_size(&mut self) {
        let (width, height) = physical_size(&self.canvas);
        if (width, height) == self.renderer.size() {
            return;
        }

        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.renderer.resize(width, height);
        self.camera.set_aspect(width as f32, height as f32);
        self.dirty = true;
    }

    /// Stand whatever `slot` now holds in the right place.
    fn place(&mut self, index: usize) {
        let slot = &self.slots[index];
        let (min, max) = self.renderer.model_bounds(slot.module.as_deref());
        let translation = self.layout.placement(&slot.slot, min, max);
        self.slots[index].translation = translation;
        self.dirty = true;
    }

    /// The slot under a point given in viewport CSS pixels.
    fn pick(&self, client: (f32, f32)) -> Option<usize> {
        let rect = self.canvas.get_bounding_client_rect();
        let (width, height) = (rect.width() as f32, rect.height() as f32);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        let x = (client.0 - rect.left() as f32) / width * 2.0 - 1.0;
        let y = 1.0 - (client.1 - rect.top() as f32) / height * 2.0;
        if !(-1.0..=1.0).contains(&x) || !(-1.0..=1.0).contains(&y) {
            return None;
        }

        let ray = self.camera.ray_from_ndc(x, y);
        let boxes: Vec<Aabb> = self.slots.iter().map(|slot| slot.slot.bounds).collect();
        pick_nearest(&ray, &boxes)
    }

    fn set_hovered(&mut self, hovered: Option<usize>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            self.dirty = true;
        }
        self.update_cursor();
    }

    fn update_cursor(&self) {
        let cursor = if self.press.as_ref().is_some_and(|press| press.dragged) {
            "grabbing"
        } else if self.hovered.is_some() {
            "pointer"
        } else {
            "grab"
        };
        let _ = self.canvas.style().set_property("cursor", cursor);
    }
}

/// The animation frame closure, owned by the cell that re-arms it each frame.
type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

/// A running constructor. Dropping the JS handle does not stop it - call
/// `destroy()`, which the React component does when it unmounts.
#[wasm_bindgen]
pub struct KitchenConstructor {
    state: Rc<RefCell<State>>,
    listeners: Vec<Listener>,
    observer: Option<web_sys::ResizeObserver>,
    frame_callback: FrameCallback,
    backend: String,
    slot_count: u32,
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
impl KitchenConstructor {
    /// Which wgpu backend the browser gave us: `WebGPU` or `WebGL2`.
    #[wasm_bindgen(getter)]
    pub fn backend(&self) -> String {
        self.backend.clone()
    }

    /// How many slots fit along the wall.
    #[wasm_bindgen(getter, js_name = slotCount)]
    pub fn slot_count(&self) -> u32 {
        self.slot_count
    }

    /// Put the module `module_id` in `slot`, replacing whatever is there.
    ///
    /// `glb` is only parsed the first time an id is seen; after that the
    /// uploaded model is reused and the bytes are ignored, so the caller does
    /// not need to track what Rust already has.
    #[wasm_bindgen(js_name = placeModule)]
    pub fn place_module(&self, slot: u32, module_id: String, glb: Vec<u8>) -> Result<(), JsValue> {
        let mut state = self.state.borrow_mut();
        let index = slot_index(&state, slot)?;

        if !state.renderer.has_module(&module_id) {
            let model = Model::from_glb_in_metres(&glb).map_err(|error| {
                JsValue::from_str(&format!("could not load module `{module_id}`: {error}"))
            })?;
            state.renderer.add_module(&module_id, &model);
        }

        state.slots[index].module = Some(module_id);
        state.place(index);
        Ok(())
    }

    /// Take the module out of `slot`, leaving the placeholder.
    #[wasm_bindgen(js_name = clearSlot)]
    pub fn clear_slot(&self, slot: u32) -> Result<(), JsValue> {
        let mut state = self.state.borrow_mut();
        let index = slot_index(&state, slot)?;

        state.slots[index].module = None;
        state.place(index);
        Ok(())
    }

    /// The id of the module in `slot`, or `undefined` for the placeholder.
    #[wasm_bindgen(js_name = slotModule)]
    pub fn slot_module(&self, slot: u32) -> Option<String> {
        let state = self.state.borrow();
        state
            .slots
            .get(slot as usize)
            .and_then(|slot| slot.module.clone())
    }

    /// Return the camera to the framing it started with.
    #[wasm_bindgen(js_name = resetView)]
    pub fn reset_view(&self) {
        let mut state = self.state.borrow_mut();
        state.camera.reset();
        state.dirty = true;
    }

    /// Stop the frame loop, detach every listener and release the GPU
    /// resources. Safe to call more than once.
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

        self.frame_callback.borrow_mut().take();
    }
}

fn slot_index(state: &State, slot: u32) -> Result<usize, JsValue> {
    let index = slot as usize;
    if index < state.slots.len() {
        Ok(index)
    } else {
        Err(JsValue::from_str(&format!(
            "there is no slot {slot}; the wall has {}",
            state.slots.len()
        )))
    }
}

/// Build the room for a `wall_width_cm` x `wall_height_cm` wall in `canvas` and
/// line the wall with placeholders.
///
/// `background_png` and `foreground_png` are the skybox the metal fronts
/// reflect; empty bytes fall back to a gradient. `on_slot_click` is called with
/// the slot index and the id of the module in it (or `null`) whenever a slot is
/// clicked or tapped.
///
/// Resolves once the first frame is on screen.
#[wasm_bindgen(js_name = startKitchen)]
pub async fn start_kitchen(
    canvas: web_sys::HtmlCanvasElement,
    wall_width_cm: u32,
    wall_height_cm: u32,
    placeholder_glb: Vec<u8>,
    background_png: Vec<u8>,
    foreground_png: Vec<u8>,
    on_slot_click: js_sys::Function,
) -> Result<KitchenConstructor, JsValue> {
    let layout = KitchenLayout::new(wall_width_cm, wall_height_cm)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let placeholder = Model::from_glb_in_metres(&placeholder_glb)
        .map_err(|error| JsValue::from_str(&format!("could not load the placeholder: {error}")))?;
    let environment = decode_environment(&background_png, &foreground_png);

    let (width, height) = physical_size(&canvas);
    canvas.set_width(width);
    canvas.set_height(height);

    let renderer = Renderer::new(
        canvas.clone(),
        &layout,
        &placeholder,
        environment.as_ref(),
        width,
        height,
    )
    .await
    .map_err(|error| JsValue::from_str(&error))?;
    let backend = renderer.backend().to_owned();

    let mut camera = RoomCamera::framing(&layout);
    camera.set_aspect(width as f32, height as f32);

    let slots = layout
        .slots()
        .into_iter()
        .map(|slot| SlotState {
            slot,
            module: None,
            translation: Vec3::ZERO,
            highlight: 0.0,
        })
        .collect::<Vec<_>>();
    let slot_count = slots.len() as u32;

    let state = Rc::new(RefCell::new(State {
        renderer,
        camera,
        layout,
        slots,
        canvas: canvas.clone(),
        on_slot_click,
        hovered: None,
        pointers: HashMap::new(),
        press: None,
        pinch_distance: None,
        pending_click: None,
        last_frame_ms: None,
        dirty: true,
        running: true,
        frame_handle: None,
    }));

    {
        let mut state = state.borrow_mut();
        for index in 0..state.slots.len() {
            state.place(index);
        }
        state.update_cursor();
        // Draw once up front so the promise resolves on a visible room.
        state.draw().map_err(|error| JsValue::from_str(&error))?;
        state.dirty = false;
    }

    let listeners = attach_listeners(&canvas, &state);
    let observer = observe_resizes(&canvas, &state);
    let frame_callback = spawn_frame_loop(&state);

    Ok(KitchenConstructor {
        state,
        listeners,
        observer,
        frame_callback,
        backend,
        slot_count,
    })
}

/// Decode the skybox, or explain in the console why the room is going without
/// it.
fn decode_environment(background_png: &[u8], foreground_png: &[u8]) -> Option<Environment> {
    if background_png.is_empty() || foreground_png.is_empty() {
        web_sys::console::warn_1(&JsValue::from_str(
            "kitchen: no skybox images were supplied, the fronts reflect a gradient instead",
        ));
        return None;
    }

    match Environment::decode_png(background_png, foreground_png) {
        Ok(environment) => Some(environment),
        Err(error) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "kitchen: {error}, the fronts reflect a gradient instead"
            )));
            None
        }
    }
}

/// Wire up dragging to orbit, pinching and the wheel to zoom, and hovering and
/// clicking to pick slots.
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
        let _ = state.canvas.set_pointer_capture(event.pointer_id());
        let position = position(event);
        state.pointers.insert(event.pointer_id(), position);
        state.pinch_distance = None;

        if state.pointers.len() == 1 {
            state.press = Some(Press {
                pointer_id: event.pointer_id(),
                start: position,
                dragged: false,
            });
            // A touch has no hover, so light the slot up as it is pressed.
            let picked = state.pick(position);
            state.set_hovered(picked);
        } else {
            // A second finger turns the gesture into a pinch.
            state.press = None;
            state.set_hovered(None);
        }
    }));

    listeners.push(listen(&target, "pointermove", state, |state, event| {
        let Some(event) = event.dyn_ref::<web_sys::PointerEvent>() else {
            return;
        };
        let current = position(event);

        let Some(previous) = state.pointers.get(&event.pointer_id()).copied() else {
            // Nothing pressed: plain hovering.
            let picked = state.pick(current);
            state.set_hovered(picked);
            return;
        };
        state.pointers.insert(event.pointer_id(), current);

        match state.pointers.len() {
            1 => {
                let started_drag = match state.press.as_mut() {
                    Some(press) if !press.dragged => {
                        let (dx, dy) = (current.0 - press.start.0, current.1 - press.start.1);
                        press.dragged = (dx * dx + dy * dy).sqrt() > CLICK_SLOP;
                        press.dragged
                    }
                    _ => false,
                };
                if started_drag {
                    state.set_hovered(None);
                }
                if state.press.as_ref().is_none_or(|press| press.dragged) {
                    state
                        .camera
                        .orbit(current.0 - previous.0, current.1 - previous.1);
                    state.dirty = true;
                }
            }
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

    listeners.push(listen(&target, "pointerup", state, |state, event| {
        let Some(event) = event.dyn_ref::<web_sys::PointerEvent>() else {
            return;
        };
        let position = position(event);
        state.pointers.remove(&event.pointer_id());
        if state.pointers.len() < 2 {
            state.pinch_distance = None;
        }

        if let Some(press) = state.press.take() {
            if press.pointer_id == event.pointer_id() && !press.dragged {
                state.pending_click = state.pick(position);
            }
        }

        // A mouse is still over the canvas and keeps hovering; a finger has
        // gone, and so has the highlight.
        let hovered = if event.pointer_type() == "mouse" {
            state.pick(position)
        } else {
            None
        };
        state.set_hovered(hovered);
    }));

    listeners.push(listen(&target, "pointercancel", state, |state, event| {
        let Some(event) = event.dyn_ref::<web_sys::PointerEvent>() else {
            return;
        };
        state.pointers.remove(&event.pointer_id());
        state.pinch_distance = None;
        state.press = None;
        state.set_hovered(None);
    }));

    listeners.push(listen(&target, "pointerleave", state, |state, _| {
        // While a drag is captured the pointer may wander off the canvas and
        // back; only an idle pointer leaving clears the hover.
        if state.pointers.is_empty() {
            state.set_hovered(None);
        }
    }));

    listeners.push(listen_non_passive(
        &target,
        "wheel",
        state,
        |state, event| {
            let Some(event) = event.dyn_ref::<web_sys::WheelEvent>() else {
                return;
            };
            // Without this the page scrolls as well.
            event.prevent_default();

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

/// Pointer position in CSS pixels, relative to the viewport.
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

/// As [`listen`], but non-passive so it may call `preventDefault()`.
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

        let click = {
            let Ok(mut state) = state.try_borrow_mut() else {
                return;
            };
            if !state.running {
                return;
            }
            handler(&mut state, &event);

            state.pending_click.take().map(|index| {
                let module = state.slots[index].module.clone();
                (state.on_slot_click.clone(), index, module)
            })
        };

        // Outside the borrow: the callback is free to call `placeModule` and
        // friends synchronously.
        if let Some((callback, index, module)) = click {
            let module = module.map_or(JsValue::NULL, |id| JsValue::from_str(&id));
            if let Err(error) =
                callback.call2(&JsValue::NULL, &JsValue::from(index as u32), &module)
            {
                web_sys::console::error_2(&JsValue::from_str("kitchen: onSlotClick threw"), &error);
            }
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
    callback.forget();

    Some(observer)
}

/// Start the `requestAnimationFrame` loop, returning the cell that owns it.
fn spawn_frame_loop(state: &Rc<RefCell<State>>) -> FrameCallback {
    let callback: FrameCallback = Rc::new(RefCell::new(None));
    let scheduler = Rc::clone(&callback);
    let weak = Rc::downgrade(state);

    *callback.borrow_mut() = Some(Closure::wrap(Box::new(move |now_ms: f64| {
        let Some(state) = weak.upgrade() else {
            return;
        };

        {
            let mut state = state.borrow_mut();
            if !state.running {
                return;
            }
            if let Err(error) = state.tick(now_ms) {
                web_sys::console::error_1(&JsValue::from_str(&error));
                state.running = false;
                return;
            }
        }

        let handle = scheduler
            .borrow()
            .as_ref()
            .and_then(|callback| request_frame(callback).ok());
        state.borrow_mut().frame_handle = handle;
    }) as Box<dyn FnMut(f64)>));

    let handle = callback
        .borrow()
        .as_ref()
        .and_then(|callback| request_frame(callback).ok());
    state.borrow_mut().frame_handle = handle;

    callback
}

fn request_frame(callback: &Closure<dyn FnMut(f64)>) -> Result<i32, JsValue> {
    window().request_animation_frame(callback.as_ref().unchecked_ref())
}

/// The canvas' CSS size at the device pixel ratio, capped at 2x.
fn physical_size(canvas: &web_sys::HtmlCanvasElement) -> (u32, u32) {
    let ratio = window().device_pixel_ratio().clamp(1.0, 2.0);
    let width = (f64::from(canvas.client_width()) * ratio).round().max(1.0) as u32;
    let height = (f64::from(canvas.client_height()) * ratio).round().max(1.0) as u32;
    (width, height)
}

fn window() -> web_sys::Window {
    web_sys::window().expect("the constructor only runs in a browser, where `window` exists")
}
