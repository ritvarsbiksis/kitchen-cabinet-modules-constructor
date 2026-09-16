import Link from 'next/link';
import { Button, Group, List, ListItem, Stack, Text, Title } from '@mantine/core';
import styles from './page.module.css';

export default function HomePage() {
  return (
    <Stack gap="lg">
      <div>
        <Title order={1}>
          Next.js <span className={styles.accent}>+</span> Rust
        </Title>
        <Text c="dimmed" mt="xs">
          A monorepo scaffold: a TypeScript Next.js app that calls a Leptos component compiled to
          WebAssembly.
        </Text>
      </div>

      <div className={styles.card}>
        <Title order={2} size="h4">
          What is in here
        </Title>
        <List spacing="xs" size="sm" mt="sm">
          <ListItem>
            <code>apps/web</code> — Next.js App Router, TypeScript, Mantine, CSS Modules
          </ListItem>
          <ListItem>
            <code>crates/wasm-hello</code> — Leptos component built with <code>wasm-pack</code>
          </ListItem>
          <ListItem>
            <code>crates/wasm-viewer</code> — glTF viewer rendered with <code>wgpu</code>
          </ListItem>
          <ListItem>ESLint + Prettier for linting and formatting</ListItem>
          <ListItem>Vitest + Testing Library for unit tests</ListItem>
          <ListItem>Turborepo orchestrating the WASM build ahead of the Next.js build</ListItem>
        </List>
      </div>

      <Group>
        <Button component={Link} href="/wasm-example" color="orange" size="md">
          Go to the WASM example
        </Button>
        <Button component={Link} href="/wgpu-example" variant="default" size="md">
          Go to the WGPU example
        </Button>
      </Group>
    </Stack>
  );
}
