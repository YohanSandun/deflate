use std::fmt;
use std::io::{self, Read};

use crate::stream::{Format, InflateStream};

/// Decompresses a raw DEFLATE stream from any [`Read`] source, in constant memory.
///
/// It reads compressed input from the wrapped reader as needed and keeps only a small
/// input buffer and the last 32 KB of output (under 400 KB in all), so streams of any
/// size, including multi-gigabyte files, can be decompressed.
///
/// ```
/// use std::io::Read;
/// use rust_deflate::DeflateDecoder;
///
/// // "hello hello hello hello", compressed by zlib as raw DEFLATE.
/// let compressed: &[u8] = &[0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
///
/// let mut text = String::new();
/// DeflateDecoder::new(compressed).read_to_string(&mut text)?;
/// assert_eq!(text, "hello hello hello hello");
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// # Errors
///
/// Reads fail with an [`io::Error`] wrapping a [`crate::Error`], with
/// [`io::ErrorKind::UnexpectedEof`] for truncated data and
/// [`io::ErrorKind::InvalidData`] for corrupt data; errors from the wrapped reader are
/// passed through. After a decoding error, every later read fails the same way.
///
/// The decoder may read past the end of the compressed stream from the wrapped
/// reader, so data following it there is not preserved.
pub struct DeflateDecoder<R> {
    reader: R,
    stream: InflateStream,
}

impl<R: Read> DeflateDecoder<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            stream: InflateStream::new(Format::Raw),
        }
    }
}

impl<R> DeflateDecoder<R> {
    pub fn get_ref(&self) -> &R {
        &self.reader
    }
    
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }
    
    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: Read> Read for DeflateDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read_from(&mut self.reader, buf)
    }
}

impl<R> fmt::Debug for DeflateDecoder<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeflateDecoder").finish_non_exhaustive()
    }
}

/// Decompresses a zlib stream from any [`Read`] source, in constant memory, verifying
/// its Adler-32 checksum at the end.
///
/// Like [`DeflateDecoder`], it keeps under 400 KB of buffers however large the stream
/// is.
///
/// ```no_run
/// use std::fs::File;
/// use rust_deflate::ZlibDecoder;
///
/// // Decompress a zlib file of any size to another file.
/// let mut decoder = ZlibDecoder::new(File::open("archive.zz")?);
/// std::io::copy(&mut decoder, &mut File::create("archive.bin")?)?;
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// # Errors
///
/// Same as [`DeflateDecoder`], plus header errors and
/// [`crate::Error::ChecksumMismatch`] (as [`io::ErrorKind::InvalidData`]). The
/// checksum is only known at the end, so data read before a mismatch is found has
/// already been returned: treat the output as untrusted until the last read returns 0.
pub struct ZlibDecoder<R> {
    reader: R,
    stream: InflateStream,
}

impl<R: Read> ZlibDecoder<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            stream: InflateStream::new(Format::Zlib),
        }
    }
}

impl<R> ZlibDecoder<R> {
    pub fn get_ref(&self) -> &R {
        &self.reader
    }
    
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }
    
    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: Read> Read for ZlibDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read_from(&mut self.reader, buf)
    }
}

impl<R> fmt::Debug for ZlibDecoder<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZlibDecoder").finish_non_exhaustive()
    }
}
