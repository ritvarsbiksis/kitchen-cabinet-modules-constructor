import { Anchor, Code, List, ListItem, Stack, Text, Title } from '@mantine/core';
import { ObjectViewer } from '@/components/ObjectViewer';
import styles from './page.module.css';

export const metadata = {
  title: 'WGPU example',
};

export default function WgpuExamplePage() {
  return (
    <Stack gap="md">
      <div>
        <Title order={1} size="h2">
          WGPU example
        </Title>
        <Text c="dimmed" mt="xs">
          Pressing the button opens a modal whose canvas is driven entirely from Rust. The
          <Code>wasm-viewer</Code> crate parses a binary glTF file, uploads the meshes and the
          embedded texture to the GPU with the <Code>wgpu</Code> crate, and runs its own render
          loop. wgpu targets WebGPU where the browser supports it and falls back to WebGL2
          everywhere else — the badge under the canvas says which one you got.
        </Text>
      </div>

      <div className={styles.card}>
        <Title order={2} size="h4">
          What the Rust side does
        </Title>
        <List spacing="xs" size="sm" mt="sm">
          <ListItem>
            Parses the <Code>.glb</Code>, flattens the node hierarchy and bakes every transform into
            the vertices
          </ListItem>
          <ListItem>
            Shades with metallic-roughness PBR in WGSL, with the lenses blended back-to-front for
            their <Code>KHR_materials_transmission</Code> glass
          </ListItem>
          <ListItem>
            Handles pointer, pinch and wheel events itself to orbit and zoom an orbit camera
          </ListItem>
          <ListItem>
            Watches the canvas with a <Code>ResizeObserver</Code> and keeps the drawing buffer at
            device-pixel resolution
          </ListItem>
        </List>
      </div>

      <div className={styles.source}>
        <Text size="xs" tt="uppercase" fw={700} c="dimmed" mb={6}>
          crates/wasm-viewer/src/viewer.rs
        </Text>
        <pre className={styles.code}>
          {`#[wasm_bindgen(js_name = startViewer)]
pub async fn start_viewer(
    canvas: web_sys::HtmlCanvasElement,
    model_bytes: Vec<u8>,
) -> Result<Viewer, JsValue> {
    let model = Model::from_glb(&model_bytes)?;
    let renderer = Renderer::new(canvas.clone(), &model, width, height).await?;
    // ... listeners + requestAnimationFrame loop
}`}
        </pre>
      </div>

      <ObjectViewer />

      <Text size="xs" c="dimmed">
        Model: “Sunglasses” by Eric Chadwick, Darmstadt Graphics Group GmbH for the{' '}
        <Anchor
          href="https://github.com/KhronosGroup/glTF-Sample-Assets"
          target="_blank"
          rel="noreferrer"
        >
          Khronos glTF sample assets
        </Anchor>
        , licensed CC BY 4.0.
      </Text>
    </Stack>
  );
}
