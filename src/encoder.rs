use std::fmt;
use std::io::{self, Write};

use crate::compress_stream::DeflateStream;
use crate::options::CompressionOptions;
use crate::stream::Format;

/// Compresses everything written to it into a raw DEFLATE stream, which it writes
/// to the wrapped [`Write`], in constant memory.
///
/// Call [`finish`](Self::finish) when done: it writes the end of the stream and
/// returns the writer. Dropping the encoder finishes too, but ignores any error.
///
/// ```
/// use std::io::Write;
/// use rust_deflate::{decompress, DeflateEncoder};
///
/// let mut encoder = DeflateEncoder::new(Vec::new());
/// encoder.write_all(b"hello hello hello hello")?;
/// let compressed = encoder.finish()?;
///
/// assert_eq!(decompress(&compressed).unwrap(), b"hello hello hello hello");
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// [`flush`](Write::flush) writes out everything written so far, so it can be
/// decompressed without waiting for the end of the stream (like zlib's
/// `Z_SYNC_FLUSH`), then flushes the wrapped writer.
///
/// # Errors
///
/// Only errors from the wrapped writer, passed through. After one, the stream is
/// incomplete and should be discarded.
pub struct DeflateEncoder<W: Write> {
    inner: Encoder<W>,
}

impl<W: Write> DeflateEncoder<W> {
    /// An encoder writing to `writer` at [`CompressionLevel::MEDIUM`].
    ///
    /// [`CompressionLevel::MEDIUM`]: crate::CompressionLevel::MEDIUM
    pub fn new(writer: W) -> Self {
        Self::with_options(writer, CompressionOptions::new())
    }

    /// An encoder writing to `writer` at the given [`CompressionLevel`] or with
    /// [`CompressionOptions`].
    ///
    /// [`CompressionLevel`]: crate::CompressionLevel
    pub fn with_options(writer: W, options: impl Into<CompressionOptions>) -> Self {
        Self {
            inner: Encoder::new(writer, Format::Raw, options.into()),
        }
    }

    pub fn get_ref(&self) -> &W {
        self.inner.writer()
    }

    /// The wrapped writer. Writing to it directly corrupts the stream.
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.writer_mut()
    }

    /// Writes the rest of the stream, then returns the wrapped writer.
    ///
    /// # Errors
    ///
    /// Errors from the wrapped writer.
    pub fn finish(self) -> io::Result<W> {
        self.inner.finish()
    }
}

impl<W: Write> Write for DeflateEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write> fmt::Debug for DeflateEncoder<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeflateEncoder").finish_non_exhaustive()
    }
}

/// Compresses everything written to it into a zlib stream, with its Adler-32
/// checksum, which it writes to the wrapped [`Write`], in constant memory.
///
/// It works like [`DeflateEncoder`]: call [`finish`](Self::finish) when done.
///
/// ```no_run
/// use std::fs::File;
/// use rust_deflate::{CompressionLevel, ZlibEncoder};
///
/// // Compress a file of any size to another file.
/// let mut encoder = ZlibEncoder::with_options(File::create("archive.zz")?, CompressionLevel::BEST);
/// std::io::copy(&mut File::open("archive.bin")?, &mut encoder)?;
/// encoder.finish()?;
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// # Errors
///
/// Same as [`DeflateEncoder`].
pub struct ZlibEncoder<W: Write> {
    inner: Encoder<W>,
}

impl<W: Write> ZlibEncoder<W> {
    /// An encoder writing to `writer` at [`CompressionLevel::MEDIUM`].
    ///
    /// [`CompressionLevel::MEDIUM`]: crate::CompressionLevel::MEDIUM
    pub fn new(writer: W) -> Self {
        Self::with_options(writer, CompressionOptions::new())
    }

    /// An encoder writing to `writer` at the given [`CompressionLevel`] or with
    /// [`CompressionOptions`].
    ///
    /// [`CompressionLevel`]: crate::CompressionLevel
    pub fn with_options(writer: W, options: impl Into<CompressionOptions>) -> Self {
        Self {
            inner: Encoder::new(writer, Format::Zlib, options.into()),
        }
    }

    pub fn get_ref(&self) -> &W {
        self.inner.writer()
    }

    /// The wrapped writer. Writing to it directly corrupts the stream.
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.writer_mut()
    }

    /// Writes the rest of the stream and the checksum, then returns the wrapped
    /// writer.
    ///
    /// # Errors
    ///
    /// Errors from the wrapped writer.
    pub fn finish(self) -> io::Result<W> {
        self.inner.finish()
    }
}

impl<W: Write> Write for ZlibEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write> fmt::Debug for ZlibEncoder<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZlibEncoder").finish_non_exhaustive()
    }
}

/// What both encoders share. The writer is only taken out by `finish`, so it's
/// always there while the encoder is in use.
struct Encoder<W: Write> {
    writer: Option<W>,
    stream: DeflateStream,
    out: Vec<u8>,
}

impl<W: Write> Encoder<W> {
    fn new(writer: W, format: Format, options: CompressionOptions) -> Self {
        Self {
            writer: Some(writer),
            stream: DeflateStream::new(format, options),
            out: Vec::new(),
        }
    }

    fn writer(&self) -> &W {
        self.writer.as_ref().unwrap()
    }

    fn writer_mut(&mut self) -> &mut W {
        self.writer.as_mut().unwrap()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.push(buf, &mut self.out);
        self.write_out()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush(&mut self.out);
        self.write_out()?;
        self.writer_mut().flush()
    }

    fn finish(mut self) -> io::Result<W> {
        self.try_finish()?;
        Ok(self.writer.take().unwrap())
    }

    fn try_finish(&mut self) -> io::Result<()> {
        self.stream.finish(&mut self.out);
        self.write_out()?;
        self.writer_mut().flush()
    }

    fn write_out(&mut self) -> io::Result<()> {
        let result = self.writer.as_mut().unwrap().write_all(&self.out);
        self.out.clear();
        result
    }
}

impl<W: Write> Drop for Encoder<W> {
    fn drop(&mut self) {
        if self.writer.is_some() && !self.stream.is_done() {
            let _ = self.try_finish();
        }
    }
}
