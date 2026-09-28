//! A DEFLATE ([RFC 1951]) and zlib ([RFC 1950]) compressor and decompressor written
//! from scratch in Rust, with no dependencies and no `unsafe` code.
//!
//! ```
//! let data = b"hello hello hello hello";
//!
//! let compressed = rust_deflate::compress(data);
//! assert_eq!(rust_deflate::decompress(&compressed).unwrap(), data);
//! ```
//!
//! # Decompressing
//!
//! [`decompress`] takes a raw DEFLATE stream, and [`decompress_zlib`] a zlib stream:
//! DEFLATE with a 2-byte header and an Adler-32 checksum. Stored, fixed Huffman and
//! dynamic Huffman blocks are supported, and malformed input returns an [`Error`].
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
//! decoding tables between calls. To cap the output size of untrusted input, pass
//! [`OutputOptions`] to the `_with` functions.
//!
//! To decompress data too large to hold in memory, such as a multi-gigabyte file,
//! wrap any [`std::io::Read`] source in a [`ZlibDecoder`] or [`DeflateDecoder`].
//! They decompress in constant memory. When data arrives in pieces instead (network
//! callbacks, a WebAssembly wrapper), push it into a [`StreamDecompressor`].
//!
//! # Compressing
//!
//! [`compress`] and [`compress_zlib`] go the other way, at
//! [`CompressionLevel::MEDIUM`]. [`compress_with`] and [`compress_zlib_with`] take a
//! [`CompressionLevel`], from [`CompressionLevel::NONE`] to
//! [`CompressionLevel::BEST`], or [`CompressionOptions`] to also choose the block
//! [`Strategy`].
//!
//! ```
//! use rust_deflate::{compress_with, decompress, CompressionLevel};
//!
//! let data = b"hello hello hello hello";
//! let compressed = compress_with(data, CompressionLevel::BEST);
//! assert_eq!(decompress(&compressed).unwrap(), data);
//! ```
//!
//! To compress many inputs, reuse a [`Compressor`], which can also append to a
//! buffer you pass in. To compress data of any size in constant memory, write it
//! through a [`ZlibEncoder`] or [`DeflateEncoder`], or push it into a
//! [`StreamCompressor`].
//!
//! [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
//! [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
#![forbid(unsafe_code)]

mod checksum;
mod compress_stream;
mod compression;
mod decoder;
mod encoder;
mod error;
mod io;
mod options;
mod stream;
mod stream_compressor;
mod stream_decompressor;
mod zlib;

pub use decoder::{DeflateDecoder, ZlibDecoder};
pub use encoder::{DeflateEncoder, ZlibEncoder};
pub use error::Error;
pub use options::{CompressionLevel, CompressionOptions, OutputOptions, Strategy};
pub use stream_compressor::StreamCompressor;
pub use stream_decompressor::StreamDecompressor;

use compression::deflater::{self, Deflater};
use compression::inflater::Inflater;
use io::bit_writer::BitWriter;

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

/// Compresses `data` into a raw DEFLATE stream (RFC 1951) at
/// [`CompressionLevel::MEDIUM`], the reverse of [`decompress`].
///
/// To choose the level, use [`compress_with`]. To compress many inputs, a reused
/// [`Compressor`] keeps its match-finding tables between calls.
///
/// ```
/// let data = b"hello hello hello hello";
/// assert_eq!(rust_deflate::decompress(&rust_deflate::compress(data)).unwrap(), data);
/// ```
pub fn compress(data: &[u8]) -> Vec<u8> {
    compress_with(data, CompressionOptions::new())
}

/// Like [`compress`], at the given [`CompressionLevel`], or with
/// [`CompressionOptions`] to also choose the block type.
///
/// ```
/// use rust_deflate::{compress_with, decompress, CompressionLevel};
///
/// let data = b"hello hello hello hello";
/// for level in [CompressionLevel::NONE, CompressionLevel::FAST, CompressionLevel::BEST] {
///     assert_eq!(decompress(&compress_with(data, level)).unwrap(), data);
/// }
/// ```
pub fn compress_with(data: &[u8], options: impl Into<CompressionOptions>) -> Vec<u8> {
    Compressor::new().compress_with(data, options)
}

/// Compresses `data` into a zlib stream (RFC 1950) at
/// [`CompressionLevel::MEDIUM`], the reverse of [`decompress_zlib`]: a 2-byte
/// header, raw DEFLATE, then an Adler-32 checksum.
///
/// ```
/// let data = b"hello hello hello hello";
/// assert_eq!(rust_deflate::decompress_zlib(&rust_deflate::compress_zlib(data)).unwrap(), data);
/// ```
pub fn compress_zlib(data: &[u8]) -> Vec<u8> {
    compress_zlib_with(data, CompressionOptions::new())
}

/// Like [`compress_zlib`], at the given [`CompressionLevel`], or with
/// [`CompressionOptions`] to also choose the block type. The level is also recorded
/// in the header's FLEVEL field.
///
/// ```
/// use rust_deflate::{compress_zlib_with, decompress_zlib, CompressionLevel};
///
/// let data = b"hello hello hello hello";
/// let compressed = compress_zlib_with(data, CompressionLevel::BEST);
/// assert_eq!(decompress_zlib(&compressed).unwrap(), data);
/// ```
pub fn compress_zlib_with(data: &[u8], options: impl Into<CompressionOptions>) -> Vec<u8> {
    Compressor::new().compress_zlib_with(data, options)
}

