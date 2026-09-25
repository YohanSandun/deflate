use std::fmt;

/// Why a DEFLATE stream couldn't be decompressed.
///
/// Every variant means the input is malformed (or not DEFLATE at all). New
/// variants may be added in minor releases, so match with a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The input ended before the final block did.
    UnexpectedEndOfInput,
    /// A block header uses the reserved block type 3.
    InvalidBlockType,
    /// A stored block's length doesn't match its one's-complement copy (LEN/NLEN).
    StoredLengthMismatch,
    /// A dynamic block header declares more than 286 literal/length codes or more
    /// than 30 distance codes.
    TooManyCodes,
    /// The code lengths in a dynamic block header describe more codes than can
    /// exist (an over-subscribed Huffman code).
    OverSubscribedCode,
    /// A "repeat previous code length" instruction comes before any code length.
    RepeatWithoutPreviousLength,
    /// A code-length repeat runs past the end of the code lengths.
    RepeatPastEnd,
    /// A dynamic block has no code for the end-of-block symbol.
    MissingEndOfBlockCode,
    /// The next bits don't match any code in the current Huffman table.
    InvalidCode,
    /// Literal/length symbol 286 or 287, which have codes but no meaning.
    InvalidLengthSymbol,
    /// Distance symbol 30 or 31, which have codes but no meaning.
    InvalidDistanceSymbol,
    /// A match refers back past the start of the output.
    DistanceTooFarBack,
    /// Invalid FCHECK in zlib header
    InvalidFcheck,
    /// Preset dictionary is set in zlib header; not supported
    PresetDictionary,
    /// Adler32 checksum mismatch
    ChecksumMismatch,
    /// The zlib header's compression method isn't DEFLATE (method 8).
    UnsupportedCompressionMethod,
    /// The zlib header declares a window larger than 32 KB (CINFO above 7).
    InvalidWindowSize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::UnexpectedEndOfInput => "unexpected end of input",
            Error::InvalidBlockType => "invalid flate block type",
            Error::StoredLengthMismatch => "corrupted stored block",
            Error::TooManyCodes => "too many length or distance symbols",
            Error::OverSubscribedCode => "over-subscribed Huffman code",
            Error::RepeatWithoutPreviousLength => "repeat with no previous code length",
            Error::RepeatPastEnd => "code length repeat past end",
            Error::MissingEndOfBlockCode => "missing end-of-block code",
            Error::InvalidCode => "invalid Huffman code",
            Error::InvalidLengthSymbol => "invalid length symbol",
            Error::InvalidDistanceSymbol => "invalid distance symbol",
            Error::DistanceTooFarBack => "invalid distance: too far back",
            Error::InvalidFcheck => "invalid fcheck",
            Error::PresetDictionary => "preset dictionary is not supported",
            Error::ChecksumMismatch => "checksum mismatch",
            Error::UnsupportedCompressionMethod => "unsupported compression method",
            Error::InvalidWindowSize => "invalid window size",
        })
    }
}

impl std::error::Error for Error {}
