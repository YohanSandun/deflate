# rust-deflate

A DEFLATE ([RFC 1951]) and zlib ([RFC 1950]) compressor and decompressor written from
scratch in Rust.

- No dependencies and no `unsafe` code (`#![forbid(unsafe_code)]`).
- Reads and writes all three block types: stored, fixed Huffman and dynamic Huffman.
- Decodes raw DEFLATE, and zlib streams with their Adler-32 checksum verified.
- Compresses at zlib's levels 0 to 9, splitting the input into blocks and choosing
  the smallest type for each. On typical text and binary data the output is a little
  smaller than zlib's at the same level.
- Streams data of any size in constant memory, both ways: through `std::io::Read` and
  `std::io::Write`, or by pushing chunks in as they arrive.
- Malformed input returns an error; it never panics.
- Builds for `wasm32`, so it can sit behind a WebAssembly/JS wrapper.

## Installation

```sh
cargo add rust-deflate
```

or in `Cargo.toml`:

```toml
[dependencies]
rust-deflate = "0.4"
```

The crate is imported as `rust_deflate`.

## Usage

```rust
let data = b"hello hello hello hello";

let compressed = rust_deflate::compress(data);
assert_eq!(rust_deflate::decompress(&compressed).unwrap(), data);
```

## Decompressing

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

### Streaming large data

For data too large to hold in memory, `ZlibDecoder` and `DeflateDecoder` wrap any
`std::io::Read` source and implement `Read` themselves. They read compressed input as
needed and keep under 400 KB of buffers however large the stream is, so a
multi-gigabyte file decompresses in constant memory:

```rust,no_run
use std::fs::File;
use rust_deflate::ZlibDecoder;

let mut decoder = ZlibDecoder::new(File::open("huge.zz")?);
std::io::copy(&mut decoder, &mut File::create("huge.bin")?)?;
# Ok::<(), std::io::Error>(())
```

Errors come back as `std::io::Error`: `UnexpectedEof` for truncated data and
`InvalidData` for corrupt data, with the `rust_deflate::Error` inside
(`error.get_ref()` and `downcast_ref`). A zlib checksum can only be checked at the
end, so treat streamed output as untrusted until the last read returns 0. The
decoders may read past the end of the compressed stream from their source.

### Pushing chunks as they arrive

When the data doesn't come from a `Read` source (network callbacks, a
WebAssembly/JS wrapper), push it into a `StreamDecompressor` instead. Each `push`
appends everything its chunk lets the decoder produce, so output is never held back
waiting for more input: a zlib stream split by sync flushes (as used by WebSocket
compression) gives each message's output as soon as its bytes are pushed.

```rust
use rust_deflate::StreamDecompressor;

# let chunks: Vec<Vec<u8>> = vec![vec![0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57], vec![0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1]];
let mut stream = StreamDecompressor::zlib(); // or ::deflate()
let mut out = Vec::new();

for chunk in &chunks {
    stream.push(chunk, &mut out)?;
    // use `out`, then out.clear() to keep memory flat
}
stream.finish(&mut out)?; // no more input: fails if the stream is incomplete
# Ok::<(), rust_deflate::Error>(())
```

`reset()` starts a new stream reusing the same buffers. Errors are
`rust_deflate::Error`; after one, every call returns it until `reset()`.

## Compressing

`compress` and `compress_zlib` compress at the default level,
`CompressionLevel::MEDIUM` (6). The `_with` versions take a level, from
`CompressionLevel::NONE` (0, stored as is) through `FAST` (1) to `BEST` (9), or
`CompressionLevel::new(n)` for any level in between:

```rust
use rust_deflate::{compress, compress_zlib_with, decompress, decompress_zlib, CompressionLevel};

let data = b"hello hello hello hello";

let compressed = compress(data);
assert_eq!(decompress(&compressed)?, data);

let smallest = compress_zlib_with(data, CompressionLevel::BEST);
assert_eq!(decompress_zlib(&smallest)?, data);
# Ok::<(), rust_deflate::Error>(())
```

