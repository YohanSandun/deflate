use crate::compression::inflater::OutputSize;

/// Output settings for the `_with` functions and methods, such as
/// [`decompress_with`], [`Decompressor::decompress_into_with`] and
/// [`Decompressor::decompress_zlib_into_with`].
///
/// The default is no limit and no size hint, which is what the versions without
/// `_with` use.
///
/// ```
/// use rust_deflate::OutputOptions;
///
/// // Untrusted input: fail instead of producing more than 64 MB.
/// let limited = OutputOptions::new().max_output(64 << 20);
///
/// // The exact size is known up front, e.g. from a PNG's IHDR chunk: allocate it
/// // once, and treat anything longer as corrupt.
/// let exact = OutputOptions::exact(1920 * 1080 * 4 + 1080);
/// ```
///
/// [`decompress_with`]: crate::decompress_with
/// [`Decompressor::decompress_into_with`]: crate::Decompressor::decompress_into_with
/// [`Decompressor::decompress_zlib_into_with`]: crate::Decompressor::decompress_zlib_into_with
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputOptions {
    max_output: Option<usize>,
    size_hint: Option<usize>,
}

impl OutputOptions {
    /// No limit and no size hint.
    pub const fn new() -> Self {
        Self {
            max_output: None,
            size_hint: None,
        }
    }

    /// Both a limit and a size hint of `bytes`: for when the exact decompressed size
    /// is known, like a PNG image's size from its header.
    pub const fn exact(bytes: usize) -> Self {
        Self::new().max_output(bytes).size_hint(bytes)
    }

    /// Fails with [`Error::OutputLimitExceeded`] as soon as a stream decompresses to
    /// more than `bytes`, counting only what this call appends. The output buffer is
    /// never grown much past the limit, so a small malicious input can't make it
    /// allocate far more memory than you allowed.
    ///
    /// [`Error::OutputLimitExceeded`]: crate::Error::OutputLimitExceeded
    pub const fn max_output(mut self, bytes: usize) -> Self {
        self.max_output = Some(bytes);
        self
    }

    /// Allocates room for `bytes` of output up front, instead of guessing from the
    /// input size. With the right size, the output buffer is allocated once and never
    /// grows. A wrong hint is only a performance issue: decompression still works,
    /// growing the buffer if it's too small.
    ///
    /// The memory is allocated before any data is decoded, so don't pass a size taken
    /// straight from untrusted input without capping it (the hint is also capped by
    /// [`OutputOptions::max_output`]).
    pub const fn size_hint(mut self, bytes: usize) -> Self {
        self.size_hint = Some(bytes);
        self
    }

    pub(crate) const fn to_output_size(self) -> OutputSize {
        OutputSize {
            max_output: self.max_output,
            size_hint: self.size_hint,
        }
    }
}
