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

    pub(crate) fn inflate(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let mut reader = BitReader::new(data);

        let capacity = data.len().saturating_mul(4).min(MAX_INITIAL_CAPACITY);
        let mut inflated_data: Vec<u8> = Vec::with_capacity(capacity);

        let mut b_final = 0u8;
        while b_final == 0 {
            b_final = reader.read_next_bit()?;
            let b_type = reader.read_next_bits(2)?;

            match b_type {
                0 => Self::inflate_stored_block(&mut reader, &mut inflated_data)?,
                1 => Self::inflate_fixed_huffman_block(&mut reader, &mut inflated_data)?,
                2 => self.inflate_dynamic_huffman_block(&mut reader, &mut inflated_data)?,
                _ => return Err(Error::InvalidBlockType),
            }
        }

        Ok(inflated_data)
    }

    fn inflate_stored_block(
        reader: &mut BitReader,
        inflated_data: &mut Vec<u8>,
    ) -> Result<(), Error> {
        reader.align_to_byte();

        let len = reader.read_next_bits(16)?;
        let n_len = reader.read_next_bits(16)?;

        if len ^ n_len != 0xFFFF {
            return Err(Error::StoredLengthMismatch);
        }

        inflated_data.extend_from_slice(reader.read_bytes(len as usize)?);

        Ok(())
    }

    fn inflate_fixed_huffman_block(
        reader: &mut BitReader,
        inflated_data: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let (literal_length_decoder, distance_decoder) = &*FIXED_DECODERS;

        Self::inflate_block(
            reader,
            literal_length_decoder,
            distance_decoder,
            inflated_data,
        )?;

        Ok(())
    }

    fn inflate_dynamic_huffman_block(
        &mut self,
        reader: &mut BitReader,
        inflated_data: &mut Vec<u8>,
    ) -> Result<(), Error> {
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

        Self::inflate_block(
            reader,
            &self.literal_length_decoder,
            &self.distance_decoder,
            inflated_data,
        )?;

        Ok(())
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

    fn inflate_block(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        inflated_data: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let result = Self::decode_symbols(
            reader,
            literal_length_decoder,
            distance_decoder,
            inflated_data,
        );
        reader.finish_buffered();
        result
    }

    fn decode_symbols(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        inflated_data: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let mut pos = inflated_data.len();
        let result = Self::decode_symbols_into(
            reader,
            literal_length_decoder,
            distance_decoder,
            inflated_data,
            &mut pos,
        );
        inflated_data.truncate(pos);
        result
    }

    #[inline(always)]
    fn decode_symbols_into(
        reader: &mut BitReader,
        literal_length_decoder: &HuffmanDecoder,
        distance_decoder: &HuffmanDecoder,
        out: &mut Vec<u8>,
        pos: &mut usize,
    ) -> Result<(), Error> {
        reader.refill_full();
        let mut entry = literal_length_decoder.lookup(reader.peek_buffer());

        loop {
            if out.len() - *pos < OUTPUT_SLACK {
                Self::grow_output(out, *pos);
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

                    Self::copy_match(out, *pos, length as usize, distance as usize)?;
                    *pos += length as usize;
                }
                Entry::END_OF_BLOCK => return Ok(()),
                _ => return Err(entry.error()),
            }
        }
    }

    #[inline(always)]
    fn write_literals(out: &mut [u8], pos: &mut usize, entry: Entry) {
        let bytes = (entry.value() as u16).to_le_bytes();
        out[*pos..*pos + 2].copy_from_slice(&bytes);
        *pos += entry.extra_bits() as usize;
    }

    #[cold]
    fn grow_output(out: &mut Vec<u8>, pos: usize) {
        let new_len = (pos + 64 * 1024)
            .min(out.capacity())
            .max(pos + OUTPUT_SLACK);
        out.resize(new_len, 0);
    }

    #[inline(always)]
    fn copy_match(out: &mut [u8], pos: usize, length: usize, distance: usize) -> Result<(), Error> {
        if distance > pos {
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
