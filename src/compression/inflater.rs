use std::sync::LazyLock;

use crate::Error;
use crate::compression::huffman_decoder::{Alphabet, Entry, HuffmanDecoder};
use crate::io::bit_reader::BitReader;

// Built once per process and shared by every fixed block.
static FIXED_DECODERS: LazyLock<(HuffmanDecoder, HuffmanDecoder)> = LazyLock::new(|| {
    let mut lengths = [8u8; 288];
    let distance_length = [5u8; 32];

    for i in 144..256 {
        lengths[i] = 9;
    }

    for i in 256..280 {
        lengths[i] = 7;
    }

    (
        HuffmanDecoder::new_for(&lengths, Alphabet::LiteralLength)
            .unwrap_or_else(|_| unreachable!()),
        HuffmanDecoder::new_for(&distance_length, Alphabet::Distance)
            .unwrap_or_else(|_| unreachable!()),
    )
});

const MAX_INITIAL_CAPACITY: usize = 64 << 20;

const OUTPUT_SLACK: usize = 258 + 8;

const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

const MAX_LITERAL_LENGTH_CODES: usize = 286;
const MAX_DISTANCE_CODES: usize = 30;

const MAX_SYMBOL_BITS: u32 = 48;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Progress {
    EndOfBlock,
    /// Streaming only: fewer than `MAX_SYMBOL_BITS` of input are left.
    NeedInput,
    /// Streaming only: the output passed the limit for this call.
    OutputFull,
}

pub(crate) struct Inflated {
    pub(crate) input_used: usize,
    pub(crate) output_added: usize,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct OutputSize {
    pub(crate) max_output: Option<usize>,
    pub(crate) size_hint: Option<usize>,
}

#[derive(Clone, Copy)]
struct Bounds {
    stream_start: usize,
    limit: usize,
}

pub(crate) struct Inflater {
    code_length_decoder: HuffmanDecoder,
    literal_length_decoder: HuffmanDecoder,
    distance_decoder: HuffmanDecoder,
}

impl Inflater {
    pub(crate) fn new() -> Self {
        Self {
            code_length_decoder: HuffmanDecoder::empty(),
            literal_length_decoder: HuffmanDecoder::empty(),
            distance_decoder: HuffmanDecoder::empty(),
        }
    }

    pub(crate) fn inflate_into(
        &mut self,
        data: &[u8],
        out: &mut Vec<u8>,
        size: OutputSize,
    ) -> Result<Inflated, Error> {
        let stream_start = out.len();
        let bounds = Bounds {
            stream_start,
            limit: size
                .max_output
                .map_or(usize::MAX, |max| stream_start.saturating_add(max)),
        };

        Self::reserve_output(data, out, size);

        match self.inflate_blocks(data, out, bounds) {
            Ok(input_used) => Ok(Inflated {
                input_used,
                output_added: out.len() - stream_start,
            }),
            Err(error) => {
                out.truncate(stream_start);
                Err(error)
            }
        }
    }

    // With a size hint, reserves exactly that plus the loop's working room, so the
    // buffer never has to grow. Otherwise guesses 4x the input, capped by the limit.
    // Either way it's a no-op when `out` already has the room.
    fn reserve_output(data: &[u8], out: &mut Vec<u8>, size: OutputSize) {
        let max = size.max_output.unwrap_or(usize::MAX);

        match size.size_hint {
            Some(hint) => out.reserve_exact(hint.min(max).saturating_add(OUTPUT_SLACK)),
            None => out.reserve(
                data.len()
                    .saturating_mul(4)
                    .min(MAX_INITIAL_CAPACITY)
                    .min(max.saturating_add(OUTPUT_SLACK)),
            ),
        }
    }

