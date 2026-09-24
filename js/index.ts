import initWasm, {
  decompress as wasmDecompress,
  decompressToBuffer as wasmDecompressToBuffer,
  type DecompressedBuffer,
} from "../pkg/deflate.js";

/** Anything wasm-bindgen accepts as the WebAssembly module source. */
export type InitInput =
  | RequestInfo
  | URL
  | Response
  | BufferSource
  | WebAssembly.Module;

let ready: Promise<void> | undefined;
let initialized = false;
let memory: WebAssembly.Memory | undefined;

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

  const exports = await initWasm(input === undefined ? undefined : { module_or_path: input });
  memory = exports.memory;
}

function isNode(): boolean {
  return typeof process !== "undefined" && process.versions?.node != null;
}

function ensureInitialized(): void {
  if (!initialized) {
    throw new Error("deflate-wasm is not initialized: await init() before decompressing.");
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

/**
 * Decompresses raw DEFLATE data like `decompress`, but leaves the output inside
 * WebAssembly memory and returns a view of it, skipping the copy into a new
 * `Uint8Array`. Worth it for large outputs you only need to read once, e.g. to
 * parse, hash, or write to a stream.
 *
 * Call `free()` when done (or use `using view = decompressView(data)` where
 * explicit resource management is supported). If you forget, the memory is
 * released when the view is garbage-collected, but that may be much later.
 *
 * ```ts
 * const view = decompressView(compressed);
 * try {
 *   parse(view.bytes);
 * } finally {
 *   view.free();
 * }
 * ```
 */
export function decompressView(data: Uint8Array): DecompressedView {
  ensureInitialized();
  return new DecompressedView(wasmDecompressToBuffer(data));
}

/** Decompressed bytes held in WebAssembly memory. See `decompressView`. */
export class DecompressedView {
  #buffer: DecompressedBuffer | undefined;
  readonly #ptr: number;

  /** Number of decompressed bytes. */
  readonly length: number;

  /** @internal */
  constructor(buffer: DecompressedBuffer) {
    this.#buffer = buffer;
    this.#ptr = buffer.ptr;
    this.length = buffer.len;
  }

  /**
   * The bytes, as a `Uint8Array` over WebAssembly memory. No copy is made.
   *
   * Read this again after any other call into the library instead of keeping the
   * array: WebAssembly memory can grow during a call, and growing it detaches
   * every existing view (they become empty). Don't use it after `free()`.
   */
  get bytes(): Uint8Array {
    if (this.#buffer === undefined) {
      throw new Error("DecompressedView was freed.");
    }

    return new Uint8Array(memory!.buffer, this.#ptr, this.length);
  }

  /** Copies the bytes into a normal `Uint8Array`, which stays valid after `free()`. */
  toUint8Array(): Uint8Array {
    return this.bytes.slice();
  }

  /** Whether `free()` has been called. */
  get freed(): boolean {
    return this.#buffer === undefined;
  }

  /** Releases the WebAssembly memory. Safe to call more than once. */
  free(): void {
    this.#buffer?.free();
    this.#buffer = undefined;
  }
}

export interface DecompressedView extends Disposable {}

// `using` support where the runtime has Symbol.dispose (Node 20+, recent browsers).
if (typeof Symbol.dispose === "symbol") {
  (DecompressedView.prototype as unknown as Record<symbol, () => void>)[Symbol.dispose] = DecompressedView.prototype.free;
}
