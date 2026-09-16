'use client';

import { memo, useState } from 'react';
import { Alert, Button, Group, Paper, Text } from '@mantine/core';
import { loadWasm } from '@/lib/loadWasm';
import styles from './WasmRunner.module.css';

/** Id of the DIV the Rust side mounts its Leptos view into. */
export const WASM_TARGET_ID = 'wasm-target';

/**
 * The element Rust writes into.
 *
 * Wrapped in `memo` with no props deliberately: React must never re-render this
 * subtree. Leptos mutates these children from outside React, and anything React
 * reconciles here can wipe that content on an unrelated state change. A memo
 * component with no props always bails out, so React renders the div exactly
 * once and never touches it again.
 */
const WasmTarget = memo(function WasmTarget() {
  return <div id={WASM_TARGET_ID} className={styles.target} data-testid="wasm-target" />;
});

export function WasmRunner() {
  const [loading, setLoading] = useState(false);
  const [hasRun, setHasRun] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleRun() {
    setLoading(true);
    setError(null);

    try {
      const wasm = await loadWasm();
      wasm.run_wasm(WASM_TARGET_ID);
      setHasRun(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setHasRun(false);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.root}>
      <Group>
        <Button onClick={handleRun} loading={loading} color="orange" size="md">
          Run WASM
        </Button>
        <Text size="sm" c="dimmed">
          {hasRun
            ? 'Rendered by Leptos, compiled to WebAssembly.'
            : 'Nothing has touched the target element yet.'}
        </Text>
      </Group>

      {error && (
        <Alert color="red" title="WASM call failed" mt="md" role="alert">
          {error}
        </Alert>
      )}

      <Paper withBorder radius="md" mt="md" className={styles.stage}>
        <Text size="xs" tt="uppercase" fw={700} c="dimmed" className={styles.stageLabel}>
          {`<div id="${WASM_TARGET_ID}">`}
        </Text>

        <WasmTarget />

        {/* Kept mounted and toggled with an attribute rather than conditionally
            rendered, so the child list around <WasmTarget /> never changes. */}
        <Text
          size="sm"
          c="dimmed"
          fs="italic"
          className={styles.hint}
          data-done={hasRun || undefined}
        >
          empty — press “Run WASM”
        </Text>
      </Paper>
    </div>
  );
}
