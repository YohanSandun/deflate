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
//! To decompress data too large to hold in memory, such as a multi-gigabyte file,
//! wrap any [`std::io::Read`] source in a [`ZlibDecoder`] or [`DeflateDecoder`].
//! They decompress in constant memory. When data arrives in pieces instead (network
//! callbacks, a WebAssembly wrapper), push it into a [`StreamDecompressor`].
//!
//! [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
//! [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
#![forbid(unsafe_code)]

mod checksum;
mod compression;
mod decoder;
mod error;
mod io;
mod options;
mod stream;
mod stream_decompressor;
mod zlib;

pub use decoder::{DeflateDecoder, ZlibDecoder};
pub use error::Error;
pub use options::OutputOptions;
pub use stream_decompressor::StreamDecompressor;

use compression::inflater::Inflater;

/// Decompresses a raw DEFLATE stream.
///
/// Decoding stops at the end of the final block; any bytes after it are ignored.
/// To decompress many streams, a reused [`Decompressor`] avoids setting up its
/// decoding tables each time. To limit the output size, use [`decompress_with`].
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
    decompress_with(data, OutputOptions::new())
}

/// Like [`decompress`], with [`OutputOptions`] to cap the output size and to say
/// how much to allocate up front.
///
/// ```
/// use rust_deflate::{decompress_with, Error, OutputOptions};
///
/// // "hello hello hello hello" (23 bytes), compressed by zlib as raw DEFLATE.
/// let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
///
/// assert_eq!(decompress_with(&compressed, OutputOptions::exact(23)).unwrap(), b"hello hello hello hello");
/// assert_eq!(
///     decompress_with(&compressed, OutputOptions::new().max_output(10)),
///     Err(Error::OutputLimitExceeded)
/// );
/// ```
///
/// # Errors
///
/// Same as [`decompress`], plus [`Error::OutputLimitExceeded`] if the stream
/// decompresses to more than `options` allows.
pub fn decompress_with(data: &[u8], options: OutputOptions) -> Result<Vec<u8>, Error> {
    Decompressor::new().decompress_with(data, options)
}

/// Decompresses a zlib stream ([RFC 1950]): DEFLATE data with a 2-byte header and
/// an Adler-32 checksum of the decompressed data, as used by PNG and HTTP
/// `Content-Encoding: deflate`.
///
/// Bytes after the checksum are ignored. To limit the output size, use
/// [`decompress_zlib_with`].
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
    decompress_zlib_with(data, OutputOptions::new())
}

