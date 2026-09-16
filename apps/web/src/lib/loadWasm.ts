/**
 * Runtime loader for the WASM module built from `crates/wasm-hello`.
 *
 * The module is fetched straight from `public/wasm/` at runtime rather than
 * being bundled: the `webpackIgnore` / `turbopackIgnore` magic comments tell
 * both Next.js bundlers to leave the dynamic import untouched, so the browser
 * resolves `/wasm/wasm_hello.js` itself. That JS glue in turn locates
 * `wasm_hello_bg.wasm` relative to its own URL, so nothing needs a loader or a
 * `next.config` WASM experiment.
 */

/** The shape of the `wasm-pack --target web` output we rely on. */
export interface WasmHelloModule {
  /** wasm-bindgen's generated `init`; resolves once the module is instantiated. */
  default: (options?: unknown) => Promise<unknown>;
  /** Mounts the Leptos "Hello World!" view into the element with this id. */
  run_wasm: (targetId: string) => void;
}

/** Path the module is served from, relative to the site root. */
const WASM_MODULE_URL = '/wasm/wasm_hello.js';

let modulePromise: Promise<WasmHelloModule> | null = null;

/**
 * Load and instantiate the WASM module, memoising the result so repeat calls
 * reuse the same instance instead of re-downloading and re-instantiating it.
 */
export function loadWasm(): Promise<WasmHelloModule> {
  modulePromise ??= import(
    /* webpackIgnore: true */ /* turbopackIgnore: true */ WASM_MODULE_URL
  ).then(async (mod: WasmHelloModule) => {
    await mod.default();
    return mod;
  });

  return modulePromise;
}

/** Drop the cached instance. Only used by tests. */
export function resetWasmCache(): void {
  modulePromise = null;
}
