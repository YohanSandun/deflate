use crate::io::bit_reader::{BitReader};
use crate::compression::huffman_decoder::{HuffmanDecoder};

pub struct Inflater<'a> {
    reader: BitReader<'a>
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
        let mut distance_length = [5u8; 32];

        for i in 144..256 {
            lengths[i] = 9;
        }

        for i in 256..280 {
            lengths[i] = 7;
        }

        

        Ok(())
    }

    fn inflate_dynamic_huffman_block(&mut self, inflated_data: &mut Vec<u8>) -> Result<(), String> {
        Ok(())
    }
}
