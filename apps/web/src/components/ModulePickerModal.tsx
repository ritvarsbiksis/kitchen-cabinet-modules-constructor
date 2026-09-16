'use client';

import { useEffect, useState } from 'react';
import {
  Alert,
  Badge,
  Button,
  Group,
  Loader,
  Modal,
  Stack,
  Text,
  UnstyledButton,
} from '@mantine/core';
import { KITCHEN_MODULES, type KitchenModule } from '@/lib/kitchenCatalog';
import styles from './ModulePickerModal.module.css';

/** The slot the picker was opened for. */
export interface PickerTarget {
  /** Zero-based index, left to right. */
  slot: number;
  /** The module in the slot, or `null` while it holds the placeholder. */
  moduleId: string | null;
}

interface ModulePickerModalProps {
  /** Open while set. */
  target: PickerTarget | null;
  onClose: () => void;
  /** Place `module` in the slot. May reject, which the modal reports. */
  onSelect: (module: KitchenModule) => Promise<void>;
  /** Put the placeholder back. */
  onRemove: () => void;
}

/** The list of modules a slot can hold. */
export function ModulePickerModal({ target, onClose, onSelect, onRemove }: ModulePickerModalProps) {
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // A fresh slot starts with a clean slate.
  useEffect(() => {
    setPendingId(null);
    setError(null);
  }, [target?.slot, target?.moduleId]);

  async function handleSelect(module: KitchenModule) {
    setPendingId(module.id);
    setError(null);
    try {
      await onSelect(module);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPendingId(null);
    }
  }

  const busy = pendingId !== null;

  return (
    <Modal
      opened={target !== null}
      onClose={onClose}
      title={target ? `Slot ${target.slot + 1} — choose a module` : undefined}
      centered
      radius="md"
      closeButtonProps={{ 'aria-label': 'Close the module list' }}
    >
      <Stack gap="sm">
        <ul aria-label="Kitchen modules" className={styles.list}>
          {KITCHEN_MODULES.map((module) => {
            const current = target?.moduleId === module.id;
            return (
              <li key={module.id}>
                <UnstyledButton
                  className={styles.option}
                  data-current={current || undefined}
                  onClick={() => void handleSelect(module)}
                  disabled={busy}
                  aria-current={current || undefined}
                >
                  <span
                    className={styles.swatch}
                    style={{ background: module.swatch }}
                    aria-hidden
                  />
                  <span className={styles.text}>
                    <Text fw={600} size="sm" component="span" className={styles.name}>
                      {module.name}
                    </Text>
                    <Text size="xs" c="dimmed" component="span">
                      {module.description}
                    </Text>
                  </span>
                  {pendingId === module.id ? (
                    <Loader size="xs" color="orange" />
                  ) : (
                    current && (
                      <Badge size="sm" variant="light" color="orange">
                        Current
                      </Badge>
                    )
                  )}
                </UnstyledButton>
              </li>
            );
          })}
        </ul>

        {error && (
          <Alert color="red" title="Could not place the module" role="alert">
            {error}
          </Alert>
        )}

        {target?.moduleId && (
          <Group justify="flex-end">
            <Button variant="subtle" color="red" size="xs" onClick={onRemove} disabled={busy}>
              Remove module
            </Button>
          </Group>
        )}
      </Stack>
    </Modal>
  );
}