/// A reusable DEFLATE compressor for compressing many inputs.
///
/// It keeps its match-finding tables (about 256 KB) and working buffers between
/// calls, so each input after the first skips allocating them. Inputs don't
/// affect each other: the same data always compresses to the same bytes.
///
/// ```
/// use rust_deflate::{CompressionLevel, Compressor, decompress};
///
/// let mut compressor = Compressor::new();
/// let mut out = Vec::new();
///
/// for data in [&b"first first first"[..], b"second second second"] {
///     out.clear();
///     let written = compressor.compress_into_with(data, &mut out, CompressionLevel::BEST);
///     assert_eq!(written, out.len());
///     assert_eq!(decompress(&out).unwrap(), data);
/// }
/// ```
pub struct Compressor {
    // Shared by the raw DEFLATE and zlib methods.
    deflater: Deflater,
}

impl Compressor {
    /// Creates a compressor. Its tables are allocated on first use.
    pub fn new() -> Self {
        Self {
            deflater: Deflater::new(),
        }
    }

    /// Compresses `data` into a raw DEFLATE stream, like [`compress`].
    pub fn compress(&mut self, data: &[u8]) -> Vec<u8> {
        self.compress_with(data, CompressionOptions::new())
    }

    /// Like [`Compressor::compress`], at the given [`CompressionLevel`] or with
    /// [`CompressionOptions`], like [`compress_with`].
    pub fn compress_with(&mut self, data: &[u8], options: impl Into<CompressionOptions>) -> Vec<u8> {
        let mut out = Vec::new();
        self.compress_into_with(data, &mut out, options);
        out
    }

    /// Compresses `data` into a raw DEFLATE stream and appends it to `out`,
    /// returning the number of bytes appended. What `out` already holds is left
    /// alone.
    ///
    /// Reusing `out` between calls (with [`Vec::clear`] in between) also saves
    /// allocating an output buffer each time.
    ///
    /// ```
    /// use rust_deflate::{Compressor, decompress};
    ///
    /// let mut compressor = Compressor::new();
    /// let mut out = b"header".to_vec();
    ///
    /// let written = compressor.compress_into(b"hello hello hello hello", &mut out);
    /// assert_eq!(&out[..6], b"header");
    /// assert_eq!(decompress(&out[6..]).unwrap(), b"hello hello hello hello");
    /// assert_eq!(written, out.len() - 6);
    /// ```
    pub fn compress_into(&mut self, data: &[u8], out: &mut Vec<u8>) -> usize {
        self.compress_into_with(data, out, CompressionOptions::new())
    }

    /// Like [`Compressor::compress_into`], at the given [`CompressionLevel`] or
    /// with [`CompressionOptions`].
    pub fn compress_into_with(
        &mut self,
        data: &[u8],
        out: &mut Vec<u8>,
        options: impl Into<CompressionOptions>,
    ) -> usize {
        let options = options.into();
        let start = out.len();
        let mut writer =
            BitWriter::appending_to(std::mem::take(out), deflater::output_bound(data.len()));
        self.deflater.deflate_into(data, options, &mut writer);
        *out = writer.finish();
        out.len() - start
    }

    /// Compresses `data` into a zlib stream, like [`compress_zlib`].
    pub fn compress_zlib(&mut self, data: &[u8]) -> Vec<u8> {
        self.compress_zlib_with(data, CompressionOptions::new())
    }

    /// Like [`Compressor::compress_zlib`], at the given [`CompressionLevel`] or
    /// with [`CompressionOptions`], like [`compress_zlib_with`].
    pub fn compress_zlib_with(
        &mut self,
        data: &[u8],
        options: impl Into<CompressionOptions>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        self.compress_zlib_into_with(data, &mut out, options);
        out
    }

    /// Compresses `data` into a zlib stream and appends it to `out`, returning the
    /// number of bytes appended. Works like [`Compressor::compress_into`]; the
    /// checksum covers only `data`.
    pub fn compress_zlib_into(&mut self, data: &[u8], out: &mut Vec<u8>) -> usize {
        self.compress_zlib_into_with(data, out, CompressionOptions::new())
    }

    /// Like [`Compressor::compress_zlib_into`], at the given [`CompressionLevel`]
    /// or with [`CompressionOptions`].
    pub fn compress_zlib_into_with(
        &mut self,
        data: &[u8],
        out: &mut Vec<u8>,
        options: impl Into<CompressionOptions>,
    ) -> usize {
        let options = options.into();
        let start = out.len();
        let mut writer =
            BitWriter::appending_to(std::mem::take(out), zlib::output_bound(data.len()));
        zlib::deflate_into(&mut self.deflater, data, options, &mut writer);
        *out = writer.finish();
        out.len() - start
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Compressor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compressor").finish_non_exhaustive()
    }
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
