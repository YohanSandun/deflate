use crate::io::bit_reader::{BitReader};
use crate::compression::huffman_decoder::{HuffmanDecoder};

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

const CODE_LENGTH_ORDER: [u16; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15
];

pub struct Inflater<'a> {
    reader: BitReader<'a>,
}

impl<'a> Inflater<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            reader: BitReader::new(data)
        }
    }

    pub fn inflate(&mut self) -> Result<Vec<u8>, String> {
        let mut inflated_data: Vec<u8> = Vec::new();

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

        for _ in 0..len {
            inflated_data.push(self.reader.read_next_bits(8)? as u8);
        }

        Ok(())
    }

    fn inflate_fixed_huffman_block(&mut self, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        let mut lengths = [8u8; 288];
        let distance_length = [5u8; 32];

        for i in 144..256 {
            lengths[i] = 9;
        }

        for i in 256..280 {
            lengths[i] = 7;
        }

        let literal_length_decoder = HuffmanDecoder::new(&lengths)?;
        let distance_decoder = HuffmanDecoder::new(&distance_length)?;

        self.inflate_block(&literal_length_decoder, &distance_decoder, inflated_data)?;

        Ok(())
    }

    fn inflate_dynamic_huffman_block(&mut self, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        Ok(())
    }

    fn inflate_block(&mut self, literal_length_decoder: &HuffmanDecoder, distance_decoder: &HuffmanDecoder, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        let mut symbol = literal_length_decoder.decode(&mut self.reader)?;

        while symbol != 256 {
            if symbol < 256 {
                inflated_data.push(symbol as u8);
            } else {
                let length = self.decode_length(symbol)?;
                let distance_symbol = distance_decoder.decode(&mut self.reader)?;
                let distance = self.decode_distance(distance_symbol)?;
                self.copy_data_from_earlier(length, distance, inflated_data)?;
            }
            symbol = literal_length_decoder.decode(&mut self.reader)?;
        }

        Ok(())
    }

    #[inline]
    fn decode_length(&mut self, symbol: usize) -> Result<u16, String> {
        let length_index = symbol - 257;

        if length_index >= LENGTH_BASE.len() {
            return Err("invalid length symbol".to_string());
        }

        let mut length = LENGTH_BASE[length_index];
        let extra_bits = LENGTH_EXTRA_BITS[length_index];

        if extra_bits > 0 {
            length += self.reader.read_next_bits(extra_bits as u32)? as u16;
        }

        Ok(length)
    }

    #[inline]
    fn decode_distance(&mut self, symbol: usize) -> Result<u16, String> {
        if symbol >= DISTANCE_BASE.len() {
            return Err("invalid distance symbol".to_string());
        }

        let mut distance = DISTANCE_BASE[symbol];
        let extra_bits = DISTANCE_EXTRA_BITS[symbol];

        if extra_bits > 0 {
            distance += self.reader.read_next_bits(extra_bits as u32)? as u16;
        }

        Ok(distance)
    }

    #[inline]
    fn copy_data_from_earlier(&mut self, length: u16, distance: u16, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        if distance as usize > inflated_data.len() {
            return Err("invalid distance: too far back".to_string());
        }

        let pos = inflated_data.len() - distance as usize;

        for i in 0..length as usize {
            inflated_data.push(inflated_data[pos + i]);
        }

        Ok(())
    }
}
