/**
 * Runtime loader for the kitchen constructor built from `crates/wasm-kitchen`,
 * and for the `.glb` files it places.
 *
 * Same approach as `loadViewerWasm.ts`: the module is fetched from `public/` at
 * runtime rather than bundled, and only once someone has entered their wall
 * size. The skybox the metal fronts reflect is fetched with the viewer's
 * `loadEnvironment`, pointed at `KITCHEN_ENVIRONMENT_URLS`.
 */

/** A running constructor, as exported by `wasm-kitchen`. */
export interface KitchenConstructor {
  /** The wgpu backend in use: `WebGPU` or `WebGL2`. */
  readonly backend: string;
  /** How many slots fit along the wall. */
  readonly slotCount: number;
  /**
   * Put a module in a slot. The bytes are only parsed the first time an id is
   * seen. Throws when the slot does not exist or the model will not load.
   */
  placeModule: (slot: number, moduleId: string, glb: Uint8Array) => void;
  /** Put the placeholder back in a slot. */
  clearSlot: (slot: number) => void;
  /** The id of the module in a slot, or `undefined` for the placeholder. */
  slotModule: (slot: number) => string | undefined;
  /** Return the camera to its initial framing. */
  resetView: () => void;
  /** Stop the frame loop and release the GPU resources. Idempotent. */
  destroy: () => void;
}

/** Called by Rust when a slot is clicked, with the module in it or `null`. */
export type SlotClickHandler = (slot: number, moduleId: string | null) => void;

/** The shape of the `wasm-pack --target web` output we rely on. */
export interface KitchenWasmModule {
  /** wasm-bindgen's generated `init`; resolves once the module is instantiated. */
  default: (options?: unknown) => Promise<unknown>;
  /** Builds the room in `canvas`; resolves after the first frame. */
  startKitchen: (
    canvas: HTMLCanvasElement,
    wallWidthCm: number,
    wallHeightCm: number,
    placeholderGlb: Uint8Array,
    background: Uint8Array,
    foreground: Uint8Array,
    onSlotClick: SlotClickHandler,
  ) => Promise<KitchenConstructor>;
}

/** Path the module is served from, relative to the site root. */
const WASM_MODULE_URL = '/wasm-kitchen/wasm_kitchen.js';

let modulePromise: Promise<KitchenWasmModule> | null = null;
const glbPromises = new Map<string, Promise<ArrayBuffer>>();

/**
 * Load and instantiate the constructor module, memoising the result so starting
 * again reuses the instance instead of downloading the WASM again.
 */
export function loadKitchenWasm(): Promise<KitchenWasmModule> {
  modulePromise ??= import(
    /* webpackIgnore: true */ /* turbopackIgnore: true */ WASM_MODULE_URL
  ).then(async (mod: KitchenWasmModule) => {
    await mod.default();
    return mod;
  });

  return modulePromise;
}

/**
 * Fetch a `.glb`, memoised per URL.
 *
 * Returns a fresh `Uint8Array` view on every call: Rust takes ownership of the
 * bytes it is handed, so the cached buffer is the thing worth keeping.
 */
export async function loadGlb(url: string): Promise<Uint8Array> {
  let promise = glbPromises.get(url);
  if (!promise) {
    promise = fetch(url).then(async (response) => {
      if (!response.ok) {
        throw new Error(`could not load ${url} (HTTP ${response.status})`);
      }
      return response.arrayBuffer();
    });
    glbPromises.set(url, promise);
  }

  try {
    return new Uint8Array(await promise);
  } catch (cause) {
    // A failed fetch should not poison every later attempt.
    glbPromises.delete(url);
    throw cause;
  }
}

/** Drop the cached module and models. Only used by tests. */
export function resetKitchenCache(): void {
  modulePromise = null;
  glbPromises.clear();
}
