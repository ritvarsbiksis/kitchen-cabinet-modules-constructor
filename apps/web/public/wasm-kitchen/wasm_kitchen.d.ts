/* tslint:disable */
/* eslint-disable */

/**
 * A running constructor. Dropping the JS handle does not stop it - call
 * `destroy()`, which the React component does when it unmounts.
 */
export class KitchenConstructor {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Take the module out of `slot`, leaving the placeholder.
     */
    clearSlot(slot: number): void;
    /**
     * Stop the frame loop, detach every listener and release the GPU
     * resources. Safe to call more than once.
     */
    destroy(): void;
    /**
     * Put the module `module_id` in `slot`, replacing whatever is there.
     *
     * `glb` is only parsed the first time an id is seen; after that the
     * uploaded model is reused and the bytes are ignored, so the caller does
     * not need to track what Rust already has.
     */
    placeModule(slot: number, module_id: string, glb: Uint8Array): void;
    /**
     * Return the camera to the framing it started with.
     */
    resetView(): void;
    /**
     * The id of the module in `slot`, or `undefined` for the placeholder.
     */
    slotModule(slot: number): string | undefined;
    /**
     * Which wgpu backend the browser gave us: `WebGPU` or `WebGL2`.
     */
    readonly backend: string;
    /**
     * How many slots fit along the wall.
     */
    readonly slotCount: number;
}

/**
 * Routes Rust panics to the browser console instead of an opaque trap.
 */
export function start(): void;

/**
 * Build the room for a `wall_width_cm` x `wall_height_cm` wall in `canvas` and
 * line the wall with placeholders.
 *
 * `background_png` and `foreground_png` are the skybox the metal fronts
 * reflect; empty bytes fall back to a gradient. `on_slot_click` is called with
 * the slot index and the id of the module in it (or `null`) whenever a slot is
 * clicked or tapped.
 *
 * Resolves once the first frame is on screen.
 */
export function startKitchen(canvas: HTMLCanvasElement, wall_width_cm: number, wall_height_cm: number, placeholder_glb: Uint8Array, background_png: Uint8Array, foreground_png: Uint8Array, on_slot_click: Function): Promise<KitchenConstructor>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_kitchenconstructor_free: (a: number, b: number) => void;
    readonly kitchenconstructor_backend: (a: number, b: number) => void;
    readonly kitchenconstructor_clearSlot: (a: number, b: number, c: number) => void;
    readonly kitchenconstructor_destroy: (a: number) => void;
    readonly kitchenconstructor_placeModule: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly kitchenconstructor_resetView: (a: number) => void;
    readonly kitchenconstructor_slotCount: (a: number) => number;
    readonly kitchenconstructor_slotModule: (a: number, b: number, c: number) => void;
    readonly start: () => void;
    readonly startKitchen: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => number;
    readonly __wasm_bindgen_func_elem_698: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_11036: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_11046: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_11046_10: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1285: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1285_15: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1285_16: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_697: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export5: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
