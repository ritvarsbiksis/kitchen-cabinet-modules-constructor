# rust-wasm-example

A monorepo scaffold pairing a **Next.js + TypeScript** app with Rust compiled to WebAssembly.
The web app has four routes:

- **WASM example** — a **Run WASM** button that calls into Rust, which mounts a Leptos view
  rendering `Hello World!` into a `<div>`.
- **WGPU example** — a **View 3D object** button that opens a modal where Rust renders a glTF
  model with [`wgpu`](https://wgpu.rs), and you can orbit and zoom it.
- **Kitchen constructor** — enter a wall size, then plan a run of kitchen base units along it in
  an interactive 3D room rendered with `wgpu`.

## Layout

```
.
├── apps/web/            Next.js app (App Router, TypeScript, Mantine, CSS Modules)
├── crates/wasm-hello/   Leptos component compiled to WASM with wasm-pack
├── crates/wasm-viewer/  glTF viewer rendered with wgpu, compiled to WASM
├── crates/wasm-kitchen/ Kitchen constructor rendered with wgpu, compiled to WASM
├── crates/scene-assets/ glTF and skybox decoding shared by both wgpu crates (host target)
├── Cargo.toml           Cargo workspace
├── package.json         npm workspaces root
└── turbo.json           Turborepo task graph (build:wasm runs before dev, not build)
```

## Prerequisites

| Tool      | Version | Install                                                                   |
| --------- | ------- | ------------------------------------------------------------------------- |
| Node.js   | >= 20   | https://nodejs.org                                                        |
| Rust      | stable  | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh`         |
| wasm32    | —       | `rustup target add wasm32-unknown-unknown`                                |
| wasm-pack | >= 0.13 | `cargo install wasm-pack` (or grab a release binary, which is far faster) |

`rust-toolchain.toml` pins the channel and target, so `rustup` provisions them automatically once
it is installed.

## Getting started

```bash
npm install
npm run dev
```

`npm run dev` goes through Turborepo, which runs `build:wasm` first — so the WASM artifacts exist
before Next.js starts. The app is served at http://localhost:3000.

## Commands

| Command              | What it does                                        |
| -------------------- | --------------------------------------------------- |
| `npm run dev`        | Build the WASM, then start the Next.js dev server   |
| `npm run build`      | Production Next.js build from the committed WASM    |
| `npm start`          | Serve the production build                          |
| `npm run build:wasm` | Build the three WASM crates into `apps/web/public/` |
| `npm test`           | Run the Vitest unit tests                           |
| `npm run test:rust`  | Run the Rust unit tests (`cargo test --workspace`)  |
| `npm run lint`       | ESLint over the web app                             |
| `npm run typecheck`  | `tsc --noEmit`                                      |
| `npm run format`     | Prettier write across the repo                      |

## How the WASM integration works

1. **Build.** `wasm-pack build crates/wasm-hello --target web` emits an ES module plus a `.wasm`
   binary into `apps/web/public/wasm/`. The output of all three crates is committed, so
   `npm run build` (and Vercel) never needs a Rust toolchain. After changing a crate, run
   `npm run build:wasm` and commit the regenerated files.

2. **Load.** [`apps/web/src/lib/loadWasm.ts`](apps/web/src/lib/loadWasm.ts) imports the glue module
   with `webpackIgnore` / `turbopackIgnore` magic comments, so neither Next.js bundler touches it.
   The browser fetches `/wasm/wasm_hello.js` at runtime, and the glue resolves the `.wasm` relative
   to its own URL. No `next.config` WASM experiments, no loader coupling. The resulting promise is
   memoised so repeat clicks reuse the same instance.

3. **Call.** [`WasmRunner.tsx`](apps/web/src/components/WasmRunner.tsx) is a client component with
   the **Run WASM** button and an empty `<div id="wasm-target">`. On click it awaits the module and
   calls `run_wasm('wasm-target')`.

4. **Render.** [`crates/wasm-hello/src/lib.rs`](crates/wasm-hello/src/lib.rs) looks the element up,
   clears it (so repeat clicks stay idempotent), and mounts a Leptos component with
   `leptos::mount::mount_to`. `mount_to` returns an `UnmountHandle` that unmounts on drop, so it is
   `.forget()`-ed to keep the view on screen.

React never renders children into `#wasm-target`, so React and Leptos never contend over the same
DOM subtree.

## How the wgpu viewer works

The 3D route is a second, independent WASM module — roughly 2 MB of `wgpu`, so it is only fetched
when someone opens the modal.

1. **Load.** [`loadViewerWasm.ts`](apps/web/src/lib/loadViewerWasm.ts) fetches the module, the
   `.glb` asset and the two skybox images, memoising all of them.
   [`ObjectViewer.tsx`](apps/web/src/components/ObjectViewer.tsx) renders the button, the modal and
   the `<canvas>`, then calls `startViewer(canvas, model, background, foreground)`. The skybox is
   best effort: an image that will not load arrives as empty bytes and the viewer renders on a
   built-in gradient instead of failing.

2. **Parse.** [`model.rs`](crates/scene-assets/src/model.rs) reads the binary glTF with the `gltf`
   crate, flattens the node hierarchy, bakes each node's transform into the vertices, decodes the
   embedded PNG, and recentres and rescales the model into a unit sphere — so the camera works the
   same way for any asset.

3. **Light.** [`environment.rs`](crates/scene-assets/src/environment.rs) decodes the skybox and
   builds a mip chain for each image on the CPU, averaging in linear light. The two images play
   different parts: the **background** is a blurred equirectangular panorama of a room, drawn as
   the backdrop and reflected as the soft half of the surroundings; the **foreground** is the room
   photograph itself, hung in front of the model like a studio light card, which is what puts
   readable highlights on the polished metal. Surface roughness picks the mip level, so a reflection
   goes from sharp to blurred with the material.

4. **Render.** [`renderer.rs`](crates/wasm-viewer/src/renderer.rs) creates a `wgpu` surface on the
   canvas, requests an adapter with `Backends::BROWSER_WEBGPU | Backends::GL`, and asks for limits
   within `downlevel_webgl2_defaults()` so the same code runs on **WebGPU** or falls back to
   **WebGL2**. [`shader.wgsl`](crates/wasm-viewer/src/shader.wgsl) draws the backdrop as one
   full-screen triangle, then metallic-roughness PBR lit by the skybox and a three-light rig;
   transmissive materials go through a second, blended pipeline drawn back-to-front.

5. **Interact.** [`viewer.rs`](crates/wasm-viewer/src/viewer.rs) registers its own pointer, wheel
   and `ResizeObserver` listeners, runs a `requestAnimationFrame` loop, and returns a handle whose
   `destroy()` cancels the loop, removes the listeners and drops the GPU resources. The React
   component calls it when the modal closes, including on StrictMode's double mount.

Two details worth knowing if you adapt this:

- **Gamma.** WebGPU hands out a plain UNORM surface here and WebGL2 an sRGB one. The shader encodes
  gamma itself only in the first case, and `clear_color` converts the background to match — without
  that, the two backends show noticeably different backgrounds.
- **Aspect.** A perspective projection only fits the _vertical_ field of view, so `OrbitCamera`
  backs off by the aspect ratio on portrait viewports; otherwise a phone crops the model's sides.
- **Skybox assets.** `apps/web/public/env/` holds the two PNGs, both derived from one interior
  photograph: the foreground is the photo itself, the background is that photo mirrored beside
  itself into a seamless 512×256 panorama, blurred and faded into ceiling and floor tones. They are
  sRGB 8-bit, so `expand_range` in the shader fakes the dynamic range a photograph does not have -
  without it a reflected window is just light grey rather than a highlight.
- **Kitchen skybox.** The constructor reflects a different room, `living-room-*.png`, derived from
  one living-room photograph. The foreground is the photo at 1024×683; the background is a
  2048×1024 panorama with that photo projected onto four cards tilted 35° down - in front of the
  run, behind it, and mirrored on the two sides so every seam meets itself - fading into ceiling and
  floor tones above and below. The foreground card in `kitchen.wgsl` uses the same direction and
  size, so the sharp photo and the panorama line up where the fronts reflect them.

## How the kitchen constructor works

The `/kitchen-constructor` route is a third WASM module, `wasm-kitchen`, fetched only once the
user has entered their wall.

1. **Wall size.** [`KitchenConstructor.tsx`](apps/web/src/components/KitchenConstructor.tsx) shows a
   **Start** button that opens [`DimensionsModal.tsx`](apps/web/src/components/DimensionsModal.tsx):
   width 200–500 cm and height 250–300 cm. On **Continue** the button is replaced by a stage that
   spans the whole viewport width (the page marks itself `data-full-width`, which lifts the root
   layout's column limit in `layout.module.css`).

2. **Start.** [`loadKitchenWasm.ts`](apps/web/src/lib/loadKitchenWasm.ts) loads the module and the
   placeholder `.glb` (memoised per URL), the skybox - the kitchen's own pair,
   `KITCHEN_ENVIRONMENT_URLS` in [`kitchenCatalog.ts`](apps/web/src/lib/kitchenCatalog.ts) - comes
   through the viewer's loader, which memoises each pair of URLs separately, and
   `startKitchen(canvas, widthCm, heightCm, placeholder, background, foreground, onSlotClick)`
   builds the room. Rust validates the wall size again.

3. **Layout.** [`layout.rs`](crates/wasm-kitchen/src/layout.rs) puts a 6 × 6 m floor at the origin,
   a 20 cm thick wall along its back edge and three pendant lamps in a row over the middle. Slots
   are 80 cm wide, so a wall holds `floor(width / 80)` of them — integer centimetres, since
   `4.0 / 0.8` in floating point would lose a module on a 4 m wall — centred along the wall. A model
   is stood in its slot by its bounding box, so an asset exported slightly off-centre still lines up.

4. **Render.** [`renderer.rs`](crates/wasm-kitchen/src/renderer.rs) uploads each model once and
   draws it at any number of placements, each with a small uniform for its translation and hover
   state. [`kitchen.wgsl`](crates/wasm-kitchen/src/kitchen.wgsl) lights everything with the three
   lamps as point lights, a room ambient and reflections of a living room. Polished metal mirrors
   that photograph almost entirely and nearly as sharply as a mirror - window, blinds, dark walls
   and sofa - while satin aluminium and painted surfaces blur it and blend it with a neutral room.
   The floor tiles
   are generated in [`floor.rs`](crates/wasm-kitchen/src/floor.rs) with a mip chain; the lamps and
   wall are built in [`geometry.rs`](crates/wasm-kitchen/src/geometry.rs). Soft contact shadows stand
   the run of modules on the floor.

5. **Interact.** [`constructor.rs`](crates/wasm-kitchen/src/constructor.rs) handles pointer, pinch
   and wheel events like the viewer, within limits in [`camera.rs`](crates/wasm-kitchen/src/camera.rs)
   that keep the camera in front of the wall and above the floor. Hovering casts a ray through the
   pointer against the slot boxes ([`picking.rs`](crates/wasm-kitchen/src/picking.rs)); the hit slot
   eases into an LED glow — lit edges, glowing glass and light spilling onto the floor and wall. A
   press that does not move more than a few pixels is a click and calls `onSlotClick(slot, moduleId)`
   once the Rust state is no longer borrowed.

6. **Choose.** [`ModulePickerModal.tsx`](apps/web/src/components/ModulePickerModal.tsx) lists the
   modules from [`kitchenCatalog.ts`](apps/web/src/lib/kitchenCatalog.ts). Picking one fetches its
   `.glb` and calls `placeModule(slot, id, bytes)`; Rust parses and uploads a module the first time
   its id is seen and reuses it after that. A slot that already holds a module can be switched to
   the other one or cleared back to the placeholder with `clearSlot(slot)`.

To add a module, export an 80 × 87 × 58 cm `.glb` (Y up, front facing +Z) into
`apps/web/public/models/` and add an entry to `KITCHEN_MODULES`.

### wasm-opt note

`crates/wasm-hello/Cargo.toml` passes explicit feature flags to `wasm-opt`
(`--enable-bulk-memory` and friends). The `wasm-opt` bundled with wasm-pack predates WASM features
that current rustc emits by default, and validation fails without them. Set `wasm-opt = false`
under `[package.metadata.wasm-pack.profile.release]` to skip the optimiser entirely.

## Deploying to Vercel

Import the repository and set the project's **Root Directory** to `apps/web`. Vercel detects
Turborepo and Next.js and runs `turbo run build`, which is just `next build` — the WASM in
`apps/web/public/` is served from the committed files, since Vercel's build image has no Rust.

## Testing

- **TypeScript** — Vitest + Testing Library in jsdom. `apps/web/src/__tests__/` covers the
  button-to-`run_wasm` call path (the loader is mocked at the module boundary), the viewer's modal
  lifecycle down to `destroy()` on close, the kitchen constructor's wall size validation, Start →
  stage flow, module picking and removal, error surfacing, nav links, and loader memoisation.
  `src/test-utils/render.tsx` wraps renders in `MantineProvider`.
- **Rust** — `cargo test --workspace` runs host-target unit tests. The glTF parsing and skybox
  decoding (`scene-assets`), the viewer's camera and the constructor's layout, camera, picking,
  procedural meshes and floor texture are target independent and tested on the host — including
  checks that the bundled kitchen `.glb` files are the size of one slot. The GPU and DOM code is
  `cfg(target_arch = "wasm32")` and exercised in a browser instead.

## Styling

Mantine provides the components (`Button`, `Alert`, `Paper`, `Group`, …), configured in
`apps/web/src/app/layout.tsx` with `MantineProvider`, `ColorSchemeScript`, and `mantineHtmlProps`.
Custom styling is CSS Modules (`*.module.css`) using Mantine's CSS variables and `light-dark()`, so
both colour schemes work. `postcss-preset-mantine` supplies `light-dark()` and the breakpoint vars.
