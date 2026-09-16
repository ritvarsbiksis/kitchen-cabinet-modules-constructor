import { afterEach, describe, expect, it, vi } from 'vitest';
import { loadGlb, loadKitchenWasm, resetKitchenCache } from '@/lib/loadKitchenWasm';

function okResponse(bytes: number[]) {
  return {
    ok: true,
    status: 200,
    arrayBuffer: async () => new Uint8Array(bytes).buffer,
  };
}

describe('loadKitchenWasm', () => {
  afterEach(() => {
    resetKitchenCache();
  });

  it('memoises the module promise so the bundle is fetched once', () => {
    // jsdom cannot resolve `/wasm-kitchen/wasm_kitchen.js`, so both calls
    // reject - what matters is that they are the same promise instance.
    const first = loadKitchenWasm();
    const second = loadKitchenWasm();

    expect(first).toBe(second);

    return Promise.allSettled([first, second]);
  });
});

describe('loadGlb', () => {
  afterEach(() => {
    resetKitchenCache();
    vi.unstubAllGlobals();
  });

  it('fetches each URL once and hands out a fresh view every time', async () => {
    const fetchMock = vi.fn(async (url: string) =>
      okResponse(url.includes('aluminium') ? [2] : [1]),
    );
    vi.stubGlobal('fetch', fetchMock);

    const steel = await loadGlb('/models/kitchen-module-1.glb');
    const steelAgain = await loadGlb('/models/kitchen-module-1.glb');
    const aluminium = await loadGlb('/models/kitchen-module-1-aluminium.glb');

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(Array.from(steel)).toEqual([1]);
    expect(Array.from(aluminium)).toEqual([2]);
    // A separate view, because Rust takes ownership of the bytes it is given.
    expect(steelAgain).not.toBe(steel);
  });

  it('rejects with the status when a model is missing, and retries next time', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, status: 404 })
      .mockResolvedValueOnce(okResponse([7]));
    vi.stubGlobal('fetch', fetchMock);

    await expect(loadGlb('/models/missing.glb')).rejects.toThrow('HTTP 404');
    expect(Array.from(await loadGlb('/models/missing.glb'))).toEqual([7]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
