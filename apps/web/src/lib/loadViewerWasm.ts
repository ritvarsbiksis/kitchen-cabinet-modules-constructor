/**
 * Runtime loader for the wgpu viewer built from `crates/wasm-viewer`, and for
 * the glTF asset it renders.
 *
 * Same approach as `loadWasm.ts`: the module is fetched from `public/` at
 * runtime rather than bundled, so the `webpackIgnore` / `turbopackIgnore` magic
 * comments keep both Next.js bundlers away from the dynamic import. It is a
 * separate module from the Leptos one because it is roughly 2 MB of wgpu, which
 * should only be downloaded when someone actually opens the 3D view.
 */

/** A running viewer, as exported by `wasm-viewer`. */
export interface Viewer {
  /** The wgpu backend in use: `WebGPU` or `WebGL2`. */
  readonly backend: string;
  /** Triangles in the loaded model. */
  readonly triangleCount: number;
  /** Return the camera to its initial framing. */
  resetView: () => void;
  /** Stop the frame loop and release the GPU resources. Idempotent. */
  destroy: () => void;
}

/** The two PNGs that make up the skybox, as Rust takes them. */
export interface EnvironmentBytes {
  /** Equirectangular panorama of the room: the backdrop and the soft reflections. */
  background: Uint8Array;
  /** The room photograph, hung in front of the model as a studio light card. */
  foreground: Uint8Array;
}

/** The shape of the `wasm-pack --target web` output we rely on. */
export interface ViewerWasmModule {
  /** wasm-bindgen's generated `init`; resolves once the module is instantiated. */
  default: (options?: unknown) => Promise<unknown>;
  /** Renders `modelBytes` into `canvas`; resolves after the first frame. */
  startViewer: (
    canvas: HTMLCanvasElement,
    modelBytes: Uint8Array,
    background: Uint8Array,
    foreground: Uint8Array,
  ) => Promise<Viewer>;
}

/** Path the module is served from, relative to the site root. */
const WASM_MODULE_URL = '/wasm-viewer/wasm_viewer.js';

/** The glTF asset the page renders. */
export const MODEL_URL = '/models/SunglassesKhronos.glb';

/** The skybox images, in the order Rust expects them. */
export const ENVIRONMENT_URLS = {
  background: '/env/kitchen-background.png',
  foreground: '/env/kitchen-foreground.png',
} as const;

let modulePromise: Promise<ViewerWasmModule> | null = null;
let modelPromise: Promise<ArrayBuffer> | null = null;
const environmentPromises = new Map<string, Promise<EnvironmentBytes>>();

/**
 * Load and instantiate the viewer module, memoising the result so reopening the
 * modal reuses the instance instead of downloading the WASM again.
 */
export function loadViewerWasm(): Promise<ViewerWasmModule> {
  modulePromise ??= import(
    /* webpackIgnore: true */ /* turbopackIgnore: true */ WASM_MODULE_URL
  ).then(async (mod: ViewerWasmModule) => {
    await mod.default();
    return mod;
  });

  return modulePromise;
}

/**
 * Fetch the `.glb`, memoised the same way.
 *
 * Returns a fresh `Uint8Array` view on every call: Rust takes ownership of the
 * bytes it is handed, so the cached buffer is the thing worth keeping.
 */
export async function loadModel(url: string = MODEL_URL): Promise<Uint8Array> {
  modelPromise ??= fetch(url).then(async (response) => {
    if (!response.ok) {
      throw new Error(`could not load ${url} (HTTP ${response.status})`);
    }
    return response.arrayBuffer();
  });

  try {
    return new Uint8Array(await modelPromise);
  } catch (cause) {
    // A failed fetch should not poison every later attempt.
    modelPromise = null;
    throw cause;
  }
}

/**
 * Fetch the two skybox images, memoised per pair of URLs - the viewer and the
 * kitchen constructor each reflect their own room.
 *
 * Best effort on purpose: the viewer treats empty bytes as "no skybox" and falls
 * back to a procedural gradient, so a missing or broken image costs the room
 * behind the model rather than the model itself.
 */
export async function loadEnvironment(
  urls: { background: string; foreground: string } = ENVIRONMENT_URLS,
): Promise<EnvironmentBytes> {
  const key = `${urls.background}\n${urls.foreground}`;
  let promise = environmentPromises.get(key);
  if (!promise) {
    promise = Promise.all([fetchImage(urls.background), fetchImage(urls.foreground)]).then(
      ([background, foreground]) => ({ background, foreground }),
    );
    environmentPromises.set(key, promise);
  }

  return promise;
}

/** Fetch one image as bytes, warning rather than throwing when it is missing. */
async function fetchImage(url: string): Promise<Uint8Array> {
  try {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return new Uint8Array(await response.arrayBuffer());
  } catch (cause) {
    console.warn(`could not load the skybox image ${url}, rendering without it`, cause);
    return new Uint8Array();
  }
}

/** Drop the cached module, model and skybox. Only used by tests. */
export function resetViewerCache(): void {
  modulePromise = null;
  modelPromise = null;
  environmentPromises.clear();
}
