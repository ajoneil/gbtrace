// Thin wrapper around WASM initialization.
// Ensures init() is called exactly once before any TraceStore use.

let initPromise = null;
let wasmModule = null;

export async function loadWasm() {
  if (wasmModule) return wasmModule;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    const mod = await import('../../pkg/gbtrace_wasm.js');
    await mod.default();
    wasmModule = mod;
    return mod;
  })();

  return initPromise;
}

export async function createTraceStore(bytes) {
  const mod = await loadWasm();
  return new mod.TraceStore(bytes);
}

export async function prepareForDiff(storeA, storeB) {
  const mod = await loadWasm();
  return mod.prepareForDiff(storeA, storeB);
}

/** Synchronous version — only works after WASM is already loaded. */
export function prepareForDiffSync(storeA, storeB) {
  if (!wasmModule) throw new Error('WASM not loaded');
  return wasmModule.prepareForDiff(storeA, storeB);
}
