import { describe, expect, it, vi, beforeEach } from 'vitest';
import userEvent from '@testing-library/user-event';
import { screen, render } from '@/test-utils/render';
import { WasmRunner, WASM_TARGET_ID } from '@/components/WasmRunner';
import { loadWasm } from '@/lib/loadWasm';

// The real loader pulls a .wasm file over the network, which jsdom cannot do -
// so the module boundary is where this is stubbed.
vi.mock('@/lib/loadWasm', () => ({
  loadWasm: vi.fn(),
}));

const loadWasmMock = vi.mocked(loadWasm);

/** Stand-in for the wasm-bindgen module, with `run_wasm` spied on. */
function fakeWasmModule() {
  const run_wasm = vi.fn();
  loadWasmMock.mockResolvedValue({ default: vi.fn().mockResolvedValue({}), run_wasm });
  return run_wasm;
}

describe('WasmRunner', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders the button and an empty target div', () => {
    fakeWasmModule();
    render(<WasmRunner />);

    expect(screen.getByRole('button', { name: 'Run WASM' })).toBeInTheDocument();

    const target = screen.getByTestId('wasm-target');
    expect(target).toHaveAttribute('id', WASM_TARGET_ID);
    expect(target).toBeEmptyDOMElement();
  });

  it('calls the exported WASM function with the target id when clicked', async () => {
    const run_wasm = fakeWasmModule();
    render(<WasmRunner />);

    await userEvent.click(screen.getByRole('button', { name: 'Run WASM' }));

    expect(loadWasmMock).toHaveBeenCalledTimes(1);
    expect(run_wasm).toHaveBeenCalledWith(WASM_TARGET_ID);
    expect(
      await screen.findByText('Rendered by Leptos, compiled to WebAssembly.'),
    ).toBeInTheDocument();
  });

  it('loads the module once across repeated clicks', async () => {
    const run_wasm = fakeWasmModule();
    render(<WasmRunner />);

    const button = screen.getByRole('button', { name: 'Run WASM' });
    await userEvent.click(button);
    await userEvent.click(button);

    expect(run_wasm).toHaveBeenCalledTimes(2);
  });

  // Asserts the invariant the WASM integration depends on: the target node keeps
  // its identity and the content written into it from outside React survives
  // re-renders. Note this passes with or without the `memo` wrapper - the actual
  // clobbering only reproduces in a real browser, so `memo` is the guard and this
  // test only documents and pins the expected behaviour.
  it('keeps DOM written by WASM intact across re-renders', async () => {
    const run_wasm = vi.fn((id: string) => {
      const target = document.getElementById(id)!;
      target.innerHTML = '<p class="wasm-hello">Hello World!</p>';
    });
    loadWasmMock.mockResolvedValue({ default: vi.fn().mockResolvedValue({}), run_wasm });
    render(<WasmRunner />);

    const button = screen.getByRole('button', { name: 'Run WASM' });
    const target = screen.getByTestId('wasm-target');

    for (let i = 0; i < 3; i += 1) {
      await userEvent.click(button);
      // Same node every time - React never replaced it...
      expect(screen.getByTestId('wasm-target')).toBe(target);
      // ...and the content Rust wrote is still there, exactly once.
      expect(target.children).toHaveLength(1);
      expect(target).toHaveTextContent('Hello World!');
    }
  });

  it('surfaces an error when the WASM module fails to load', async () => {
    loadWasmMock.mockRejectedValue(new Error('failed to fetch /wasm/wasm_hello.js'));
    render(<WasmRunner />);

    await userEvent.click(screen.getByRole('button', { name: 'Run WASM' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('failed to fetch /wasm/wasm_hello.js');
  });

  it('surfaces an error when the Rust function throws', async () => {
    const run_wasm = vi.fn(() => {
      throw new Error('no element with id `wasm-target` found');
    });
    loadWasmMock.mockResolvedValue({ default: vi.fn().mockResolvedValue({}), run_wasm });
    render(<WasmRunner />);

    await userEvent.click(screen.getByRole('button', { name: 'Run WASM' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'no element with id `wasm-target` found',
    );
  });
});
