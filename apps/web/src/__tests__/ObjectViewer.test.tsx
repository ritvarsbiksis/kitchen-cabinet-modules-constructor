import { describe, expect, it, vi, beforeEach } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, waitFor } from '@/test-utils/render';
import { ObjectViewer, VIEWER_CANVAS_ID } from '@/components/ObjectViewer';
import { loadEnvironment, loadModel, loadViewerWasm, type Viewer } from '@/lib/loadViewerWasm';

// jsdom has neither a GPU nor `fetch` for a 370 KB binary, so the module
// boundary is where this is stubbed - exactly as in `WasmRunner.test.tsx`.
vi.mock('@/lib/loadViewerWasm', () => ({
  loadViewerWasm: vi.fn(),
  loadModel: vi.fn(),
  loadEnvironment: vi.fn(),
  MODEL_URL: '/models/SunglassesKhronos.glb',
}));

const loadViewerWasmMock = vi.mocked(loadViewerWasm);
const loadModelMock = vi.mocked(loadModel);
const loadEnvironmentMock = vi.mocked(loadEnvironment);

/** A stand-in for the `Viewer` handle Rust hands back. */
function fakeViewer(overrides: Partial<Viewer> = {}) {
  return {
    backend: 'WebGPU',
    triangleCount: 12_345,
    resetView: vi.fn(),
    destroy: vi.fn(),
    ...overrides,
  } satisfies Viewer;
}

/** Wire the mocked module up to return `viewer` from `startViewer`. */
function mockModule(viewer: Viewer) {
  const startViewer = vi.fn().mockResolvedValue(viewer);
  loadViewerWasmMock.mockResolvedValue({
    default: vi.fn().mockResolvedValue({}),
    startViewer,
  });
  loadModelMock.mockResolvedValue(new Uint8Array([0x67, 0x6c, 0x54, 0x46]));
  loadEnvironmentMock.mockResolvedValue({
    background: new Uint8Array([0x89, 0x50]),
    foreground: new Uint8Array([0x89, 0x4e]),
  });
  return startViewer;
}

/** Open the modal and wait for the viewer to report itself as running. */
async function openViewer() {
  await userEvent.click(screen.getByRole('button', { name: 'View 3D object' }));
  return screen.findByTestId('viewer-info');
}

describe('ObjectViewer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows the button without loading any WASM up front', () => {
    mockModule(fakeViewer());
    render(<ObjectViewer />);

    expect(screen.getByRole('button', { name: 'View 3D object' })).toBeInTheDocument();
    expect(screen.queryByTestId('wgpu-canvas')).not.toBeInTheDocument();
    expect(loadViewerWasmMock).not.toHaveBeenCalled();
  });

  it('opens a modal with a canvas and hands it to the Rust entry point', async () => {
    const viewer = fakeViewer();
    const startViewer = mockModule(viewer);
    render(<ObjectViewer />);

    await openViewer();

    const canvas = screen.getByTestId('wgpu-canvas');
    expect(canvas).toHaveAttribute('id', VIEWER_CANVAS_ID);
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(startViewer).toHaveBeenCalledWith(
      canvas,
      expect.any(Uint8Array),
      expect.any(Uint8Array),
      expect.any(Uint8Array),
    );
  });

  it('starts anyway when the skybox images could not be fetched', async () => {
    const startViewer = mockModule(fakeViewer());
    // `loadEnvironment` resolves to empty bytes rather than rejecting; Rust
    // reads that as "no skybox" and renders the model on its gradient.
    loadEnvironmentMock.mockResolvedValue({
      background: new Uint8Array(),
      foreground: new Uint8Array(),
    });
    render(<ObjectViewer />);

    await openViewer();

    expect(startViewer).toHaveBeenCalledWith(
      expect.anything(),
      expect.any(Uint8Array),
      new Uint8Array(),
      new Uint8Array(),
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('reports the backend and triangle count the viewer came back with', async () => {
    mockModule(fakeViewer({ backend: 'WebGL2', triangleCount: 9_000 }));
    render(<ObjectViewer />);

    expect(await openViewer()).toHaveTextContent('WebGL2 · 9,000 triangles');
  });

  it('forwards the reset button to the viewer', async () => {
    const viewer = fakeViewer();
    mockModule(viewer);
    render(<ObjectViewer />);
    await openViewer();

    await userEvent.click(screen.getByRole('button', { name: 'Reset view' }));

    expect(viewer.resetView).toHaveBeenCalledTimes(1);
  });

  it('destroys the viewer when the modal is closed, so the GPU work stops', async () => {
    const viewer = fakeViewer();
    mockModule(viewer);
    render(<ObjectViewer />);
    await openViewer();

    await userEvent.click(screen.getByRole('button', { name: 'Close the 3D viewer' }));

    await waitFor(() => expect(viewer.destroy).toHaveBeenCalled());
  });

  it('surfaces an error when the viewer cannot start', async () => {
    loadViewerWasmMock.mockResolvedValue({
      default: vi.fn().mockResolvedValue({}),
      startViewer: vi.fn().mockRejectedValue(new Error('no GPU adapter is available')),
    });
    loadModelMock.mockResolvedValue(new Uint8Array());
    loadEnvironmentMock.mockResolvedValue({
      background: new Uint8Array(),
      foreground: new Uint8Array(),
    });
    render(<ObjectViewer />);

    await userEvent.click(screen.getByRole('button', { name: 'View 3D object' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('no GPU adapter is available');
    expect(screen.getByRole('button', { name: 'Reset view' })).toBeDisabled();
  });

  it('surfaces an error when the model cannot be fetched', async () => {
    mockModule(fakeViewer());
    loadModelMock.mockRejectedValue(new Error('could not load /models/SunglassesKhronos.glb'));
    render(<ObjectViewer />);

    await userEvent.click(screen.getByRole('button', { name: 'View 3D object' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'could not load /models/SunglassesKhronos.glb',
    );
  });
});