/// Like [`decompress_zlib`], with [`OutputOptions`] to cap the output size and to
/// say how much to allocate up front.
///
/// ```
/// use rust_deflate::{decompress_zlib_with, Error, OutputOptions};
///
/// // "hello hello hello hello" (23 bytes), compressed by zlib.
/// let compressed = [
///     0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1,
/// ];
///
/// assert_eq!(
///     decompress_zlib_with(&compressed, OutputOptions::new().max_output(10)),
///     Err(Error::OutputLimitExceeded)
/// );
/// ```
///
/// # Errors
///
/// Same as [`decompress_zlib`], plus [`Error::OutputLimitExceeded`] if the stream
/// decompresses to more than `options` allows.
pub fn decompress_zlib_with(data: &[u8], options: OutputOptions) -> Result<Vec<u8>, Error> {
    Decompressor::new().decompress_zlib_with(data, options)
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
        self.decompress_with(data, OutputOptions::new())
    }

    /// Like [`Decompressor::decompress`], with [`OutputOptions`], like
    /// [`decompress_with`].
    ///
    /// # Errors
    ///
    /// Same as [`decompress_with`]. The decompressor can still be used afterwards.
    pub fn decompress_with(
        &mut self,
        data: &[u8],
        options: OutputOptions,
    ) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.decompress_into_with(data, &mut out, options)?;
        Ok(out)
    }

    /// Decompresses a raw DEFLATE stream and appends it to `out`, returning the
    /// number of bytes appended.
    ///
    /// Reusing `out` between calls (with [`Vec::clear`] in between) also saves
    /// allocating an output buffer each time. What `out` already holds is left
    /// alone, and the stream can't refer back into it.
    ///
    /// To limit the output size or allocate it up front, use
    /// [`Decompressor::decompress_into_with`].
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
        self.decompress_into_with(data, out, OutputOptions::new())
    }

    /// Like [`Decompressor::decompress_into`], with [`OutputOptions`] to cap how much
    /// this call may produce and to say how much to allocate up front.
    ///
    /// ```
    /// use rust_deflate::{Decompressor, Error, OutputOptions};
    ///
    /// // "hello hello hello hello" (23 bytes), compressed by zlib as raw DEFLATE.
    /// let compressed = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
    ///
    /// let mut decompressor = Decompressor::new();
    /// let mut out = Vec::new();
    ///
    /// let result = decompressor.decompress_into_with(&compressed, &mut out, OutputOptions::new().max_output(10));
    /// assert_eq!(result, Err(Error::OutputLimitExceeded));
    /// ```
    ///
    /// # Errors
    ///
    /// Same as [`Decompressor::decompress_into`], plus [`Error::OutputLimitExceeded`]
    /// if the stream decompresses to more than `options` allows.
    pub fn decompress_into_with(
        &mut self,
        data: &[u8],
        out: &mut Vec<u8>,
        options: OutputOptions,
    ) -> Result<usize, Error> {
        Ok(self
            .inflater
            .inflate_into(data, out, options.to_output_size())?
            .output_added)
    }

    /// Decompresses a zlib stream, like [`decompress_zlib`].
    ///
    /// # Errors
    ///
    /// Same as [`decompress_zlib`]. The decompressor can still be used afterwards.
    pub fn decompress_zlib(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.decompress_zlib_with(data, OutputOptions::new())
    }

    /// Like [`Decompressor::decompress_zlib`], with [`OutputOptions`], like
    /// [`decompress_zlib_with`].
    ///
    /// # Errors
    ///
    /// Same as [`decompress_zlib_with`]. The decompressor can still be used afterwards.
    pub fn decompress_zlib_with(
        &mut self,
        data: &[u8],
        options: OutputOptions,
    ) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.decompress_zlib_into_with(data, &mut out, options)?;
        Ok(out)
    }

    /// Decompresses a zlib stream and appends it to `out`, returning the number of
    /// bytes appended. Works like [`Decompressor::decompress_into`]; the checksum
    /// covers only the bytes this call appends.
    ///
    /// To limit the output size or allocate it up front, use
    /// [`Decompressor::decompress_zlib_into_with`].
    ///
    /// # Errors
    ///
    /// Same as [`decompress_zlib`]. `out` is then truncated back to its original
    /// length, so it never ends up holding part of a stream.
    pub fn decompress_zlib_into(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<usize, Error> {
        self.decompress_zlib_into_with(data, out, OutputOptions::new())
    }

    /// Like [`Decompressor::decompress_zlib_into`], with [`OutputOptions`] to cap how
    /// much this call may produce and to say how much to allocate up front.
    ///
    /// ```
    /// use rust_deflate::{Decompressor, Error, OutputOptions};
    ///
    /// // "hello hello hello hello" (23 bytes), compressed by zlib.
    /// let compressed = [
    ///     0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1,
    /// ];
    ///
    /// let mut decompressor = Decompressor::new();
    /// let mut out = Vec::new();
    ///
    /// // Exactly the expected size: allocated once, and more would be an error.
    /// decompressor.decompress_zlib_into_with(&compressed, &mut out, OutputOptions::exact(23))?;
    /// assert_eq!(out, b"hello hello hello hello");
    ///
    /// // A smaller limit is rejected, and `out` is left as it was.
    /// out.clear();
    /// let result = decompressor.decompress_zlib_into_with(&compressed, &mut out, OutputOptions::new().max_output(10));
    /// assert_eq!(result, Err(Error::OutputLimitExceeded));
    /// assert!(out.is_empty());
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Same as [`Decompressor::decompress_zlib_into`], plus
    /// [`Error::OutputLimitExceeded`] if the stream decompresses to more than
    /// `options` allows.
    pub fn decompress_zlib_into_with(
        &mut self,
        data: &[u8],
        out: &mut Vec<u8>,
        options: OutputOptions,
    ) -> Result<usize, Error> {
        zlib::inflate_into(&mut self.inflater, data, out, options.to_output_size())
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
