import { Code, Stack, Text, Title } from '@mantine/core';
import { WasmRunner } from '@/components/WasmRunner';
import styles from './page.module.css';

export const metadata = {
  title: 'WASM example',
};

export default function WasmExamplePage() {
  return (
    <Stack gap="md">
      <div>
        <Title order={1} size="h2">
          WASM example
        </Title>
        <Text c="dimmed" mt="xs">
          Pressing the button loads <Code>/wasm/wasm_hello.js</Code> at runtime, instantiates the
          WebAssembly module, and calls the exported <Code>run_wasm</Code> function. That Rust
          function mounts a Leptos component into the target <Code>&lt;div&gt;</Code> below.
        </Text>
      </div>

      <div className={styles.source}>
        <Text size="xs" tt="uppercase" fw={700} c="dimmed" mb={6}>
          crates/wasm-hello/src/lib.rs
        </Text>
        <pre className={styles.code}>
          {`#[wasm_bindgen]
pub fn run_wasm(target_id: &str) -> Result<(), JsValue> {
    let target = document.get_element_by_id(target_id)?;
    target.set_inner_html("");
    mount_to(target, HelloWorld).forget();
    Ok(())
}`}
        </pre>
      </div>

      <WasmRunner />
    </Stack>
  );
}
