//! A DEFLATE ([RFC 1951]) decompressor written from scratch in Rust, with no
//! dependencies and no `unsafe` code.
//!
//! The input is a raw DEFLATE stream: no zlib (RFC 1950) or gzip (RFC 1952)
//! wrapper. Stored, fixed Huffman and dynamic Huffman blocks are supported.
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
//! [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
#![forbid(unsafe_code)]

mod compression;
mod error;
mod io;

pub use error::Error;

use compression::inflater::Inflater;

/// Decompresses a raw DEFLATE stream.
///
/// Decoding stops at the end of the final block; any bytes after it are ignored.
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
    Inflater::new(data).inflate()
}

// Runs the README's code examples as doctests so they can't go stale.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
