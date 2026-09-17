'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Badge, Button, Group, Loader, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { DimensionsModal } from '@/components/DimensionsModal';
import { ModulePickerModal, type PickerTarget } from '@/components/ModulePickerModal';
import {
  KITCHEN_ENVIRONMENT_URLS,
  PLACEHOLDER_URL,
  type KitchenModule,
  type WallDimensions,
} from '@/lib/kitchenCatalog';
import {
  loadGlb,
  loadKitchenWasm,
  type KitchenConstructor as Kitchen,
} from '@/lib/loadKitchenWasm';
import { loadEnvironment } from '@/lib/loadViewerWasm';
import styles from './KitchenConstructor.module.css';

/** Test id of the canvas the constructor renders into. */
export const KITCHEN_CANVAS_ID = 'kitchen-canvas';

/** What the constructor reports about itself once it is running. */
interface KitchenInfo {
  backend: string;
  slotCount: number;
}

/**
 * The full-width 3D stage: the canvas, the WASM lifecycle around it and the
 * module picker it opens.
 */
function KitchenStage({ dimensions }: { dimensions: WallDimensions }) {
  const { widthCm, heightCm } = dimensions;
  const sectionRef = useRef<HTMLElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const kitchenRef = useRef<Kitchen | null>(null);
  const [info, setInfo] = useState<KitchenInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picker, setPicker] = useState<PickerTarget | null>(null);
  const [filled, setFilled] = useState(0);

  // Bring the stage into view under the sticky nav bar as soon as it appears.
  useEffect(() => {
    sectionRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, []);

  useEffect(() => {
    // Set as soon as the effect is cleaned up, so StrictMode's double invoke
    // does not leave a second constructor running on a canvas that is gone.
    let cancelled = false;
    let kitchen: Kitchen | null = null;

    async function run() {
      try {
        const [wasm, placeholder, environment] = await Promise.all([
          loadKitchenWasm(),
          loadGlb(PLACEHOLDER_URL),
          loadEnvironment(KITCHEN_ENVIRONMENT_URLS),
        ]);
        const canvas = canvasRef.current;
        if (cancelled || !canvas) {
          return;
        }

        kitchen = await wasm.startKitchen(
          canvas,
          widthCm,
          heightCm,
          placeholder,
          environment.background,
          environment.foreground,
          (slot, moduleId) => {
            if (!cancelled) {
              setPicker({ slot, moduleId });
            }
          },
        );
        if (cancelled) {
          kitchen.destroy();
          kitchen = null;
          return;
        }

        kitchenRef.current = kitchen;
        setInfo({ backend: kitchen.backend, slotCount: kitchen.slotCount });
      } catch (cause) {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      }
    }

    void run();

    return () => {
      cancelled = true;
      kitchenRef.current = null;
      kitchen?.destroy();
    };
  }, [widthCm, heightCm]);

  const countFilled = useCallback((kitchen: Kitchen) => {
    let count = 0;
    for (let slot = 0; slot < kitchen.slotCount; slot += 1) {
      if (kitchen.slotModule(slot) !== undefined) {
        count += 1;
      }
    }
    setFilled(count);
  }, []);

  const handleSelect = useCallback(
    async (module: KitchenModule) => {
      const kitchen = kitchenRef.current;
      if (!kitchen || !picker) {
        return;
      }
      if (module.id !== picker.moduleId) {
        const glb = await loadGlb(module.url);
        kitchen.placeModule(picker.slot, module.id, glb);
        countFilled(kitchen);
      }
      setPicker(null);
    },
    [picker, countFilled],
  );

  const handleRemove = useCallback(() => {
    const kitchen = kitchenRef.current;
    if (kitchen && picker) {
      kitchen.clearSlot(picker.slot);
      countFilled(kitchen);
    }
    setPicker(null);
  }, [picker, countFilled]);

  const handleReset = useCallback(() => {
    kitchenRef.current?.resetView();
  }, []);

  const ready = info !== null;

  return (
    <section ref={sectionRef} className={styles.section} aria-label="Kitchen constructor">
      <div className={styles.toolbar}>
        <Group gap="sm" wrap="nowrap" className={styles.summary}>
          <Text size="sm" fw={600} data-testid="wall-summary">
            Wall {widthCm} × {heightCm} cm
          </Text>
          {info && (
            <>
              <Text size="sm" c="dimmed" truncate="end" data-testid="slot-summary">
                {filled} of {info.slotCount} slots filled
              </Text>
              <Badge variant="light" color="gray" size="sm" visibleFrom="sm">
                {info.backend}
              </Badge>
            </>
          )}
        </Group>

        <Group gap="md" wrap="nowrap">
          <Text size="xs" c="dimmed" visibleFrom="md">
            Drag to orbit · scroll or pinch to zoom · click a box to choose a module
          </Text>
          <Button variant="default" size="xs" onClick={handleReset} disabled={!ready}>
            Reset view
          </Button>
        </Group>
      </div>

      <div className={styles.stage}>
        {/* Kept mounted from the first render: Rust creates the wgpu surface
            against this exact element. */}
        <canvas
          ref={canvasRef}
          id={KITCHEN_CANVAS_ID}
          className={styles.canvas}
          data-testid={KITCHEN_CANVAS_ID}
          data-ready={ready || undefined}
          aria-label={`3D kitchen with a ${widthCm} cm wall. Drag to orbit, scroll to zoom, click a box to choose a module.`}
          role="img"
        />

        {!ready && !error && (
          <div className={styles.overlay}>
            <Loader color="orange" size="sm" />
            <Text size="sm" c="dimmed">
              Building the room…
            </Text>
          </div>
        )}

        {error && (
          <div className={styles.overlay}>
            <Alert
              color="red"
              title="Could not start the constructor"
              role="alert"
              className={styles.error}
            >
              {error}
            </Alert>
          </div>
        )}
      </div>

      <ModulePickerModal
        target={picker}
        onClose={() => setPicker(null)}
        onSelect={handleSelect}
        onRemove={handleRemove}
      />
    </section>
  );
}

/**
 * The Start button, the wall size form it opens, and - once the form is
 * submitted - the 3D constructor in its place.
 */
export function KitchenConstructor() {
  const [formOpened, { open, close }] = useDisclosure(false);
  const [dimensions, setDimensions] = useState<WallDimensions | null>(null);

  const handleSubmit = useCallback(
    (submitted: WallDimensions) => {
      close();
      setDimensions(submitted);
    },
    [close],
  );

  if (dimensions) {
    return <KitchenStage dimensions={dimensions} />;
  }

  return (
    <div className={styles.start}>
      <Button onClick={open} color="orange" size="lg">
        Start
      </Button>
      <DimensionsModal opened={formOpened} onClose={close} onSubmit={handleSubmit} />
    </div>
  );
}
