declare module 'talib-wasm/build/talib.wrapped.min.js' {
  /** Emscripten module factory; resolves to the TA-Lib WASM module (cwrap, _malloc, HEAPF64, ...). */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const factory: () => Promise<any>;
  export default factory;
}
