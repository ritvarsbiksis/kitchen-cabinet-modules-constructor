import { describe, expect, it } from 'vitest';
import { loadWasm, resetWasmCache } from '@/lib/loadWasm';

describe('loadWasm', () => {
  it('memoises the module promise so the WASM is instantiated once', () => {
    resetWasmCache();

    // jsdom cannot resolve `/wasm/wasm_hello.js`, so both calls reject - what
    // matters is that they are the same promise instance, i.e. it is cached.
    const first = loadWasm();
    const second = loadWasm();

    expect(first).toBe(second);

    // Keep the rejections handled so they do not surface as unhandled.
    return Promise.allSettled([first, second]);
  });

  it('returns a fresh promise after the cache is reset', async () => {
    const first = loadWasm();
    resetWasmCache();
    const second = loadWasm();

    expect(first).not.toBe(second);
    await Promise.allSettled([first, second]);
  });
});
