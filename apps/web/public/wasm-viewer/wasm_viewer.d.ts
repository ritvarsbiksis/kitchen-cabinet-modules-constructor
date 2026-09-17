/* tslint:disable */
/* eslint-disable */

/**
 * A running viewer. Dropping the JS handle does not stop it - call `destroy()`,
 * which the React component does when the modal closes.
 */
export class Viewer {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Stop the frame loop, detach every listener and release the GPU resources.
     * Safe to call more than once.
     */
    destroy(): void;
    /**
     * Return the camera to the framing it started with.
     */
    resetView(): void;
    /**
     * Which wgpu backend the browser gave us: `WebGPU` or `WebGL2`.
     */
    readonly backend: string;
    /**
     * Triangles in the loaded model.
     */
    readonly triangleCount: number;
}

/**
 * Routes Rust panics to the browser console instead of an opaque trap.
 */
export function start(): void;

/**
 * Load `model_bytes` and start rendering them into `canvas`.
 *
 * `background_png` and `foreground_png` are the two skybox images. They are
 * what the model stands in and reflects, but they are not essential to seeing
 * it: empty or undecodable bytes leave the shader on its procedural gradient
 * and only cost a warning in the console.
 *
 * Resolves once the first frame is on screen, so the caller can keep a loading
 * state up until the model is actually visible.
 */
export function startViewer(canvas: HTMLCanvasElement, model_bytes: Uint8Array, background_png: Uint8Array, foreground_png: Uint8Array): Promise<Viewer>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_viewer_free: (a: number, b: number) => void;
    readonly start: () => void;
    readonly startViewer: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
    readonly viewer_backend: (a: number, b: number) => void;
    readonly viewer_destroy: (a: number) => void;
    readonly viewer_resetView: (a: number) => void;
    readonly viewer_triangleCount: (a: number) => number;
    readonly __wasm_bindgen_func_elem_10856: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_10866: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_10866_7: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1122: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1122_11: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1122_12: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_705: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_704: (a: number, b: number) => void;
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
