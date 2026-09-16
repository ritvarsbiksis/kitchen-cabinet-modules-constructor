import { Code, Text, Title } from '@mantine/core';
import { KitchenConstructor } from '@/components/KitchenConstructor';
import styles from './page.module.css';

export const metadata = {
  title: 'Kitchen constructor',
};

export default function KitchenConstructorPage() {
  return (
    // `data-full-width` lifts the root layout's reading-column width limit, so
    // the constructor can span the whole viewport once it starts.
    <div data-full-width className={styles.page}>
      <div className={styles.intro}>
        <Title order={1} size="h2">
          Kitchen constructor
        </Title>
        <Text c="dimmed" mt="xs">
          Tell us how big your kitchen wall is and plan the run of base units along it in 3D. The
          room is rendered by the <Code>wasm-kitchen</Code> crate with <Code>wgpu</Code> — WebGPU
          where the browser has it, WebGL2 everywhere else. Hover a glass box to light it up, click
          it to pick the module that goes there.
        </Text>
      </div>

      <KitchenConstructor />
    </div>
  );
}