    fn inflate_blocks(
        &mut self,
        data: &[u8],
        out: &mut Vec<u8>,
        bounds: Bounds,
    ) -> Result<usize, Error> {
        let mut reader = BitReader::new(data);

        let mut b_final = 0u8;
        while b_final == 0 {
            b_final = reader.read_next_bit()?;
            let b_type = reader.read_next_bits(2)?;

            match b_type {
                0 => Self::inflate_stored_block(&mut reader, out, bounds)?,
                1 => Self::inflate_fixed_huffman_block(&mut reader, out, bounds)?,
                2 => self.inflate_dynamic_huffman_block(&mut reader, out, bounds)?,
                _ => return Err(Error::InvalidBlockType),
            }
        }

        reader.align_to_byte();
        Ok(reader.position())
    }

    fn inflate_stored_block(
        reader: &mut BitReader,
        inflated_data: &mut Vec<u8>,
        bounds: Bounds,
    ) -> Result<(), Error> {
        let len = Self::read_stored_length(reader)?;
        let bytes = reader.read_bytes(len)?;

        if inflated_data.len() + bytes.len() > bounds.limit {
            return Err(Error::OutputLimitExceeded);
        }

        inflated_data.extend_from_slice(bytes);

        Ok(())
    }

    pub(crate) fn read_stored_length(reader: &mut BitReader) -> Result<usize, Error> {
        reader.align_to_byte();

        let len = reader.read_next_bits(16)?;
        let n_len = reader.read_next_bits(16)?;

        if len ^ n_len != 0xFFFF {
            return Err(Error::StoredLengthMismatch);
        }

        Ok(len as usize)
    }

    fn inflate_fixed_huffman_block(
        reader: &mut BitReader,
        inflated_data: &mut Vec<u8>,
        bounds: Bounds,
    ) -> Result<(), Error> {
        let (literal_length_decoder, distance_decoder) = &*FIXED_DECODERS;

        Self::inflate_block::<false>(
            reader,
            literal_length_decoder,
            distance_decoder,
            inflated_data,
            bounds,
            true,
        )?;

        Ok(())
    }

    fn inflate_dynamic_huffman_block(
        &mut self,
        reader: &mut BitReader,
        inflated_data: &mut Vec<u8>,
        bounds: Bounds,
    ) -> Result<(), Error> {
        self.read_dynamic_header(reader)?;

        Self::inflate_block::<false>(
            reader,
            &self.literal_length_decoder,
            &self.distance_decoder,
            inflated_data,
            bounds,
            true,
        )?;

        Ok(())
    }

    pub(crate) fn read_dynamic_header(&mut self, reader: &mut BitReader) -> Result<(), Error> {
        let hlit = reader.read_next_bits(5)? as usize + 257;
        let hdist = reader.read_next_bits(5)? as usize + 1;
        let hclen = reader.read_next_bits(4)? as usize + 4;

        if hlit > MAX_LITERAL_LENGTH_CODES || hdist > MAX_DISTANCE_CODES {
            return Err(Error::TooManyCodes);
        }

        let mut code_length_code_lengths = [0u8; 19];

        for &symbol in &CODE_LENGTH_ORDER[..hclen] {
            code_length_code_lengths[symbol] = reader.read_next_bits(3)? as u8;
        }

        self.code_length_decoder
            .rebuild(&code_length_code_lengths)?;

        let mut code_lengths = [0u8; MAX_LITERAL_LENGTH_CODES + MAX_DISTANCE_CODES];
        let total = hlit + hdist;

        let mut index = 0;
        while index < total {
            index = Self::decode_code_length(
                &self.code_length_decoder,
                reader,
                &mut code_lengths[..total],
                index,
            )?;
        }

        let (literal_length_code_lengths, distance_code_lengths) =
            code_lengths[..total].split_at(hlit);

        if literal_length_code_lengths[256] == 0 {
            return Err(Error::MissingEndOfBlockCode);
        }

        self.literal_length_decoder
            .rebuild_for(literal_length_code_lengths, Alphabet::LiteralLength)?;
        self.distance_decoder
            .rebuild_for(distance_code_lengths, Alphabet::Distance)?;

        Ok(())
    }
    
