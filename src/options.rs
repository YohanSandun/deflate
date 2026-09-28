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

/// Which kind of DEFLATE blocks the compressor writes (RFC 1951 section 3.2.3).
///
/// ```
/// use rust_deflate::{CompressionOptions, Strategy};
///
/// let options = CompressionOptions::new().strategy(Strategy::Fixed).level(9);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Strategy {
    /// Uncompressed blocks only.
    #[default]
    Stored,
    /// One block with the fixed Huffman codes.
    Fixed,
    /// Splits the input into blocks and writes each as whichever type is smallest,
    /// usually one with Huffman codes built for its data. Levels 8 and 9 search
    /// for the best split.
    Dynamic,
}

/// Settings for [`compress_with`] and [`compress_zlib_with`].
///
/// The default is [`Strategy::Stored`] at level 6.
///
/// [`compress_with`]: crate::compress_with
/// [`compress_zlib_with`]: crate::compress_zlib_with
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressionOptions {
    strategy: Strategy,
    level: u8,
}

impl CompressionOptions {
    /// The highest level: [`CompressionOptions::level`] clamps to it.
    pub const MAX_LEVEL: u8 = 9;

    /// [`Strategy::Stored`] at level 6.
    pub const fn new() -> Self {
        Self {
            strategy: Strategy::Stored,
            level: 6,
        }
    }

    /// Which kind of blocks to write.
    pub const fn strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// How hard to look for repeated data, from 0 to 9: higher levels search more
    /// and compress better, but take longer. 0 finds no matches at all, so only the
    /// Huffman coding shrinks the data. Values above 9 are treated as 9.
    pub const fn level(mut self, level: u8) -> Self {
        self.level = if level > Self::MAX_LEVEL {
            Self::MAX_LEVEL
        } else {
            level
        };
        self
    }

    pub(crate) const fn get_strategy(self) -> Strategy {
        self.strategy
    }

    pub(crate) const fn get_level(self) -> u8 {
        self.level
    }
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self::new()
    }
}
