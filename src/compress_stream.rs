use crate::checksum::adler32::Adler32;
use crate::compression::deflater::{ChunkEnd, Deflater};
use crate::compression::lz77::{MAX_DISTANCE, MAX_MATCH};
use crate::io::bit_writer::BitWriter;
use crate::options::CompressionOptions;
use crate::stream::Format;
use crate::zlib;

/// Input is compressed this many bytes at a time.
const CHUNK: usize = 256 * 1024;

/// Levels 8 and 9 take bigger chunks: their block split search costs about the
/// same for any chunk, so fewer chunks are faster.
const SEARCH_CHUNK: usize = 1024 * 1024;

/// The state of a compression stream: input waiting to be compressed, the window
/// before it that matches can reach back into, and the bits of a partial last byte.
///
/// Memory stays constant: at most the window, one chunk and a match's worth of
/// lookahead are buffered, however much input goes through.
pub(crate) struct DeflateStream {
    format: Format,
    options: CompressionOptions,
    chunk: usize,
    deflater: Deflater,
    writer: BitWriter,
    buffer: Vec<u8>,
    start: usize,
    checksum: Adler32,
    started: bool,
    done: bool,
}

impl DeflateStream {
    pub(crate) fn new(format: Format, options: CompressionOptions) -> Self {
        Self {
            format,
            options,
            chunk: if options.get_level() >= 8 {
                SEARCH_CHUNK
            } else {
                CHUNK
            },
            deflater: Deflater::new(),
            writer: BitWriter::appending_to(Vec::new(), 0),
            buffer: Vec::new(),
            start: 0,
            checksum: Adler32::new(),
            started: false,
            done: false,
        }
    }

    /// Adds `input` to the stream and appends whatever compressed output is ready
    /// to `out`, returning the number of bytes appended.
    pub(crate) fn push(&mut self, mut input: &[u8], out: &mut Vec<u8>) -> usize {
        assert!(!self.done, "input pushed after the stream was finished");
        let before = out.len();
        self.start_stream();
        if self.format == Format::Zlib {
            self.checksum.update(input);
        }

        // Taken a chunk at a time, so a huge push doesn't grow the buffer.
        let full = self.chunk + MAX_MATCH;
        while !input.is_empty() {
            let room = (self.start + full).saturating_sub(self.buffer.len());
            let (now, later) = input.split_at(room.min(input.len()));
            self.buffer.extend_from_slice(now);
            input = later;
            if self.buffer.len() - self.start >= full {
                self.compress(ChunkEnd::More);
            }
        }

        self.writer.drain_into(out);
        out.len() - before
    }

    /// Compresses all the input so far and byte-aligns the output, so everything
    /// pushed can be decoded from what's been returned.
    pub(crate) fn flush(&mut self, out: &mut Vec<u8>) -> usize {
        assert!(!self.done, "flush after the stream was finished");
        let before = out.len();
        self.start_stream();
        self.compress(ChunkEnd::Flush);
        Deflater::write_sync_flush(&mut self.writer);
        self.writer.drain_into(out);
        out.len() - before
    }

    /// Compresses the rest of the input and ends the stream. Appends nothing if the
    /// stream is already done.
    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) -> usize {
        if self.done {
            return 0;
        }
        let before = out.len();
        self.start_stream();
        self.compress(ChunkEnd::Last);

        self.writer.align_to_byte();
        if self.format == Format::Zlib {
            self.writer
                .write_bytes(&self.checksum.finish().to_be_bytes());
        }
        self.writer.drain_into(out);
        self.done = true;
        out.len() - before
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done
    }

    /// Starts a new stream with the same settings, keeping the allocations.
    pub(crate) fn reset(&mut self) {
        self.writer.clear();
        self.buffer.clear();
        self.start = 0;
        self.checksum = Adler32::new();
        self.started = false;
        self.done = false;
    }

    /// Writes the zlib header, the first time anything is written.
    fn start_stream(&mut self) {
        if !self.started {
            if self.format == Format::Zlib {
                zlib::write_header(&mut self.writer, self.options);
            }
            self.started = true;
        }
    }

    /// Compresses buffered input, then drops what the next chunk no longer needs:
    /// everything but the last `MAX_DISTANCE` bytes compressed.
    fn compress(&mut self, chunk_end: ChunkEnd) {
        let end = self.deflater.deflate_chunk(
            &self.buffer,
            self.start,
            chunk_end,
            self.options,
            &mut self.writer,
        );
        let keep_from = end.saturating_sub(MAX_DISTANCE);
        self.buffer.drain(..keep_from);
        self.start = end - keep_from;
    }
}

#[cfg(test)]
mod tests;