    pub(crate) fn decode_block_streaming(
        &self,
        reader: &mut BitReader,
        fixed: bool,
        out: &mut Vec<u8>,
        limit: usize,
        input_finished: bool,
    ) -> Result<Progress, Error> {
        let (literal_length_decoder, distance_decoder) = if fixed {
            let (literal_length, distance) = &*FIXED_DECODERS;
            (literal_length, distance)
        } else {
            (&self.literal_length_decoder, &self.distance_decoder)
        };

        let bounds = Bounds {
            stream_start: 0,
            limit,
        };

        Self::inflate_block::<true>(
            reader,
            literal_length_decoder,
            distance_decoder,
            out,
            bounds,
            input_finished,
        )
    }

    fn decode_code_length(
        code_length_decoder: &HuffmanDecoder,
        reader: &mut BitReader,
        code_lengths: &mut [u8],
        index: usize,
    ) -> Result<usize, Error> {
        let symbol = code_length_decoder.decode(reader)?;

        let (code_length, repeat) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => {
                if index == 0 {
                    return Err(Error::RepeatWithoutPreviousLength);
                }
                (
                    code_lengths[index - 1],
                    reader.read_next_bits(2)? as usize + 3,
                )
            }
            17 => (0, reader.read_next_bits(3)? as usize + 3),
            18 => (0, reader.read_next_bits(7)? as usize + 11),
            _ => unreachable!("code-length alphabet has 19 symbols"),
        };

        if repeat > code_lengths.len() - index {
            return Err(Error::RepeatPastEnd);
        }

        code_lengths[index..index + repeat].fill(code_length);

        Ok(index + repeat)
    }
    
    fn inflate_block<const STREAMING: bool>(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        inflated_data: &mut Vec<u8>,
        bounds: Bounds,
        input_finished: bool,
    ) -> Result<Progress, Error> {
        let result = Self::decode_symbols::<STREAMING>(
            reader,
            literal_length_decoder,
            distance_decoder,
            inflated_data,
            bounds,
            input_finished,
        );
        reader.finish_buffered();
        result
    }

    fn decode_symbols<const STREAMING: bool>(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        inflated_data: &mut Vec<u8>,
        bounds: Bounds,
        input_finished: bool,
    ) -> Result<Progress, Error> {
        let mut pos = inflated_data.len();
        let result = Self::decode_symbols_into::<STREAMING>(
            reader,
            literal_length_decoder,
            distance_decoder,
            inflated_data,
            &mut pos,
            bounds,
            input_finished,
        );
        inflated_data.truncate(pos);
        result
    }
    
    #[inline(always)]
    fn decode_symbols_into<const STREAMING: bool>(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        out: &mut Vec<u8>,
        pos: &mut usize,
        bounds: Bounds,
        input_finished: bool,
    ) -> Result<Progress, Error> {
        reader.refill_full();
        let mut entry = literal_length_decoder.lookup(reader.peek_buffer());

        loop {
            if out.len() - *pos < OUTPUT_SLACK {
                if *pos > bounds.limit {
                    return if STREAMING {
                        Ok(Progress::OutputFull)
                    } else {
                        Err(Error::OutputLimitExceeded)
                    };
                }
                Self::grow_output(out, *pos, bounds.limit);
            }
            
            if STREAMING && !input_finished && reader.buffered_bits() < MAX_SYMBOL_BITS {
                let checkpoint = (reader.clone(), *pos);

                match Self::decode_one_symbol(
                    reader,
                    literal_length_decoder,
                    distance_decoder,
                    out,
                    pos,
                    bounds,
                ) {
                    Ok(true) => return Ok(Progress::EndOfBlock),
                    Ok(false) => {
                        reader.refill_full();
                        entry = literal_length_decoder.lookup(reader.peek_buffer());
                        continue;
                    }
                    Err(_) => {
                        (*reader, *pos) = checkpoint;
                        return Ok(Progress::NeedInput);
                    }
                }
            }

            if entry.kind() == Entry::LITERAL {
                reader.consume_buffered(entry.code_length())?;
                let next = literal_length_decoder.lookup(reader.peek_buffer());
                Self::write_literals(out, pos, entry);
                entry = next;

                if entry.kind() == Entry::LITERAL {
                    reader.consume_buffered(entry.code_length())?;
                    let next = literal_length_decoder.lookup(reader.peek_buffer());
                    Self::write_literals(out, pos, entry);
                    entry = next;

                    if entry.kind() == Entry::LITERAL {
                        reader.consume_buffered(entry.code_length())?;
                        Self::write_literals(out, pos, entry);

                        reader.refill_full();
                        entry = literal_length_decoder.lookup(reader.peek_buffer());
                        continue;
                    }
                }

                reader.refill_full();
                continue;
            }

            reader.consume_buffered(entry.code_length())?;

            match entry.kind() {
                Entry::BASE => {
                    let length = entry.value() + reader.take_buffered(entry.extra_bits())?;

                    let distance_entry = distance_decoder.lookup(reader.peek_buffer());
                    reader.consume_buffered(distance_entry.code_length())?;

                    if distance_entry.kind() != Entry::BASE {
                        return Err(distance_entry.error());
                    }

                    let distance = distance_entry.value()
                        + reader.take_buffered(distance_entry.extra_bits())?;

                    reader.refill_full();
                    entry = literal_length_decoder.lookup(reader.peek_buffer());

                    Self::copy_match(out, *pos, length as usize, distance as usize, bounds)?;
                    *pos += length as usize;
                }
                Entry::END_OF_BLOCK => {
                    if !STREAMING && *pos > bounds.limit {
                        return Err(Error::OutputLimitExceeded);
                    }
                    return Ok(Progress::EndOfBlock);
                }
                _ => return Err(entry.error()),
            }
        }
    }
    
