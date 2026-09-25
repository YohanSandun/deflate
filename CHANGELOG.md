# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 09/25/2026

### Added

- zlib ([RFC 1950]) support: `decompress_zlib`, `Decompressor::decompress_zlib` and
  `Decompressor::decompress_zlib_into`. The header is validated (compression
  method, window size and header check) and the Adler-32 checksum is verified.
  Streams that need a preset dictionary are rejected.
- `Decompressor`, a reusable decompressor that keeps its decoding tables between
  calls. It makes decompressing many small streams faster, and one `Decompressor`
  handles both raw DEFLATE and zlib.
- `Decompressor::decompress_into` (and `decompress_zlib_into`), which append to a
  caller-provided `Vec<u8>` so the output buffer can be reused as well. They return
  the number of bytes added, truncate the `Vec` back to its original length on
  error, and never let a stream refer back into bytes that were already in it.
- `_with` variants of every function and method (`decompress_with`,
  `decompress_zlib_with`, and on `Decompressor` also `decompress_into_with` and
  `decompress_zlib_into_with`), which take `OutputOptions`; the versions without
  `_with` use the defaults. `max_output` fails with the new
  `Error::OutputLimitExceeded` as soon as a stream would exceed a size limit
  (protecting against decompression bombs), `size_hint` allocates the output buffer
  up front, and `exact` sets both.
- Streaming: `ZlibDecoder` and `DeflateDecoder` wrap any `std::io::Read` source and
  implement `Read`, decompressing data of any size in constant memory (under 400 KB
  of buffers).
- `StreamDecompressor`, a push-based streaming decompressor for raw DEFLATE and
  zlib: `push` chunks of any size as they arrive and get back everything they
  decompress to (output is never held back waiting for more input), `finish` at
  the end, `reset` to reuse it.
- `impl From<Error> for std::io::Error`.
- New `Error` variants for zlib: `UnsupportedCompressionMethod`,
  `InvalidWindowSize`, `InvalidFcheck`, `PresetDictionary` and `ChecksumMismatch`.

### Changed

- Faster decoding: the main loop now decodes two short literals per table lookup
  where they fit, and looks up the next symbol ahead of time so that work overlaps
  with writing output and copying matches.

## [0.1.0] - 2026-09-24

### Added

- Initial release: a raw DEFLATE ([RFC 1951]) decompressor, `decompress`, supporting
  stored, fixed Huffman and dynamic Huffman blocks.
- `Error`, a typed error enum describing why a stream couldn't be decompressed.
  Malformed input returns an error and never panics.
- No dependencies and no `unsafe` code.

[0.2.0]: https://github.com/YohanSandun/deflate/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/YohanSandun/deflate/releases/tag/v0.1.0
[RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
[RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
