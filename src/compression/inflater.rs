use std::sync::LazyLock;

use crate::io::bit_reader::{BitReader};
use crate::compression::huffman_decoder::{HuffmanDecoder};

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
        HuffmanDecoder::new(&lengths).expect("fixed literal/length code is valid"),
        HuffmanDecoder::new(&distance_length).expect("fixed distance code is valid"),
    )
});

const MAX_INITIAL_CAPACITY: usize = 64 << 20;

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10,
    11, 13, 15, 17,
    19, 23, 27, 31,
    35, 43, 51, 59,
    67, 83, 99, 115,
    131, 163, 195, 227,
    258,
];

const LENGTH_EXTRA_BITS: [u16; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0,
    1, 1, 1, 1,
    2, 2, 2, 2,
    3, 3, 3, 3,
    4, 4, 4, 4,
    5, 5, 5, 5,
    0
];

const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4,
    5, 7, 9, 13,
    17, 25, 33, 49,
    65, 97, 129, 193,
    257, 385, 513, 769,
    1025, 1537, 2049, 3073,
    4097, 6145, 8193, 12289,
    16385, 24577
];

const DISTANCE_EXTRA_BITS: [u16; 30] = [
    0, 0, 0, 0,
    1, 1, 2, 2,
    3, 3, 4, 4,
    5, 5, 6, 6,
    7, 7, 8, 8,
    9, 9, 10, 10,
    11, 11, 12, 12,
    13, 13
];

const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15
];

const MAX_LITERAL_LENGTH_CODES: usize = 286;
const MAX_DISTANCE_CODES: usize = 30;

pub struct Inflater<'a> {
    reader: BitReader<'a>,
    code_length_decoder: HuffmanDecoder,
    literal_length_decoder: HuffmanDecoder,
    distance_decoder: HuffmanDecoder,
}