    fn decode_one_symbol(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        out: &mut [u8],
        pos: &mut usize,
        bounds: Bounds,
    ) -> Result<bool, Error> {
        let entry = literal_length_decoder.lookup(reader.peek_buffer());
        reader.consume_buffered(entry.code_length())?;

        match entry.kind() {
            Entry::LITERAL => {
                Self::write_literals(out, pos, entry);
                Ok(false)
            }
            Entry::BASE => {
                let length = entry.value() + reader.take_buffered(entry.extra_bits())?;

                let distance_entry = distance_decoder.lookup(reader.peek_buffer());
                reader.consume_buffered(distance_entry.code_length())?;
                if distance_entry.kind() != Entry::BASE {
                    return Err(distance_entry.error());
                }
                let distance =
                    distance_entry.value() + reader.take_buffered(distance_entry.extra_bits())?;

                Self::copy_match(out, *pos, length as usize, distance as usize, bounds)?;
                *pos += length as usize;
                Ok(false)
            }
            Entry::END_OF_BLOCK => Ok(true),
            _ => Err(entry.error()),
        }
    }

    #[inline(always)]
    fn write_literals(out: &mut [u8], pos: &mut usize, entry: Entry) {
        let bytes = (entry.value() as u16).to_le_bytes();
        out[*pos..*pos + 2].copy_from_slice(&bytes);
        *pos += entry.extra_bits() as usize;
    }

    #[cold]
    fn grow_output(out: &mut Vec<u8>, pos: usize, limit: usize) {
        let new_len = (pos + 64 * 1024)
            .min(out.capacity())
            .min(limit.saturating_add(OUTPUT_SLACK))
            .max(pos + OUTPUT_SLACK);
        out.resize(new_len, 0);
    }

    #[inline(always)]
    fn copy_match(
        out: &mut [u8],
        pos: usize,
        length: usize,
        distance: usize,
        bounds: Bounds,
    ) -> Result<(), Error> {
        if distance > pos - bounds.stream_start {
            return Err(Error::DistanceTooFarBack);
        }

        let src = pos - distance;

        if distance >= 8 {
            let mut i = 0;
            while i < length {
                out.copy_within(src + i..src + i + 8, pos + i);
                i += 8;
            }
        } else if distance == 1 {
            let byte = out[src];
            out[pos..pos + length].fill(byte);
        } else {
            let mut copied = 0;
            while copied < length {
                let chunk = (distance + copied).min(length - copied);
                out.copy_within(src..src + chunk, pos + copied);
                copied += chunk;
            }
        }

        Ok(())
    }
}
