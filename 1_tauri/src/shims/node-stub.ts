/**
 * Stub for Node builtins conditionally `require()`'d by the emscripten
 * TA-Lib WASM bundle (fs/path/crypto/ws). Those code paths only run under
 * Node; in the browser they are dead code. Aliased in vite.config.ts.
 */
export default {};
export const readFileSync = () => {
  throw new Error('fs not available in browser');
};
