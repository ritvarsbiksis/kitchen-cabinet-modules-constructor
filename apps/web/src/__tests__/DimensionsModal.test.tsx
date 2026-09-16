import { describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen } from '@/test-utils/render';
import { DimensionsModal, dimensionError } from '@/components/DimensionsModal';
import { WALL_LIMITS } from '@/lib/kitchenCatalog';

function renderModal() {
  const onSubmit = vi.fn();
  render(<DimensionsModal opened onClose={vi.fn()} onSubmit={onSubmit} />);
  return onSubmit;
}

async function fill(label: RegExp, value: string) {
  const input = screen.getByLabelText(label);
  await userEvent.clear(input);
  if (value !== '') {
    await userEvent.type(input, value);
  }
}

describe('dimensionError', () => {
  it('accepts both ends of the range', () => {
    expect(dimensionError('Width', 200, WALL_LIMITS.width)).toBeNull();
    expect(dimensionError('Width', 500, WALL_LIMITS.width)).toBeNull();
  });

  it('rejects sizes outside the range, blanks and fractions', () => {
    expect(dimensionError('Width', 199, WALL_LIMITS.width)).toBe(
      'Width must be between 200 and 500 cm',
    );
    expect(dimensionError('Height', 301, WALL_LIMITS.height)).toBe(
      'Height must be between 250 and 300 cm',
    );
    expect(dimensionError('Height', '', WALL_LIMITS.height)).toBe('Enter the wall height');
    expect(dimensionError('Width', 250.5, WALL_LIMITS.width)).toBe(
      'Width must be a whole number of centimetres',
    );
  });
});

describe('DimensionsModal', () => {
  it('submits the entered wall size', async () => {
    const onSubmit = renderModal();

    await fill(/Wall width/, '360');
    await fill(/Wall height/, '270');
    expect(screen.getByTestId('slot-preview')).toHaveTextContent('Room for 4 modules');
    await userEvent.click(screen.getByRole('button', { name: 'Continue' }));

    expect(onSubmit).toHaveBeenCalledWith({ widthCm: 360, heightCm: 270 });
  });

  it.each([
    ['199', '260', 'Width must be between 200 and 500 cm'],
    ['501', '260', 'Width must be between 200 and 500 cm'],
    ['300', '249', 'Height must be between 250 and 300 cm'],
    ['300', '301', 'Height must be between 250 and 300 cm'],
  ])('refuses a %s x %s cm wall', async (width, height, message) => {
    const onSubmit = renderModal();

    await fill(/Wall width/, width);
    await fill(/Wall height/, height);
    await userEvent.click(screen.getByRole('button', { name: 'Continue' }));

    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('does not clamp an out-of-range value into range behind the user’s back', async () => {
    renderModal();

    await fill(/Wall width/, '800');
    await userEvent.tab();

    expect(screen.getByLabelText(/Wall width/)).toHaveValue('800 cm');
  });
});
