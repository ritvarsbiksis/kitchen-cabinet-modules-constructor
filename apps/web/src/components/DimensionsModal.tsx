'use client';

import { useEffect, useState, type FormEvent } from 'react';
import { Button, Group, Modal, NumberInput, Stack, Text } from '@mantine/core';
import {
  WALL_LIMITS,
  slotCountFor,
  type CentimetreRange,
  type WallDimensions,
} from '@/lib/kitchenCatalog';

/** What the form starts with: a common kitchen wall. */
export const DEFAULT_DIMENSIONS: WallDimensions = { widthCm: 300, heightCm: 260 };

/**
 * Why `value` is not an acceptable size, or `null` if it is. `NumberInput`
 * reports an empty field as `''`.
 */
export function dimensionError(
  label: string,
  value: number | string,
  range: CentimetreRange,
): string | null {
  if (value === '' || Number.isNaN(Number(value))) {
    return `Enter the wall ${label.toLowerCase()}`;
  }

  const centimetres = Number(value);
  if (!Number.isInteger(centimetres)) {
    return `${label} must be a whole number of centimetres`;
  }
  if (centimetres < range.min || centimetres > range.max) {
    return `${label} must be between ${range.min} and ${range.max} cm`;
  }
  return null;
}

interface DimensionsModalProps {
  opened: boolean;
  onClose: () => void;
  onSubmit: (dimensions: WallDimensions) => void;
}

/** Asks for the wall the kitchen will stand against. */
export function DimensionsModal({ opened, onClose, onSubmit }: DimensionsModalProps) {
  const [width, setWidth] = useState<number | string>(DEFAULT_DIMENSIONS.widthCm);
  const [height, setHeight] = useState<number | string>(DEFAULT_DIMENSIONS.heightCm);
  // Errors only show once the user has tried to continue, then update live.
  const [attempted, setAttempted] = useState(false);

  useEffect(() => {
    if (!opened) {
      setAttempted(false);
    }
  }, [opened]);

  const widthError = dimensionError('Width', width, WALL_LIMITS.width);
  const heightError = dimensionError('Height', height, WALL_LIMITS.height);
  const slots = widthError ? null : slotCountFor(Number(width));

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setAttempted(true);
    if (widthError || heightError) {
      return;
    }
    onSubmit({ widthCm: Number(width), heightCm: Number(height) });
  }

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title="Your kitchen wall"
      centered
      radius="md"
      closeButtonProps={{ 'aria-label': 'Close the wall size form' }}
    >
      <form onSubmit={handleSubmit} noValidate>
        <Stack gap="md">
          <Text size="sm" c="dimmed">
            Modules are lined up along this wall, as many 80 cm units as fit across it.
          </Text>

          <NumberInput
            label="Wall width"
            description={`${WALL_LIMITS.width.min}–${WALL_LIMITS.width.max} cm`}
            suffix=" cm"
            value={width}
            onChange={setWidth}
            min={WALL_LIMITS.width.min}
            max={WALL_LIMITS.width.max}
            step={10}
            allowDecimal={false}
            allowNegative={false}
            clampBehavior="none"
            error={attempted ? widthError : null}
            required
            data-autofocus
          />

          <NumberInput
            label="Wall height"
            description={`${WALL_LIMITS.height.min}–${WALL_LIMITS.height.max} cm`}
            suffix=" cm"
            value={height}
            onChange={setHeight}
            min={WALL_LIMITS.height.min}
            max={WALL_LIMITS.height.max}
            step={5}
            allowDecimal={false}
            allowNegative={false}
            clampBehavior="none"
            error={attempted ? heightError : null}
            required
          />

          <Group justify="space-between" align="center" mt="xs">
            <Text size="xs" c="dimmed" data-testid="slot-preview">
              {slots === null ? ' ' : `Room for ${slots} modules`}
            </Text>
            <Button type="submit" color="orange">
              Continue
            </Button>
          </Group>
        </Stack>
      </form>
    </Modal>
  );
}
