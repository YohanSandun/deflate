import initWasm, { decompress as wasmDecompress } from "../pkg/deflate.js";

/** Anything wasm-bindgen accepts as the WebAssembly module source. */
export type InitInput =
  | RequestInfo
  | URL
  | Response
  | BufferSource
  | WebAssembly.Module;

let ready: Promise<void> | undefined;
let initialized = false;

/**
 * Loads the WebAssembly module. Call once (and await it) before `decompress`.
 * Calling it again returns the same promise.
 *
 * With no argument, the `.wasm` file next to this package is fetched in the
 * browser, or read from disk in Node.js. Pass `input` to load it from somewhere
 * else, e.g. a CDN URL or bytes you already have.
 */
export function init(input?: InitInput): Promise<void> {
  ready ??= load(input).then(
    () => {
      initialized = true;
    },
    (error: unknown) => {
      // Let a later call retry instead of caching the failure.
      ready = undefined;
      throw error;
    },
  );

  return ready;
}

async function load(input?: InitInput): Promise<void> {
  if (input === undefined && isNode()) {
    // Node's fetch can't read file:// URLs, so read the module from disk.
    const { readFile } = await import(/* webpackIgnore: true */ /* @vite-ignore */ "node:fs/promises");
    input = await readFile(new URL("../pkg/deflate_bg.wasm", import.meta.url));
  }

  await initWasm(input === undefined ? undefined : { module_or_path: input });
}

function isNode(): boolean {
  return typeof process !== "undefined" && process.versions?.node != null;
}

function ensureInitialized(): void {
  if (!initialized) {
    throw new Error("deflate-wasm is not initialized: await init() before calling decompress().");
  }
}

/**
 * Decompresses raw DEFLATE data (RFC 1951, no zlib or gzip wrapper).
 * Throws an `Error` if the data is corrupt or truncated.
 */
export function decompress(data: Uint8Array): Uint8Array {
  ensureInitialized();
  return wasmDecompress(data);
}
