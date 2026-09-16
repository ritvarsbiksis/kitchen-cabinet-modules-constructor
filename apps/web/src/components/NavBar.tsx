'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { Group, Text } from '@mantine/core';
import styles from './NavBar.module.css';

const ROUTES = [
  { href: '/', label: 'Home' },
  { href: '/wasm-example', label: 'WASM example' },
  { href: '/wgpu-example', label: 'WGPU example' },
] as const;

export function NavBar() {
  const pathname = usePathname();

  return (
    <header className={styles.header}>
      <Group justify="space-between" h="100%" px="md">
        <Text fw={700} size="lg" className={styles.brand}>
          rust-wasm-example
        </Text>

        <Group gap="xs" component="nav">
          {ROUTES.map((route) => (
            <Link
              key={route.href}
              href={route.href}
              className={styles.link}
              data-active={pathname === route.href || undefined}
              aria-current={pathname === route.href ? 'page' : undefined}
            >
              {route.label}
            </Link>
          ))}
        </Group>
      </Group>
    </header>
  );
}
