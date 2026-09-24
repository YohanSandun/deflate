# rust-deflate

A DEFLATE ([RFC 1951]) decompressor written from scratch in Rust.

- No dependencies and no `unsafe` code (`#![forbid(unsafe_code)]`).
- Supports all three block types: stored, fixed Huffman and dynamic Huffman.
- Malformed input returns an error; it never panics.
- Builds for `wasm32-unknown-unknown`, so it can sit behind a WebAssembly/JS wrapper.

## Installation

```sh
cargo add rust-deflate
```

or in `Cargo.toml`:

```toml
[dependencies]
rust-deflate = "0.1"
```

The crate is imported as `rust_deflate`.

## Usage

```rust
// "hello hello hello hello", compressed by zlib as raw DEFLATE.
let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];

let data = rust_deflate::decompress(&compressed).unwrap();

assert_eq!(data, b"hello hello hello hello");
```

`decompress` takes a raw DEFLATE stream and returns the decompressed bytes, or a
`rust_deflate::Error` saying why the data is corrupt, truncated or not DEFLATE.
`Error` implements `std::error::Error` and `Display`, so it works with `?`:

```rust
use rust_deflate::{decompress, Error};

match decompress(&[0x07]) {
    Ok(data) => println!("{} bytes", data.len()),
    Err(Error::UnexpectedEndOfInput) => eprintln!("the data is truncated"),
    Err(error) => eprintln!("corrupt data: {error}"), // "invalid flate block type"
}
```

`Error` is `#[non_exhaustive]`: new variants may be added in minor releases, so
keep a catch-all arm when matching on it.

### Many streams

To decompress many streams, reuse one `Decompressor`. It keeps its decoding tables
between calls, which makes small streams (a few KB or less) noticeably faster.
Each call is independent, even after one that failed.

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

## Input format

The input must be **raw DEFLATE**, with no zlib ([RFC 1950]) or gzip ([RFC 1952])
wrapper. That's what you get from, for example:

- Node.js: `zlib.deflateRawSync(data)`
- Browsers: `new CompressionStream("deflate-raw")`
- Python: `zlib.compressobj(wbits=-15)`

Decoding stops at the end of the final DEFLATE block; any bytes after it are ignored.

zlib and gzip data (including PNG image data) wrap the DEFLATE stream in a header
and a checksum. Support for those formats isn't in this version yet.

## Not supported yet

- zlib and gzip wrappers
- Compression
- Streaming (the whole input must be in memory, and the output is returned in one piece)

## Minimum supported Rust version

Rust 1.85 (edition 2024).

## License

[MIT](LICENSE)

[RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
[RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
[RFC 1952]: https://www.rfc-editor.org/rfc/rfc1952
