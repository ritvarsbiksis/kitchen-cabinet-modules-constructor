import { beforeEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { act, render, screen, waitFor, within } from '@/test-utils/render';
import { KITCHEN_CANVAS_ID, KitchenConstructor } from '@/components/KitchenConstructor';
import {
  loadGlb,
  loadKitchenWasm,
  type KitchenConstructor as Kitchen,
  type SlotClickHandler,
} from '@/lib/loadKitchenWasm';
import { loadEnvironment } from '@/lib/loadViewerWasm';

// jsdom has no GPU, so the module boundary is stubbed - as in
// `ObjectViewer.test.tsx`.
vi.mock('@/lib/loadKitchenWasm', () => ({
  loadKitchenWasm: vi.fn(),
  loadGlb: vi.fn(),
}));
vi.mock('@/lib/loadViewerWasm', () => ({
  loadEnvironment: vi.fn(),
}));

const loadKitchenWasmMock = vi.mocked(loadKitchenWasm);
const loadGlbMock = vi.mocked(loadGlb);
const loadEnvironmentMock = vi.mocked(loadEnvironment);

/** A stand-in for the handle Rust hands back, tracking what is in each slot. */
function fakeKitchen(slotCount = 4) {
  const slots = new Map<number, string>();
  return {
    backend: 'WebGPU',
    slotCount,
    placeModule: vi.fn((slot: number, moduleId: string) => {
      slots.set(slot, moduleId);
    }),
    clearSlot: vi.fn((slot: number) => {
      slots.delete(slot);
    }),
    slotModule: vi.fn((slot: number) => slots.get(slot)),
    resetView: vi.fn(),
    destroy: vi.fn(),
  } satisfies Kitchen;
}

/** Stand-in bytes for a model, derived from its URL so a test can tell which file was passed. */
function bytesOf(url: string) {
  return Uint8Array.from(url, (character) => character.charCodeAt(0));
}

function mockModule(kitchen: Kitchen) {
  const startKitchen = vi.fn().mockResolvedValue(kitchen);
  loadKitchenWasmMock.mockResolvedValue({
    default: vi.fn().mockResolvedValue({}),
    startKitchen,
  });
  loadGlbMock.mockImplementation(async (url) => bytesOf(url));
  loadEnvironmentMock.mockResolvedValue({
    background: new Uint8Array([0x89, 0x50]),
    foreground: new Uint8Array([0x89, 0x4e]),
  });
  return startKitchen;
}

async function fill(label: RegExp, value: string) {
  const input = screen.getByLabelText(label);
  await userEvent.clear(input);
  await userEvent.type(input, value);
}

/** Press Start and wait for the wall size form; Mantine opens modals on a transition. */
function openForm() {
  return userEvent
    .click(screen.getByRole('button', { name: 'Start' }))
    .then(() => screen.findByRole('dialog', { name: 'Your kitchen wall' }));
}

/** Press Start, enter a wall and continue; resolves once the stage is running. */
async function startWith(width: string, height: string) {
  await openForm();
  await fill(/Wall width/, width);
  await fill(/Wall height/, height);
  await userEvent.click(screen.getByRole('button', { name: 'Continue' }));
  return screen.findByTestId('slot-summary');
}

/** The callback the component handed to `startKitchen`. */
function slotClickHandler(startKitchen: ReturnType<typeof mockModule>): SlotClickHandler {
  return startKitchen.mock.calls[0][6] as SlotClickHandler;
}

describe('KitchenConstructor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows only a Start button and loads nothing up front', () => {
    mockModule(fakeKitchen());
    render(<KitchenConstructor />);

    expect(screen.getByRole('button', { name: 'Start' })).toBeInTheDocument();
    expect(screen.queryByTestId(KITCHEN_CANVAS_ID)).not.toBeInTheDocument();
    expect(loadKitchenWasmMock).not.toHaveBeenCalled();
  });

  it('asks for the wall size before starting', async () => {
    mockModule(fakeKitchen());
    render(<KitchenConstructor />);

    const dialog = await openForm();

    expect(within(dialog).getByLabelText(/Wall width/)).toBeInTheDocument();
    expect(within(dialog).getByLabelText(/Wall height/)).toBeInTheDocument();
    expect(loadKitchenWasmMock).not.toHaveBeenCalled();
  });

  it('replaces the Start button with the constructor and hands Rust the wall size', async () => {
    const startKitchen = mockModule(fakeKitchen(4));
    render(<KitchenConstructor />);

    const summary = await startWith('360', '270');

    expect(summary).toHaveTextContent('0 of 4 slots filled');
    expect(screen.getByTestId('wall-summary')).toHaveTextContent('Wall 360 × 270 cm');
    expect(screen.queryByRole('button', { name: 'Start' })).not.toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());

    const canvas = screen.getByTestId(KITCHEN_CANVAS_ID);
    expect(startKitchen).toHaveBeenCalledWith(
      canvas,
      360,
      270,
      bytesOf('/models/kitchen-placeholder-box.glb'),
      new Uint8Array([0x89, 0x50]),
      new Uint8Array([0x89, 0x4e]),
      expect.any(Function),
    );
  });

  it('opens the module list for a clicked slot and places the chosen module', async () => {
    const kitchen = fakeKitchen(4);
    const startKitchen = mockModule(kitchen);
    render(<KitchenConstructor />);
    await startWith('360', '270');

    act(() => slotClickHandler(startKitchen)(1, null));

    const dialog = await screen.findByRole('dialog', { name: 'Slot 2 — choose a module' });
    expect(within(dialog).getByRole('button', { name: /Polished steel/ })).toBeInTheDocument();
    expect(within(dialog).queryByRole('button', { name: 'Remove module' })).not.toBeInTheDocument();

    await userEvent.click(within(dialog).getByRole('button', { name: /Aluminium/ }));

    expect(kitchen.placeModule).toHaveBeenCalledWith(
      1,
      'aluminium',
      bytesOf('/models/kitchen-module-1-aluminium.glb'),
    );
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(screen.getByTestId('slot-summary')).toHaveTextContent('1 of 4 slots filled');
  });

  it('marks the current module and can put the placeholder back', async () => {
    const kitchen = fakeKitchen(4);
    const startKitchen = mockModule(kitchen);
    render(<KitchenConstructor />);
    await startWith('360', '270');
    kitchen.placeModule(2, 'polished-steel');

    act(() => slotClickHandler(startKitchen)(2, 'polished-steel'));

    const dialog = await screen.findByRole('dialog', { name: 'Slot 3 — choose a module' });
    expect(within(dialog).getByRole('button', { name: /Polished steel/ })).toHaveAttribute(
      'aria-current',
      'true',
    );

    await userEvent.click(within(dialog).getByRole('button', { name: 'Remove module' }));

    expect(kitchen.clearSlot).toHaveBeenCalledWith(2);
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(screen.getByTestId('slot-summary')).toHaveTextContent('0 of 4 slots filled');
  });

  it('keeps the list open and explains when a module cannot be placed', async () => {
    const kitchen = fakeKitchen(4);
    kitchen.placeModule.mockImplementation(() => {
      // wasm-bindgen throws the Rust error as a plain string.
      throw 'could not load module `aluminium`: the asset contains no triangle geometry';
    });
    const startKitchen = mockModule(kitchen);
    render(<KitchenConstructor />);
    await startWith('300', '260');

    act(() => slotClickHandler(startKitchen)(0, null));
    const dialog = await screen.findByRole('dialog', { name: 'Slot 1 — choose a module' });
    await userEvent.click(within(dialog).getByRole('button', { name: /Aluminium/ }));

    expect(await within(dialog).findByRole('alert')).toHaveTextContent(
      'the asset contains no triangle geometry',
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('forwards the reset button to the constructor', async () => {
    const kitchen = fakeKitchen();
    mockModule(kitchen);
    render(<KitchenConstructor />);
    await startWith('300', '260');

    await userEvent.click(screen.getByRole('button', { name: 'Reset view' }));

    expect(kitchen.resetView).toHaveBeenCalledTimes(1);
  });

  it('surfaces an error when the constructor cannot start', async () => {
    loadKitchenWasmMock.mockResolvedValue({
      default: vi.fn().mockResolvedValue({}),
      startKitchen: vi.fn().mockRejectedValue(new Error('no GPU adapter is available')),
    });
    loadGlbMock.mockResolvedValue(new Uint8Array());
    loadEnvironmentMock.mockResolvedValue({
      background: new Uint8Array(),
      foreground: new Uint8Array(),
    });
    render(<KitchenConstructor />);

    const dialog = await openForm();
    await userEvent.click(within(dialog).getByRole('button', { name: 'Continue' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('no GPU adapter is available');
    expect(screen.getByRole('button', { name: 'Reset view' })).toBeDisabled();
  });

  it('destroys the constructor when it unmounts, so the GPU work stops', async () => {
    const kitchen = fakeKitchen();
    mockModule(kitchen);
    const { unmount } = render(<KitchenConstructor />);
    await startWith('300', '260');

    unmount();

    expect(kitchen.destroy).toHaveBeenCalled();
  });
});
