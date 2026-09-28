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

/// How hard to compress, from 0 to 9, like zlib's levels: higher levels search
/// more for repeated data and compress better, but take longer.
///
/// ```
/// use rust_deflate::CompressionLevel;
///
/// assert_eq!(CompressionLevel::default(), CompressionLevel::MEDIUM);
/// assert_eq!(CompressionLevel::new(9), CompressionLevel::BEST);
/// assert_eq!(CompressionLevel::new(12), CompressionLevel::BEST);
/// assert_eq!(CompressionLevel::new(4).get(), 4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompressionLevel(u8);

impl CompressionLevel {
    /// 0: no compression. The data is copied into stored blocks as it is.
    pub const NONE: Self = Self(0);
    /// 1: the fastest level that compresses.
    pub const FAST: Self = Self(1);
    /// 6: a balance of speed and size, and the default.
    pub const MEDIUM: Self = Self(6);
    /// 9: the smallest output, and the slowest.
    pub const BEST: Self = Self(9);

    /// Level `level`. Values above 9 are treated as 9.
    pub const fn new(level: u8) -> Self {
        Self(if level > Self::BEST.0 {
            Self::BEST.0
        } else {
            level
        })
    }

    /// The level as a number, from 0 to 9.
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Default for CompressionLevel {
    fn default() -> Self {
        Self::MEDIUM
    }
}

impl From<u8> for CompressionLevel {
    fn from(level: u8) -> Self {
        Self::new(level)
    }
}

/// Which kind of DEFLATE blocks the compressor writes (RFC 1951 section 3.2.3).
/// At [`CompressionLevel::NONE`], every strategy writes stored blocks.
///
/// ```
/// use rust_deflate::{CompressionLevel, CompressionOptions, Strategy};
///
/// let options = CompressionOptions::new()
///     .strategy(Strategy::Fixed)
///     .level(CompressionLevel::BEST);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Strategy {
    /// Uncompressed blocks only.
    Stored,
    /// One block with the fixed Huffman codes.
    Fixed,
    /// Splits the input into blocks and writes each as whichever type is smallest,
    /// usually one with Huffman codes built for its data. Levels 8 and 9 search
    /// for the best split. The default.
    #[default]
    Dynamic,
}

/// Settings for [`compress_with`], [`compress_zlib_with`] and the [`Compressor`]
/// methods ending in `_with`. A [`CompressionLevel`] converts into options with
/// that level and the default strategy, so it can be passed on its own.
///
/// The default is [`Strategy::Dynamic`] at [`CompressionLevel::MEDIUM`], which
/// the functions and methods without `_with` use.
///
/// ```
/// use rust_deflate::{compress_with, CompressionLevel, CompressionOptions, Strategy};
///
/// let data = b"hello hello hello hello";
///
/// let smallest = compress_with(data, CompressionLevel::BEST);
/// let fixed = compress_with(data, CompressionOptions::new().strategy(Strategy::Fixed));
/// ```
///
/// [`compress_with`]: crate::compress_with
/// [`compress_zlib_with`]: crate::compress_zlib_with
/// [`Compressor`]: crate::Compressor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressionOptions {
    strategy: Strategy,
    level: CompressionLevel,
}

impl CompressionOptions {
    /// [`Strategy::Dynamic`] at [`CompressionLevel::MEDIUM`].
    pub const fn new() -> Self {
        Self {
            strategy: Strategy::Dynamic,
            level: CompressionLevel::MEDIUM,
        }
    }

    /// Which kind of blocks to write.
    pub const fn strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// How hard to compress.
    pub const fn level(mut self, level: CompressionLevel) -> Self {
        self.level = level;
        self
    }

    pub(crate) const fn get_strategy(self) -> Strategy {
        self.strategy
    }

    pub(crate) const fn get_level(self) -> u8 {
        self.level.get()
    }
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl From<CompressionLevel> for CompressionOptions {
    fn from(level: CompressionLevel) -> Self {
        Self::new().level(level)
    }
}
