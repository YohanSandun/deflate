use std::fmt;

use crate::compress_stream::DeflateStream;
use crate::options::CompressionOptions;
use crate::stream::Format;

/// A push-based streaming compressor: feed data in chunks as it becomes
/// available, and get back compressed output as it's ready.
///
/// Use it when the data isn't written through [`std::io::Write`] (for that,
/// [`ZlibEncoder`] and [`DeflateEncoder`] are simpler): network callbacks, a
/// WebAssembly or JS wrapper, or any code that produces bytes piece by piece. It
/// works in constant memory, buffering at most about 300 KB of input (1.1 MB at
/// levels 8 and 9), so streams of any size work.
///
/// ```
/// use rust_deflate::{decompress_zlib, StreamCompressor};
///
/// let mut stream = StreamCompressor::zlib();
/// let mut compressed = Vec::new();
///
/// // Chunks of any size, as they come.
/// for chunk in [&b"hello "[..], b"hello ", b"hello ", b"hello"] {
///     stream.push(chunk, &mut compressed);
/// }
/// // No more input: compress the rest and write the checksum.
/// stream.finish(&mut compressed);
///
/// assert_eq!(decompress_zlib(&compressed)?, b"hello hello hello hello");
/// # Ok::<(), rust_deflate::Error>(())
/// ```
///
/// Input is compressed 256 KB at a time (1 MB at levels 8 and 9), so a
/// [`push`](Self::push) often appends nothing. To get output for everything pushed so far without ending the stream,
/// call [`flush`](Self::flush).
///
/// [`ZlibEncoder`]: crate::ZlibEncoder
/// [`DeflateEncoder`]: crate::DeflateEncoder
pub struct StreamCompressor {
    stream: DeflateStream,
}

impl StreamCompressor {
    /// A compressor producing a raw DEFLATE stream ([RFC 1951]) at
    /// [`CompressionLevel::MEDIUM`].
    ///
    /// [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
    /// [`CompressionLevel::MEDIUM`]: crate::CompressionLevel::MEDIUM
    pub fn deflate() -> Self {
        Self::deflate_with(CompressionOptions::new())
    }

    /// Like [`StreamCompressor::deflate`], at the given [`CompressionLevel`] or
    /// with [`CompressionOptions`].
    ///
    /// [`CompressionLevel`]: crate::CompressionLevel
    pub fn deflate_with(options: impl Into<CompressionOptions>) -> Self {
        Self {
            stream: DeflateStream::new(Format::Raw, options.into()),
        }
    }

    /// A compressor producing a zlib stream ([RFC 1950]), with its Adler-32
    /// checksum, at [`CompressionLevel::MEDIUM`].
    ///
    /// [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950
    /// [`CompressionLevel::MEDIUM`]: crate::CompressionLevel::MEDIUM
    pub fn zlib() -> Self {
        Self::zlib_with(CompressionOptions::new())
    }

    /// Like [`StreamCompressor::zlib`], at the given [`CompressionLevel`] or with
    /// [`CompressionOptions`].
    ///
    /// [`CompressionLevel`]: crate::CompressionLevel
    pub fn zlib_with(options: impl Into<CompressionOptions>) -> Self {
        Self {
            stream: DeflateStream::new(Format::Zlib, options.into()),
        }
    }

    /// Adds the next chunk of data and appends whatever compressed output is ready
    /// to `out`, returning the number of bytes appended. A chunk may be any size,
    /// including empty.
    ///
    /// # Panics
    ///
    /// If the stream is finished ([`is_done`](Self::is_done)); call
    /// [`reset`](Self::reset) to start a new one.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) -> usize {
        self.stream.push(input, out)
    }

    /// Compresses everything pushed so far and appends it to `out`, returning the
    /// number of bytes appended, without ending the stream. Afterwards the output
    /// so far decompresses to all the input so far, as with zlib's `Z_SYNC_FLUSH`:
    /// use it when the other side needs the data now, like a message over a
    /// connection.
    ///
    /// Each flush costs a few bytes and cuts off matches, so flushing often makes
    /// the output larger.
    ///
    /// ```
    /// use rust_deflate::{StreamCompressor, StreamDecompressor};
    ///
    /// let mut compressor = StreamCompressor::deflate();
    /// let mut decompressor = StreamDecompressor::deflate();
    /// let (mut compressed, mut received) = (Vec::new(), Vec::new());
    ///
    /// compressor.push(b"first message", &mut compressed);
    /// compressor.flush(&mut compressed);
    ///
    /// // The stream isn't finished, but everything pushed so far decodes.
    /// decompressor.push(&compressed, &mut received)?;
    /// assert_eq!(received, b"first message");
    /// # Ok::<(), rust_deflate::Error>(())
    /// ```
    ///
    /// # Panics
    ///
    /// If the stream is finished.
    pub fn flush(&mut self, out: &mut Vec<u8>) -> usize {
        self.stream.flush(out)
    }

    /// Declares the input complete and appends the rest of the stream to `out`
    /// (for zlib, including the checksum), returning the number of bytes appended.
    /// Calling it again appends nothing.
    pub fn finish(&mut self, out: &mut Vec<u8>) -> usize {
        self.stream.finish(out)
    }

    /// Whether the stream has been finished.
    pub fn is_done(&self) -> bool {
        self.stream.is_done()
    }

    /// Starts over for a new stream with the same format and options, keeping all
    /// allocations.
    pub fn reset(&mut self) {
        self.stream.reset();
    }
}

impl fmt::Debug for StreamCompressor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamCompressor")
            .field("done", &self.is_done())
            .finish_non_exhaustive()
    }
}
