import { describe, expect, it, vi } from 'vitest';
import { screen, render } from '@/test-utils/render';
import { NavBar } from '@/components/NavBar';

vi.mock('next/navigation', () => ({
  usePathname: () => '/wasm-example',
}));

describe('NavBar', () => {
  it('links to every route', () => {
    render(<NavBar />);

    expect(screen.getByRole('link', { name: 'Home' })).toHaveAttribute('href', '/');
    expect(screen.getByRole('link', { name: 'WASM example' })).toHaveAttribute(
      'href',
      '/wasm-example',
    );
    expect(screen.getByRole('link', { name: 'WGPU example' })).toHaveAttribute(
      'href',
      '/wgpu-example',
    );
  });

  it('marks the current route as active', () => {
    render(<NavBar />);

    expect(screen.getByRole('link', { name: 'WASM example' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('link', { name: 'Home' })).not.toHaveAttribute('aria-current');
  });
});
