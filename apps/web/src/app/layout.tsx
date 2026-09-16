import type { Metadata } from 'next';
import '@mantine/core/styles.css';
import './globals.css';
import { ColorSchemeScript, MantineProvider, mantineHtmlProps } from '@mantine/core';
import { NavBar } from '@/components/NavBar';
import styles from './layout.module.css';

export const metadata: Metadata = {
  title: 'Next.js + Leptos WASM',
  description: 'A Next.js app calling into a Leptos component compiled to WebAssembly',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript />
      </head>
      <body>
        <MantineProvider defaultColorScheme="auto">
          <NavBar />
          <main className={styles.main}>{children}</main>
        </MantineProvider>
      </body>
    </html>
  );
}
