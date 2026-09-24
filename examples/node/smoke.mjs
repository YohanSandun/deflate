// Node.js smoke test for the built package: `npm run build`, then `npm run test:node`.
// Compresses inputs with node:zlib (raw deflate) and checks decompress() and
// decompressView() give them back.
import { deflateRawSync, constants } from "node:zlib";
import { readFile } from "node:fs/promises";
import { init, decompress, decompressView } from "../../dist/index.js";

await init();

let failures = 0;

function check(name, input, options) {
  const compressed = deflateRawSync(input, options);
  const start = performance.now();
  let output;

  try {
    output = decompress(compressed);
  } catch (error) {
    failures++;
    console.log(`FAIL ${name}: threw ${error.message}`);
    return;
  }

  const ms = (performance.now() - start).toFixed(2);
  const view = decompressView(compressed);
  const viewMatches = view.length === input.length && equal(view.bytes, input);
  view.free();

  if (equal(output, input) && viewMatches) {
    console.log(`ok   ${name} (${input.length} -> ${compressed.length} bytes, ${ms} ms)`);
  } else {
    failures++;
    console.log(`FAIL ${name}: output differs (copy ${equal(output, input) ? "ok" : "wrong"}, view ${viewMatches ? "ok" : "wrong"})`);
  }
}

function equal(a, b) {
  return Buffer.compare(Buffer.from(a.buffer, a.byteOffset, a.length), Buffer.from(b.buffer, b.byteOffset, b.length)) === 0;
}

function expect(name, condition) {
  if (condition) {
    console.log(`ok   ${name}`);
  } else {
    failures++;
    console.log(`FAIL ${name}`);
  }
}

function throws(fn) {
  try {
    fn();
    return false;
  } catch {
    return true;
  }
}

const text = await readFile(new URL("../../tests/data/dynamic_text.txt", import.meta.url));
const random = new Uint8Array(200_000).map(() => Math.floor(Math.random() * 256));
const runs = new Uint8Array(100_000).fill(0x61);

check("empty", new Uint8Array(0));
check("single byte", new Uint8Array([42]));
for (const level of [0, 1, 6, 9]) {
  check(`text, level ${level}`, text, { level });
}
check("text, fixed Huffman", text, { strategy: constants.Z_FIXED });
check("random bytes", random);
check("100 KB run of one byte", runs);
check("text x20 (~2 MB)", Buffer.concat(Array(20).fill(text)));

try {
  decompress(new Uint8Array([0x07]));
  failures++;
  console.log("FAIL corrupt input: did not throw");
} catch (error) {
  console.log(`ok   corrupt input throws: "${error.message}"`);
}

// decompressView specifics.
{
  const small = deflateRawSync(text);
  const view = decompressView(small);
  const before = view.bytes;

  // Decompressing ~50 MB makes wasm memory grow, which detaches `before`.
  const huge = deflateRawSync(Buffer.concat(Array(450).fill(text)));
  decompressView(huge).free();

  expect("view: memory growth detaches old arrays", before.length === 0);
  expect("view: .bytes is valid again after memory growth", equal(view.bytes, text));
  expect("view: toUint8Array copy survives free()", (() => {
    const copy = view.toUint8Array();
    view.free();
    return equal(copy, text) && view.freed;
  })());
  expect("view: .bytes throws after free()", throws(() => view.bytes));
  expect("view: free() twice is safe", !throws(() => view.free()));
  expect("view: corrupt input throws", throws(() => decompressView(new Uint8Array([0x07]))));
  expect("view: empty output", (() => {
    const empty = decompressView(deflateRawSync(new Uint8Array(0)));
    const ok = empty.length === 0 && empty.bytes.length === 0;
    empty.free();
    return ok;
  })());
  expect("view: Symbol.dispose frees it", (() => {
    const disposable = decompressView(small);
    disposable[Symbol.dispose]();
    return disposable.freed;
  })());
}

if (failures > 0) {
  console.log(`\n${failures} failed`);
  process.exit(1);
}
console.log("\nall passed");
