//! Streaming decompression: a resumable decoder that keeps only a small input buffer
//! and the last 32 KB of output, so a stream of any size decompresses in constant
//! memory.

use std::io::{self, Read};

use crate::Error;
use crate::checksum::adler32::Adler32;
use crate::compression::inflater::{Inflater, Progress};
use crate::io::bit_reader::BitReader;
use crate::zlib;

// How far back DEFLATE matches can reach, so how much output history must be kept.
const WINDOW_SIZE: usize = 32 * 1024;

// A dynamic block header is at most about 570 bytes. A block header is parsed as soon
// as possible; if that fails with less than this much input buffered and more may
// come, it's retried later instead of reported, because the input may just have run
// out partway through it.
const HEADER_INPUT: usize = 1024;

// Output produced per step before handing it to the reader.
const STEP_OUTPUT: usize = 64 * 1024;

// Bytes requested from the underlying reader at a time.
const INPUT_CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Raw,
    Zlib,
}

#[derive(Clone, Copy, Debug)]
enum State {
    ZlibHeader,
    BlockHeader,
    Stored { remaining: usize },
    Huffman { fixed: bool },
    ZlibTrailer,
    Done,
    Failed(Error),
}

enum Step {
    Output,
    NeedInput,
    Done,
}

pub(crate) struct InflateStream {
    format: Format,
    inflater: Inflater,
    state: State,
    last_block: bool,

    input: Vec<u8>,
    bit_pos: usize,
    input_finished: bool,
    read_buffer: Vec<u8>,

    window: Vec<u8>,
    taken: usize,
    checksummed: usize,
    adler: Adler32,
}

impl InflateStream {
    pub(crate) fn new(format: Format) -> Self {
        Self {
            format,
            inflater: Inflater::new(),
            state: match format {
                Format::Raw => State::BlockHeader,
                Format::Zlib => State::ZlibHeader,
            },
            last_block: false,
            input: Vec::with_capacity(INPUT_CHUNK + HEADER_INPUT),
            bit_pos: 0,
            input_finished: false,
            read_buffer: Vec::new(),
            window: Vec::with_capacity(2 * WINDOW_SIZE + STEP_OUTPUT),
            taken: 0,
            checksummed: 0,
            adler: Adler32::new(),
        }
    }

    /// `Read::read` for a decoder: fills `buf` with decompressed data, pulling input
    /// from `reader` as needed. Returns 0 at the end of the stream.
    pub(crate) fn read_from<R: Read>(
        &mut self,
        reader: &mut R,
        buf: &mut [u8],
    ) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            let pending = &self.window[self.taken..];
            if !pending.is_empty() {
                let n = pending.len().min(buf.len());
                buf[..n].copy_from_slice(&pending[..n]);
                self.taken += n;
                return Ok(n);
            }