impl<'a> Inflater<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            reader: BitReader::new(data),
            code_length_decoder: HuffmanDecoder::empty(),
            literal_length_decoder: HuffmanDecoder::empty(),
            distance_decoder: HuffmanDecoder::empty(),
        }
    }

    pub fn inflate(&mut self) -> Result<Vec<u8>, String> {
        let capacity = self.reader.input_len().saturating_mul(4).min(MAX_INITIAL_CAPACITY);
        let mut inflated_data: Vec<u8> = Vec::with_capacity(capacity);

        let mut b_final = 0u8;
        while b_final == 0 {
            b_final = self.reader.read_next_bit()?;
            let b_type = self.reader.read_next_bits(2)?;

            match b_type {
                0 => self.inflate_stored_block(&mut inflated_data)?,
                1 => self.inflate_fixed_huffman_block(&mut inflated_data)?,
                2 => self.inflate_dynamic_huffman_block(&mut inflated_data)?,
                _ => return Err("invalid flate block type".to_string())
            }
        }

        Ok(inflated_data)
    }

    fn inflate_stored_block(&mut self, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        self.reader.align_to_byte();

        let len = self.reader.read_next_bits(16)?;
        let n_len = self.reader.read_next_bits(16)?;

        if len ^ n_len != 0xFFFF {
            return Err("corrupted stored block".to_string());
        }

        inflated_data.extend_from_slice(self.reader.read_bytes(len as usize)?);

        Ok(())
    }

    fn inflate_fixed_huffman_block(&mut self, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        let (literal_length_decoder, distance_decoder) = &*FIXED_DECODERS;

        Self::inflate_block(&mut self.reader, literal_length_decoder, distance_decoder, inflated_data)?;

        Ok(())
    }

    fn inflate_dynamic_huffman_block(&mut self, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        let hlit = self.reader.read_next_bits(5)? as usize + 257;
        let hdist = self.reader.read_next_bits(5)? as usize + 1;
        let hclen = self.reader.read_next_bits(4)? as usize + 4;

        if hlit > MAX_LITERAL_LENGTH_CODES || hdist > MAX_DISTANCE_CODES {
            return Err("too many length or distance symbols".to_string());
        }

        let mut code_length_code_lengths = [0u8; 19];

        for &symbol in &CODE_LENGTH_ORDER[..hclen] {
            code_length_code_lengths[symbol] = self.reader.read_next_bits(3)? as u8;
        }

        self.code_length_decoder.rebuild(&code_length_code_lengths)?;

        let mut code_lengths = [0u8; MAX_LITERAL_LENGTH_CODES + MAX_DISTANCE_CODES];
        let total = hlit + hdist;

        let mut index = 0;
        while index < total {
            index = self.decode_code_length(&mut code_lengths[..total], index)?;
        }

        let (literal_length_code_lengths, distance_code_lengths) = code_lengths[..total].split_at(hlit);

        if literal_length_code_lengths[256] == 0 {
            return Err("missing end-of-block code".to_string());
        }

        self.literal_length_decoder.rebuild(literal_length_code_lengths)?;
        self.distance_decoder.rebuild(distance_code_lengths)?;

        Self::inflate_block(&mut self.reader, &self.literal_length_decoder, &self.distance_decoder, inflated_data)?;

        Ok(())
    }

    fn decode_code_length(&mut self, code_lengths: &mut [u8], index: usize) -> Result<usize, String> {
        let symbol = self.code_length_decoder.decode(&mut self.reader)?;

        let (code_length, repeat) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => {
                if index == 0 {
                    return Err("repeat with no previous code length".to_string());
                }
                (code_lengths[index - 1], self.reader.read_next_bits(2)? as usize + 3)
            }
            17 => (0, self.reader.read_next_bits(3)? as usize + 3),
            18 => (0, self.reader.read_next_bits(7)? as usize + 11),
            _ => unreachable!("code-length alphabet has 19 symbols"),
        };

        if repeat > code_lengths.len() - index {
            return Err("code length repeat past end".to_string());
        }

        code_lengths[index..index + repeat].fill(code_length);

        Ok(index + repeat)
    }
    
    fn inflate_block(reader: &mut BitReader, literal_length_decoder: &HuffmanDecoder, distance_decoder: &HuffmanDecoder, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        let mut symbol = literal_length_decoder.decode(reader)?;

        while symbol != 256 {
            if symbol < 256 {
                inflated_data.push(symbol as u8);
            } else {
                let length = Self::decode_length(reader, symbol)?;
                let distance_symbol = distance_decoder.decode(reader)?;
                let distance = Self::decode_distance(reader, distance_symbol)?;
                Self::copy_data_from_earlier(length, distance, inflated_data)?;
            }
            symbol = literal_length_decoder.decode(reader)?;
        }

        Ok(())
    }

    #[inline]
    fn decode_length(reader: &mut BitReader, symbol: usize) -> Result<u16, String> {
        let length_index = symbol - 257;

        if length_index >= LENGTH_BASE.len() {
            return Err("invalid length symbol".to_string());
        }

        let mut length = LENGTH_BASE[length_index];
        let extra_bits = LENGTH_EXTRA_BITS[length_index];

        if extra_bits > 0 {
            length += reader.read_next_bits(extra_bits as u32)? as u16;
        }

        Ok(length)
    }

    #[inline]
    fn decode_distance(reader: &mut BitReader, symbol: usize) -> Result<u16, String> {
        if symbol >= DISTANCE_BASE.len() {
            return Err("invalid distance symbol".to_string());
        }

        let mut distance = DISTANCE_BASE[symbol];
        let extra_bits = DISTANCE_EXTRA_BITS[symbol];

        if extra_bits > 0 {
            distance += reader.read_next_bits(extra_bits as u32)? as u16;
        }

        Ok(distance)
    }

    #[inline]
    fn copy_data_from_earlier(length: u16, distance: u16, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        if distance as usize > inflated_data.len() {
            return Err("invalid distance: too far back".to_string());
        }

        let length = length as usize;
        let distance = distance as usize;
        let pos = inflated_data.len() - distance;

        inflated_data.reserve(length);

        if distance >= length {
            inflated_data.extend_from_within(pos..pos + length);
        } else if distance == 1 {
            let byte = inflated_data[pos];
            inflated_data.resize(inflated_data.len() + length, byte);
        } else {
            let mut copied = 0;
            while copied < length {
                let chunk = (inflated_data.len() - pos).min(length - copied);
                inflated_data.extend_from_within(pos..pos + chunk);
                copied += chunk;
            }
        }

        Ok(())
    }
}
