import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

const nodeStub = fileURLToPath(new URL('./src/shims/node-stub.ts', import.meta.url))

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  resolve: {
    // The emscripten talib-wasm bundle conditionally requires these under
    // Node only; stub them out for the browser bundle. Disabled in test mode
    // so vitest's node imports (node:path etc.) keep working.
    alias: (mode === 'test'
      ? {}
      : {
          ws: nodeStub,
          fs: nodeStub,
          path: nodeStub,
          crypto: nodeStub,
          'node:fs': nodeStub,
          'node:path': nodeStub,
          'node:crypto': nodeStub,
        }) as Record<string, string>,
  },
  build: {
    chunkSizeWarningLimit: 6000, // talib wasm + chart data makes chunks big; expected
  },
}))