To compress many inputs, reuse one `Compressor`: it keeps its tables between calls.
`compress_into` and `compress_zlib_into` append to a buffer you pass in and return
how many bytes they added:

```rust
use rust_deflate::{CompressionLevel, Compressor};

# let inputs: Vec<Vec<u8>> = vec![b"first first first".to_vec(), b"second second".to_vec()];
let mut compressor = Compressor::new();
let mut out = Vec::new();

for data in &inputs {
    out.clear();
    compressor.compress_zlib_into_with(data, &mut out, CompressionLevel::FAST);
    // use `out`
}
```

Higher levels search harder for repeated data: `FAST` is about three times quicker than
`MEDIUM`, and `BEST` also searches for the best places to split the input into
blocks, which costs more time again. Compression can't fail, so these functions
return the bytes directly.

Every `_with` function and method takes either a `CompressionLevel` or a full
`CompressionOptions`, which also chooses the block type with a `Strategy`: `Dynamic`
(the default) splits the input into blocks and writes each as whichever type is
smallest, while `Fixed` and `Stored` force one type. At `CompressionLevel::NONE`
every strategy writes stored blocks, as in zlib.

```rust
use rust_deflate::{compress_with, CompressionLevel, CompressionOptions, Strategy};

let options = CompressionOptions::new()
    .strategy(Strategy::Fixed)
    .level(CompressionLevel::new(4));
let compressed = compress_with(b"hello hello hello hello", options);
```

### Streaming compression

To compress data of any size in constant memory, write it through a `ZlibEncoder` or
`DeflateEncoder`, which wrap any `std::io::Write`. `finish` writes the end of the
stream and returns the writer:

```rust,no_run
use std::fs::File;
use rust_deflate::{CompressionLevel, ZlibEncoder};

let mut encoder = ZlibEncoder::with_options(File::create("archive.zz")?, CompressionLevel::BEST);
std::io::copy(&mut File::open("archive.bin")?, &mut encoder)?;
encoder.finish()?;
# Ok::<(), std::io::Error>(())
```

When data is produced in pieces instead, push it into a `StreamCompressor`:

```rust
use rust_deflate::{decompress_zlib, StreamCompressor};

let mut stream = StreamCompressor::zlib();
let mut compressed = Vec::new();

for chunk in [&b"hello "[..], b"hello ", b"hello"] {
    stream.push(chunk, &mut compressed);
}
stream.finish(&mut compressed);

assert_eq!(decompress_zlib(&compressed)?, b"hello hello hello");
# Ok::<(), rust_deflate::Error>(())
```

Both compress 256 KB at a time (1 MB at levels 8 and 9), so output comes in bursts.
`flush` (`Write::flush` on the encoders) writes out everything so far without ending
the stream, like zlib's `Z_SYNC_FLUSH`, so the other side of a connection can decode
it right away.

## Formats

### Raw DEFLATE: `decompress` and `compress`

A bare DEFLATE stream with no header or checksum. That's what you get from, for
example:

- Node.js: `zlib.deflateRawSync(data)`
- Browsers: `new CompressionStream("deflate-raw")`
- Python: `zlib.compressobj(wbits=-15)`

Decoding stops at the end of the final DEFLATE block; any bytes after it are ignored.
`compress` writes the same format, which the matching decoders read:
`zlib.inflateRawSync` in Node.js, `new DecompressionStream("deflate-raw")` in
browsers, and `zlib.decompressobj(wbits=-15)` in Python.

### zlib: `decompress_zlib` and `compress_zlib`

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

`compress_zlib` writes the same format, which `zlib.inflateSync` (Node.js),
`new DecompressionStream("deflate")` (browsers) and `zlib.decompress` (Python) read.
The header records the level in its FLEVEL field, as zlib does.

## Not supported yet

- gzip ([RFC 1952])
- zlib preset dictionaries

## Minimum supported Rust version

Rust 1.85 (edition 2024).

## License

[MIT](LICENSE)

[RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
[RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
[RFC 1952]: https://www.rfc-editor.org/rfc/rfc1952
