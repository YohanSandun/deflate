// Node.js smoke test for the built package: `npm run build`, then `npm run test:node`.
// Compresses inputs with node:zlib (raw deflate) and checks decompress() gives them back.
import { deflateRawSync, constants } from "node:zlib";
import { readFile } from "node:fs/promises";
import { init, decompress } from "../../dist/index.js";

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
  if (Buffer.compare(Buffer.from(output), Buffer.from(input)) === 0) {
    console.log(`ok   ${name} (${input.length} -> ${compressed.length} bytes, ${ms} ms)`);
  } else {
    failures++;
    console.log(`FAIL ${name}: output differs (${output.length} vs ${input.length} bytes)`);
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

if (failures > 0) {
  console.log(`\n${failures} failed`);
  process.exit(1);
}
console.log("\nall passed");
