# rust-deflate

A DEFLATE ([RFC 1951]) and zlib ([RFC 1950]) decompressor written from scratch in Rust.

- No dependencies and no `unsafe` code (`#![forbid(unsafe_code)]`).
- Supports all three block types: stored, fixed Huffman and dynamic Huffman.
- Decodes raw DEFLATE, and zlib streams with their Adler-32 checksum verified.
- Malformed input returns an error; it never panics.
- Builds for `wasm32-unknown-unknown`, so it can sit behind a WebAssembly/JS wrapper.

## Installation

```sh
cargo add rust-deflate
```

or in `Cargo.toml`:

```toml
[dependencies]
rust-deflate = "0.2"
```

The crate is imported as `rust_deflate`.

## Usage

```rust
// "hello hello hello hello", compressed by zlib as raw DEFLATE.
let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];

let data = rust_deflate::decompress(&compressed).unwrap();

assert_eq!(data, b"hello hello hello hello");
```

For zlib data (a 2-byte header, DEFLATE, then an Adler-32 checksum), use
`decompress_zlib`:

```rust
// The same text, compressed by zlib with its header and checksum.
let compressed = [
    0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1,
];

let data = rust_deflate::decompress_zlib(&compressed).unwrap();

assert_eq!(data, b"hello hello hello hello");
```

Both return the decompressed bytes, or a `rust_deflate::Error` saying why the data
is corrupt, truncated or in the wrong format. `Error` implements `std::error::Error`
and `Display`, so it works with `?`:

```rust
use rust_deflate::{decompress, decompress_zlib, Error};

match decompress(&[0x07]) {
    Ok(data) => println!("{} bytes", data.len()),
    Err(Error::UnexpectedEndOfInput) => eprintln!("the data is truncated"),
    Err(error) => eprintln!("corrupt data: {error}"), // "invalid flate block type"
}

// zlib data whose checksum doesn't match what it decompresses to.
let tampered = [
    0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB0,
];
assert_eq!(decompress_zlib(&tampered), Err(Error::ChecksumMismatch));
```

`Error` is `#[non_exhaustive]`: new variants may be added in minor releases, so
keep a catch-all arm when matching on it.

### Many streams

To decompress many streams, reuse one `Decompressor`. It keeps its decoding tables
between calls, which makes small streams (a few KB or less) noticeably faster.
Each call is independent, even after one that failed, and the same `Decompressor`
handles both formats (`decompress` and `decompress_zlib`).

```rust
use rust_deflate::Decompressor;

# let streams: Vec<Vec<u8>> = vec![vec![0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01]];
let mut decompressor = Decompressor::new();

for compressed in &streams {
    let data = decompressor.decompress(compressed)?;
    // ...
}
# Ok::<(), rust_deflate::Error>(())
```

`decompress_into` and `decompress_zlib_into` also reuse the output buffer. They
append to a `Vec<u8>` you pass in and return the number of bytes added; on error,
the `Vec` is truncated back to its original length. A stream can't refer back into
bytes that were already there, and a zlib checksum covers only the bytes that call
added.

```rust
use rust_deflate::Decompressor;

# let streams: Vec<Vec<u8>> = vec![vec![0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1]];
let mut decompressor = Decompressor::new();
let mut out = Vec::new();

for compressed in &streams {
    out.clear();
    decompressor.decompress_zlib_into(compressed, &mut out)?;
    // use `out`...
}
# Ok::<(), rust_deflate::Error>(())
```

### Limiting and sizing the output

Every function and method has a `_with` variant that also takes an
`OutputOptions`: `decompress_with` and `decompress_zlib_with` (as functions and on
`Decompressor`), and `decompress_into_with` and `decompress_zlib_into_with`.
Without `_with`, there's no limit and the buffer size is guessed from the input.

- `max_output(n)` fails with `Error::OutputLimitExceeded` as soon as a stream would
  decompress to more than `n` bytes. DEFLATE can expand about 1000 times, so set a
  limit when the input isn't trusted: the output buffer is never grown much past it,
  so a small malicious file can't make you allocate gigabytes.
- `size_hint(n)` allocates room for `n` bytes up front, so a buffer of the right
  size is allocated once and never grows.
- `OutputOptions::exact(n)` sets both, for when the size is known in advance, like
  a PNG image's size from its header.

```rust
use rust_deflate::{Decompressor, Error, OutputOptions};

let compressed = [
    0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1,
];
let mut decompressor = Decompressor::new();
let mut out = Vec::new();

// The 23-byte result fits the exact size.
decompressor.decompress_zlib_into_with(&compressed, &mut out, OutputOptions::exact(23))?;

// A 10-byte limit is too small: an error, and `out` is left as it was.
let mut small = Vec::new();
let result = decompressor.decompress_zlib_into_with(&compressed, &mut small, OutputOptions::new().max_output(10));
assert_eq!(result, Err(Error::OutputLimitExceeded));
assert!(small.is_empty());
# Ok::<(), Error>(())
```

For one-off untrusted input, the functions work the same way:

```rust
use rust_deflate::{decompress_zlib_with, OutputOptions};

# let untrusted = [0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1];
let data = decompress_zlib_with(&untrusted, OutputOptions::new().max_output(16 << 20))?;
# Ok::<(), rust_deflate::Error>(())
```

The size hint is allocated before any data is decoded, so cap it if it comes from
untrusted input (a limit caps it too).

## Input formats

### Raw DEFLATE: `decompress`

A bare DEFLATE stream with no header or checksum. That's what you get from, for
example:

- Node.js: `zlib.deflateRawSync(data)`
- Browsers: `new CompressionStream("deflate-raw")`
- Python: `zlib.compressobj(wbits=-15)`

Decoding stops at the end of the final DEFLATE block; any bytes after it are ignored.

### zlib: `decompress_zlib`

A 2-byte header, a DEFLATE stream, then a big-endian Adler-32 checksum of the
decompressed data. That's what you get from, for example:

- Node.js: `zlib.deflateSync(data)`
- Browsers: `new CompressionStream("deflate")`
- Python: `zlib.compress(data)`

zlib is also the format of PNG image data (the `IDAT` chunks, concatenated) and of
most HTTP `Content-Encoding: deflate` responses.

The header is validated (compression method, window size and header check), and the
checksum is verified against the decompressed data. Streams that need a preset
dictionary are rejected with `Error::PresetDictionary`. Any bytes after the checksum
are ignored.

## Not supported yet

- gzip ([RFC 1952])
- zlib preset dictionaries
- Compression
- Streaming (the whole input must be in memory, and the output is returned in one piece)

## Minimum supported Rust version

Rust 1.85 (edition 2024).

## License

[MIT](LICENSE)

[RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
[RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
[RFC 1952]: https://www.rfc-editor.org/rfc/rfc1952
