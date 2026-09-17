import { afterEach, describe, expect, it, vi } from 'vitest';
import { loadEnvironment, loadModel, loadViewerWasm, resetViewerCache } from '@/lib/loadViewerWasm';

describe('loadViewerWasm', () => {
  afterEach(() => {
    resetViewerCache();
    vi.unstubAllGlobals();
  });

  it('memoises the module promise so the 2 MB bundle is fetched once', () => {
    // jsdom cannot resolve `/wasm-viewer/wasm_viewer.js`, so both calls reject -
    // what matters is that they are the same promise instance, i.e. it is cached.
    const first = loadViewerWasm();
    const second = loadViewerWasm();

    expect(first).toBe(second);

    return Promise.allSettled([first, second]);
  });

  it('returns a fresh promise after the cache is reset', async () => {
    const first = loadViewerWasm();
    resetViewerCache();
    const second = loadViewerWasm();

    expect(first).not.toBe(second);
    await Promise.allSettled([first, second]);
  });
});

describe('loadModel', () => {
  afterEach(() => {
    resetViewerCache();
    vi.unstubAllGlobals();
  });

  it('fetches the model once and hands out a fresh view of it each time', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer,
    });
    vi.stubGlobal('fetch', fetchMock);

    const first = await loadModel();
    const second = await loadModel();

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(Array.from(first)).toEqual([1, 2, 3]);
    // A separate view, because Rust takes ownership of the bytes it is given.
    expect(second).not.toBe(first);
  });

  it('rejects with the status when the asset is missing, and retries next time', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, status: 404 })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        arrayBuffer: async () => new Uint8Array([7]).buffer,
      });
    vi.stubGlobal('fetch', fetchMock);

    await expect(loadModel('/models/missing.glb')).rejects.toThrow('HTTP 404');

    // The failure must not be cached, or the viewer could never recover.
    expect(Array.from(await loadModel('/models/missing.glb'))).toEqual([7]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});

describe('loadEnvironment', () => {
  afterEach(() => {
    resetViewerCache();
    vi.unstubAllGlobals();
  });

  it('fetches both skybox images once and caches them', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      arrayBuffer: async () => new Uint8Array([0x89, 0x50, 0x4e, 0x47]).buffer,
    });
    vi.stubGlobal('fetch', fetchMock);

    const first = await loadEnvironment();
    await loadEnvironment();

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/env/kitchen-background.png',
      '/env/kitchen-foreground.png',
    ]);
    expect(Array.from(first.background)).toEqual([0x89, 0x50, 0x4e, 0x47]);
  });

  it('caches each pair of images on its own, so another room is not served the first one', async () => {
    const fetchMock = vi.fn().mockImplementation(async (url: string) => ({
      ok: true,
      status: 200,
      arrayBuffer: async () => Uint8Array.from(url, (character) => character.charCodeAt(0)).buffer,
    }));
    vi.stubGlobal('fetch', fetchMock);

    await loadEnvironment();
    const other = await loadEnvironment({ background: '/env/b.png', foreground: '/env/f.png' });
    await loadEnvironment({ background: '/env/b.png', foreground: '/env/f.png' });

    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(String.fromCharCode(...other.background)).toBe('/env/b.png');
    expect(String.fromCharCode(...other.foreground)).toBe('/env/f.png');
  });

  it('hands back empty bytes for an image that is missing, instead of throwing', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, status: 404 })
      .mockRejectedValueOnce(new Error('offline'));
    vi.stubGlobal('fetch', fetchMock);

    const environment = await loadEnvironment();

    // Empty is the signal Rust takes as "render without a skybox".
    expect(environment.background).toHaveLength(0);
    expect(environment.foreground).toHaveLength(0);
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });
});
