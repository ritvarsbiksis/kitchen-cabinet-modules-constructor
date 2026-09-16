import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  reactStrictMode: true,
  // The WASM bundle is not processed by the bundler - `src/lib/loadWasm.ts`
  // imports it at runtime from `public/wasm/`, so no wasm loader config is needed.
};

export default nextConfig;
