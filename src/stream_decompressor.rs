use std::fmt;

use crate::Error;
use crate::stream::{Format, InflateStream};

/// A push-based streaming decompressor: feed compressed data in chunks as it
/// arrives, and get back whatever each chunk decompresses to.
///
/// Use it when the data doesn't come from a [`std::io::Read`] source (for those,
/// [`ZlibDecoder`] and [`DeflateDecoder`] are simpler): network callbacks, a
/// WebAssembly or JS wrapper, or any code that receives bytes piece by piece. Like
/// the `Read` decoders it works in constant memory, keeping only the last 32 KB of
/// output and the unread end of the input, so streams of any size work.
///
/// ```
/// use rust_deflate::StreamDecompressor;
///
/// // "hello hello hello hello", compressed by zlib.
/// let compressed = [
///     0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xB1,
/// ];
///
/// let mut stream = StreamDecompressor::zlib();
/// let mut output = Vec::new();
///
/// // Chunks of any size, as they arrive.
/// for chunk in compressed.chunks(5) {
///     stream.push(chunk, &mut output)?;
/// }
/// // No more input: flush the rest and verify the checksum.
/// stream.finish(&mut output)?;
///
/// assert_eq!(output, b"hello hello hello hello");
/// # Ok::<(), rust_deflate::Error>(())
/// ```
///
/// Each [`push`](Self::push) appends to `out` everything its input lets the decoder
/// produce, which can be up to about 1000 times the input size for highly compressed
/// data. Push smaller chunks to produce less at a time, or clear `out` between
/// pushes to keep memory flat.
///
/// For zlib, the checksum is only checked in [`finish`](Self::finish) (or the push
/// that completes the stream), so treat the output as untrusted until then.
///
/// [`ZlibDecoder`]: crate::ZlibDecoder
/// [`DeflateDecoder`]: crate::DeflateDecoder
pub struct StreamDecompressor {
    stream: InflateStream,
}

impl StreamDecompressor {
    /// A decompressor for a raw DEFLATE stream ([RFC 1951]).
    ///
    /// [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
    pub fn deflate() -> Self {
        Self {
            stream: InflateStream::new(Format::Raw),
        }
    }

    /// A decompressor for a zlib stream ([RFC 1950]), verifying its Adler-32 checksum.
    ///
    /// [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
    pub fn zlib() -> Self {
        Self {
            stream: InflateStream::new(Format::Zlib),
        }
    }

    /// Adds the next chunk of compressed data and appends everything it lets the
    /// decoder produce to `out`, returning the number of bytes appended. A chunk may
    /// be any size, including empty, and may end anywhere, even mid-symbol: whatever
    /// can't be decoded yet is kept for the next call.
    ///
    /// Once the stream is complete ([`is_done`](Self::is_done)), further input is
    /// ignored.
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] if the data is corrupt or not in the expected format.
    /// `out` is then truncated back to its length before the call, and every later
    /// call returns the same error until [`reset`](Self::reset).
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<usize, Error> {
        self.stream.push(input, out)
    }

    /// Declares the input complete and appends the rest of the output to `out`,
    /// returning the number of bytes appended. Call it once the last chunk has been
    /// pushed. Calling it again, or after the stream is done, appends nothing.
    ///
    /// # Errors
    ///
    /// [`Error::UnexpectedEndOfInput`] if the stream isn't complete, and for zlib
    /// [`Error::ChecksumMismatch`] if the data doesn't match its checksum, plus any
    /// error [`push`](Self::push) can return. `out` is then truncated back to its
    /// length before the call.
    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<usize, Error> {
        self.stream.finish(out)
    }

    /// Whether the whole stream has been decompressed (for zlib, including its
    /// checksum). Can become true during a [`push`](Self::push), before
    /// [`finish`](Self::finish).
    pub fn is_done(&self) -> bool {
        self.stream.is_done()
    }

    /// Starts over for a new stream of the same format, keeping all allocations. Also
    /// clears a previous error.
    pub fn reset(&mut self) {
        self.stream.reset();
    }
}

impl fmt::Debug for StreamDecompressor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamDecompressor")
            .field("done", &self.is_done())
            .finish_non_exhaustive()
    }
}