            match self.step()? {
                Step::Output => {}
                Step::NeedInput => self.fill_from(reader)?,
                Step::Done if self.taken == self.window.len() => return Ok(0),
                Step::Done => {}
            }
        }
    }

    fn fill_from<R: Read>(&mut self, reader: &mut R) -> io::Result<()> {
        if self.read_buffer.is_empty() {
            self.read_buffer = vec![0; INPUT_CHUNK];
        }

        let read = loop {
            match reader.read(&mut self.read_buffer) {
                Ok(n) => break n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        };

        if read == 0 {
            self.input_finished = true;
        } else {
            let (buffer, input) = (&self.read_buffer, &mut self.input);
            Self::append(input, &mut self.bit_pos, &buffer[..read]);
        }

        Ok(())
    }

    fn append(input: &mut Vec<u8>, bit_pos: &mut usize, data: &[u8]) {
        let consumed = *bit_pos / 8;
        if consumed > 0 {
            input.drain(..consumed);
            *bit_pos -= consumed * 8;
        }
        input.extend_from_slice(data);
    }

    /// Push API: adds `data` to the input and appends everything it lets the decoder
    /// produce to `out`. Returns the number of bytes appended. Input after the end of
    /// the stream is ignored. On error, `out` is truncated back to its original length.
    pub(crate) fn push(&mut self, data: &[u8], out: &mut Vec<u8>) -> Result<usize, Error> {
        if matches!(self.state, State::Done) {
            return Ok(0);
        }

        if !data.is_empty() && !self.input_finished {
            Self::append(&mut self.input, &mut self.bit_pos, data);
        }

        self.drain_into(out)
    }

    /// Push API: marks the input as complete and appends the rest of the output to
    /// `out`. Fails if the stream is incomplete (or corrupt, or its checksum is wrong).
    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) -> Result<usize, Error> {
        self.input_finished = true;
        self.drain_into(out)
    }

    pub(crate) fn is_done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// Starts over for a new stream, keeping every allocation.
    pub(crate) fn reset(&mut self) {
        self.state = match self.format {
            Format::Raw => State::BlockHeader,
            Format::Zlib => State::ZlibHeader,
        };
        self.last_block = false;
        self.input.clear();
        self.bit_pos = 0;
        self.input_finished = false;
        self.window.clear();
        self.taken = 0;
        self.checksummed = 0;
        self.adler = Adler32::new();
    }

    fn drain_into(&mut self, out: &mut Vec<u8>) -> Result<usize, Error> {
        let start = out.len();

        let result = loop {
            let step = self.step();

            out.extend_from_slice(&self.window[self.taken..]);
            self.taken = self.window.len();

            match step {
                Ok(Step::Output) => {}
                Ok(Step::NeedInput | Step::Done) => break Ok(()),
                Err(error) => break Err(error),
            }
        };

        match result {
            Ok(()) => Ok(out.len() - start),
            Err(error) => {
                out.truncate(start);
                Err(error)
            }
        }
    }

    fn step(&mut self) -> Result<Step, Error> {
        if let State::Failed(error) = self.state {
            return Err(error);
        }

        self.compact_window();

        let limit = self.window.len() + STEP_OUTPUT;
        let result = self.produce(limit);
        self.update_checksum();

        result.inspect_err(|&error| self.state = State::Failed(error))
    }

    fn compact_window(&mut self) {
        let droppable = self
            .taken
            .min(self.window.len().saturating_sub(WINDOW_SIZE));

        if droppable >= STEP_OUTPUT {
            self.window.drain(..droppable);
            self.taken -= droppable;
            self.checksummed -= droppable;
        }
    }

    fn update_checksum(&mut self) {
        if self.format == Format::Zlib {
            self.adler.update(&self.window[self.checksummed..]);
        }
        self.checksummed = self.window.len();
    }

    fn remaining_input_bytes(&self) -> usize {
        (self.input.len() * 8 - self.bit_pos) / 8
    }

    fn after_block(&self) -> State {
        match (self.last_block, self.format) {
            (false, _) => State::BlockHeader,
            (true, Format::Raw) => State::Done,
            (true, Format::Zlib) => State::ZlibTrailer,
        }
    }

    fn produce(&mut self, limit: usize) -> Result<Step, Error> {
        loop {
            match self.state {
                State::ZlibHeader => {
                    let start = self.bit_pos / 8;
                    let Some(header) = self.input[start..].first_chunk::<2>() else {
                        return self.need_input();
                    };

                    zlib::check_header(header)?;
                    self.bit_pos += 16;
                    self.state = State::BlockHeader;
                }

                State::BlockHeader => {
                    let mut reader = BitReader::at_bit(&self.input, self.bit_pos);

                    match Self::read_block_header(&mut self.inflater, &mut reader) {
                        Ok((last_block, state)) => {
                            self.last_block = last_block;
                            self.state = state;
                            self.bit_pos = reader.bit_position();
                        }
                        Err(_)
                            if !self.input_finished
                                && self.remaining_input_bytes() < HEADER_INPUT =>
                        {
                            return Ok(Step::NeedInput);
                        }
                        Err(error) => return Err(error),
                    }
                }

                State::Stored { remaining } => {
                    if remaining == 0 {
                        self.state = self.after_block();
                        continue;
                    }

                    if self.window.len() >= limit {
                        return Ok(Step::Output);
                    }

                    let start = self.bit_pos / 8;
                    let available = self.input.len() - start;
                    if available == 0 {
                        return self.need_input();
                    }

                    let n = remaining.min(available).min(limit - self.window.len());
                    self.window.extend_from_slice(&self.input[start..start + n]);
                    self.bit_pos += n * 8;
                    self.state = State::Stored {
                        remaining: remaining - n,
                    };
                }

                State::Huffman { fixed } => {
                    let mut reader = BitReader::at_bit(&self.input, self.bit_pos);
                    let progress = self.inflater.decode_block_streaming(
                        &mut reader,
                        fixed,
                        &mut self.window,
                        limit,
                        self.input_finished,
                    )?;
                    self.bit_pos = reader.bit_position();

                    match progress {
                        Progress::EndOfBlock => self.state = self.after_block(),
                        Progress::NeedInput => return Ok(Step::NeedInput),
                        Progress::OutputFull => return Ok(Step::Output),
                    }
                }

                State::ZlibTrailer => {
                    let start = self.bit_pos.div_ceil(8);
                    let Some(trailer) = self.input[start..].first_chunk::<4>() else {
                        return self.need_input();
                    };
                    let expected = u32::from_be_bytes(*trailer);

                    self.update_checksum();
                    if expected != self.adler.finish() {
                        return Err(Error::ChecksumMismatch);
                    }

                    self.bit_pos = (start + 4) * 8;
                    self.state = State::Done;
                }

                State::Done => return Ok(Step::Done),

                State::Failed(error) => return Err(error),
            }
        }
    }

    fn read_block_header(
        inflater: &mut Inflater,
        reader: &mut BitReader,
    ) -> Result<(bool, State), Error> {
        let last_block = reader.read_next_bit()? == 1;

        let state = match reader.read_next_bits(2)? {
            0 => State::Stored {
                remaining: Inflater::read_stored_length(reader)?,
            },
            1 => State::Huffman { fixed: true },
            2 => {
                inflater.read_dynamic_header(reader)?;
                State::Huffman { fixed: false }
            }
            _ => return Err(Error::InvalidBlockType),
        };

        Ok((last_block, state))
    }

    fn need_input(&self) -> Result<Step, Error> {
        if self.input_finished {
            Err(Error::UnexpectedEndOfInput)
        } else {
            Ok(Step::NeedInput)
        }
    }
}

#[cfg(test)]
mod tests;
