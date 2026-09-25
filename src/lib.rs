//! A DEFLATE ([RFC 1951]) decompressor written from scratch in Rust, with no
//! dependencies and no `unsafe` code.
//!
//! [`decompress`] takes a raw DEFLATE stream, and [`decompress_zlib`] a zlib
//! ([RFC 1950]) stream: DEFLATE with a 2-byte header and an Adler-32 checksum.
//! Stored, fixed Huffman and dynamic Huffman blocks are supported.
//!
//! ```
//! // "hello hello hello hello", compressed by zlib as raw DEFLATE.
//! let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
//!
//! let data = rust_deflate::decompress(&compressed).unwrap();
//!
//! assert_eq!(data, b"hello hello hello hello");
//! ```
//!
//! To decompress many streams, reuse a [`Decompressor`] instead: it keeps its
//! decoding tables between calls.
//!
//! [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
//! [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
#![forbid(unsafe_code)]

mod checksum;
mod compression;
mod error;
mod io;
mod zlib;

pub use error::Error;

use compression::inflater::Inflater;

/// Decompresses a raw DEFLATE stream.
///
/// Decoding stops at the end of the final block; any bytes after it are ignored.
/// To decompress many streams, a reused [`Decompressor`] avoids setting up its
/// decoding tables each time.
///
/// # Errors
///
/// Returns an [`Error`] saying what's wrong if the data is corrupt, truncated,
/// or not DEFLATE. Malformed input never panics.
///
/// ```
/// use rust_deflate::{decompress, Error};
///
/// assert_eq!(decompress(&[0x07]), Err(Error::InvalidBlockType));
/// assert_eq!(decompress(&[]), Err(Error::UnexpectedEndOfInput));
/// ```
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, Error> {
    Inflater::new().inflate(data)
}

/// Decompresses a zlib stream ([RFC 1950]): DEFLATE data with a 2-byte header and
/// an Adler-32 checksum of the decompressed data, as used by PNG and HTTP
/// `Content-Encoding: deflate`.
///
/// Bytes after the checksum are ignored.
///
/// # Errors
///
/// Returns an [`Error`] if the header is invalid or asks for a preset dictionary,
/// if the DEFLATE data is corrupt or truncated, or if the checksum doesn't match.
///
/// ```
/// // "hello hello hello hello", compressed by zlib (with header and checksum).
/// let compressed = [
///     0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1,
/// ];
///
/// assert_eq!(rust_deflate::decompress_zlib(&compressed).unwrap(), b"hello hello hello hello");
/// ```
///
/// [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
pub fn decompress_zlib(data: &[u8]) -> Result<Vec<u8>, Error> {
    zlib::inflate(&mut Inflater::new(), data)
}

/// A reusable DEFLATE decompressor for decompressing many streams.
///
/// It keeps its Huffman decoding tables (about 6 KB) and their allocations between
/// calls, so each stream after the first skips that setup. Streams don't affect
/// each other: every call starts fresh, even after a call that failed.
///
/// ```
/// use rust_deflate::Decompressor;
///
/// // "hello hello hello hello", compressed by zlib as raw DEFLATE.
/// let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
///
/// let mut decompressor = Decompressor::new();
/// for _ in 0..3 {
///     assert_eq!(decompressor.decompress(&compressed).unwrap(), b"hello hello hello hello");
/// }
/// ```
pub struct Decompressor {
    // Shared by the raw DEFLATE and zlib methods.
    inflater: Inflater,
}

impl Decompressor {
    /// Creates a decompressor.
    pub fn new() -> Self {
        Self {
            inflater: Inflater::new(),
        }
    }

    /// Decompresses a raw DEFLATE stream, like [`decompress`].
    ///
    /// Decoding stops at the end of the final block; any bytes after it are ignored.
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] saying what's wrong if the data is corrupt, truncated,
    /// or not DEFLATE. The decompressor can still be used afterwards.
    pub fn decompress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.inflater.inflate(data)
    }

    /// Decompresses a raw DEFLATE stream and appends it to `out`, returning the
    /// number of bytes appended.
    ///
    /// Reusing `out` between calls (with [`Vec::clear`] in between) also saves
    /// allocating an output buffer each time. What `out` already holds is left
    /// alone, and the stream can't refer back into it.
    ///
    /// ```
    /// use rust_deflate::Decompressor;
    ///
    /// // "hello hello hello hello", compressed by zlib as raw DEFLATE.
    /// let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
    ///
    /// let mut decompressor = Decompressor::new();
    /// let mut out = Vec::new();
    ///
    /// for _ in 0..3 {
    ///     out.clear();
    ///     let written = decompressor.decompress_into(&compressed, &mut out).unwrap();
    ///     assert_eq!(written, 23);
    ///     assert_eq!(out, b"hello hello hello hello");
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] saying what's wrong if the data is corrupt, truncated,
    /// or not DEFLATE. `out` is then truncated back to its original length, so it
    /// never ends up holding part of a stream.
    pub fn decompress_into(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<usize, Error> {
        Ok(self.inflater.inflate_into(data, out)?.output_added)
    }

    /// Decompresses a zlib stream, like [`decompress_zlib`].
    ///
    /// # Errors
    ///
    /// Same as [`decompress_zlib`]. The decompressor can still be used afterwards.
    pub fn decompress_zlib(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        zlib::inflate(&mut self.inflater, data)
    }

    /// Decompresses a zlib stream and appends it to `out`, returning the number of
    /// bytes appended. Works like [`Decompressor::decompress_into`]; the checksum
    /// covers only the bytes this call appends.
    ///
    /// # Errors
    ///
    /// Same as [`decompress_zlib`]. `out` is then truncated back to its original
    /// length, so it never ends up holding part of a stream.
    pub fn decompress_zlib_into(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<usize, Error> {
        zlib::inflate_into(&mut self.inflater, data, out)
    }
}

impl Default for Decompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Decompressor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decompressor").finish_non_exhaustive()
    }
}

// Runs the README's code examples as doctests so they can't go stale.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
