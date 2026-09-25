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
