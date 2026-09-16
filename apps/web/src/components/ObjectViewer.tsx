'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Button, Group, Loader, Modal, Stack, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { loadEnvironment, loadModel, loadViewerWasm, type Viewer } from '@/lib/loadViewerWasm';
import styles from './ObjectViewer.module.css';

/** Test id of the canvas wgpu renders into. */
export const VIEWER_CANVAS_ID = 'wgpu-canvas';

/** What the viewer reports about itself once it is running. */
interface ViewerInfo {
  backend: string;
  triangles: number;
}

/**
 * The canvas plus the WASM lifecycle around it.
 *
 * Rendered only while the modal is open, so the module and the model are
 * downloaded on first use and the GPU resources are released on close.
 */
function ViewerStage() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const viewerRef = useRef<Viewer | null>(null);
  const [info, setInfo] = useState<ViewerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Set as soon as the effect is cleaned up. Without it, React's StrictMode
    // double-invoke in development would leave a second viewer running against
    // a canvas that is already gone.
    let cancelled = false;
    let viewer: Viewer | null = null;

    async function run() {
      try {
        const [wasm, model, environment] = await Promise.all([
          loadViewerWasm(),
          loadModel(),
          loadEnvironment(),
        ]);
        const canvas = canvasRef.current;
        if (cancelled || !canvas) {
          return;
        }

        viewer = await wasm.startViewer(
          canvas,
          model,
          environment.background,
          environment.foreground,
        );
        if (cancelled) {
          viewer.destroy();
          viewer = null;
          return;
        }

        viewerRef.current = viewer;
        setInfo({ backend: viewer.backend, triangles: viewer.triangleCount });
      } catch (cause) {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      }
    }

    void run();

    return () => {
      cancelled = true;
      viewerRef.current = null;
      viewer?.destroy();
    };
  }, []);

  const handleReset = useCallback(() => {
    viewerRef.current?.resetView();
  }, []);

  const ready = info !== null;

  return (
    <Stack gap="sm">
      <div className={styles.stage}>
        {/* Kept mounted from the first render: Rust needs a real canvas to
            create the wgpu surface against, and it must not be swapped out
            underneath the renderer afterwards. */}
        <canvas
          ref={canvasRef}
          id={VIEWER_CANVAS_ID}
          className={styles.canvas}
          data-testid="wgpu-canvas"
          data-ready={ready || undefined}
          aria-label="3D model of a pair of sunglasses. Drag to rotate, scroll to zoom."
          role="img"
        />

        {!ready && !error && (
          <div className={styles.overlay}>
            <Loader color="orange" size="sm" />
            <Text size="sm" c="dimmed">
              Compiling shaders and uploading the mesh…
            </Text>
          </div>
        )}
      </div>

      {error && (
        <Alert color="red" title="Could not start the viewer" role="alert">
          {error}
        </Alert>
      )}

      <Group justify="space-between" align="center">
        <Text size="xs" c="dimmed">
          Drag to rotate · scroll or pinch to zoom
        </Text>

        <Group gap="sm" align="center">
          {info && (
            <Text size="xs" c="dimmed" data-testid="viewer-info">
              {info.backend} · {info.triangles.toLocaleString('en-US')} triangles
            </Text>
          )}
          <Button variant="default" size="xs" onClick={handleReset} disabled={!ready}>
            Reset view
          </Button>
        </Group>
      </Group>
    </Stack>
  );
}

/** The button on the page, and the modal it opens. */
export function ObjectViewer() {
  const [opened, { open, close }] = useDisclosure(false);

  return (
    <>
      <Button onClick={open} color="orange" size="md" w="fit-content">
        View 3D object
      </Button>

      <Modal
        opened={opened}
        onClose={close}
        title="Sunglasses — rendered with wgpu"
        size="xl"
        centered
        radius="md"
        closeButtonProps={{ 'aria-label': 'Close the 3D viewer' }}
      >
        {/* Mantine unmounts the modal body when it closes, which is what tears
            the viewer down - see the cleanup in `ViewerStage`. */}
        <ViewerStage />
      </Modal>
    </>
  );
}
