use crate::io::bit_reader::BitReader;

const MAX_BITS: usize = 15;
const TABLE_BITS: usize = 9;
const TABLE_SIZE: usize = 1 << TABLE_BITS;

#[derive(Clone, Copy, Default)]
enum HuffmanEntry {
    #[default]
    Invalid,
    Symbol { symbol: u16, bits: u8 },
    Secondary { offset: u16, bits: u8 },
}

// Keeps the primary table at 2 KB so it stays in L1 cache.
const _: () = assert!(size_of::<HuffmanEntry>() == 4);

pub struct HuffmanDecoder {
    table: [HuffmanEntry; TABLE_SIZE],
    secondary: Vec<HuffmanEntry>,
}

impl HuffmanDecoder {
    pub fn new(code_lengths: &[u8]) -> Result<Self, String> {
        let mut bl_counts = [0u16; MAX_BITS + 1];
        for code_length in code_lengths {
            if *code_length as usize > MAX_BITS {
                return Err("invalid Huffman code length".to_string());
            }

            if *code_length > 0 {
                bl_counts[*code_length as usize] += 1;
            }
        }

        let mut left: i32 = 1;
        for bits in 1..=MAX_BITS {
            left = (left << 1) - bl_counts[bits] as i32;
            if left < 0 {
                return Err("over-subscribed Huffman code".to_string());
            }
        }

        let mut code = 0u16;
        let mut next_code = [0u16; MAX_BITS + 1];
        for bits in 1..=MAX_BITS {
            code = (code + bl_counts[bits - 1]) << 1;
            next_code[bits] = code;
        }

        let mut decoder = Self {
            table: [HuffmanEntry::Invalid; TABLE_SIZE],
            secondary: Vec::new(),
        };

        decoder.allocate_secondary_tables(code_lengths, next_code);

        for symbol in 0..code_lengths.len() {
            let bits = code_lengths[symbol] as usize;

            if bits == 0 {
                continue;
            }

            let canonical_code = next_code[bits];
            next_code[bits] += 1;

            let reversed_code = Self::reverse_bits(canonical_code, bits);

            if bits <= TABLE_BITS {
                decoder.insert_primary(reversed_code, bits, symbol as u16);
            } else {
                decoder.insert_secondary(reversed_code, bits, symbol as u16);
            }
        }

        Ok(decoder)
    }

    #[inline]
    fn reverse_bits(mut code: u16, length: usize) -> u16 {
        let mut reversed = 0u16;

        for _ in 0..length {
            reversed = (reversed << 1) | (code & 1);
            code >>= 1;
        }

        reversed
    }

    fn allocate_secondary_tables(&mut self, code_lengths: &[u8], mut next_code: [u16; MAX_BITS + 1]) {
        let mut max_bits = [0u8; TABLE_SIZE];

        for &code_length in code_lengths {
            let bits = code_length as usize;

            if bits == 0 {
                continue;
            }

            let canonical_code = next_code[bits];
            next_code[bits] += 1;

            if bits > TABLE_BITS {
                let primary_index = Self::reverse_bits(canonical_code, bits) as usize & (TABLE_SIZE - 1);
                max_bits[primary_index] = max_bits[primary_index].max(code_length);
            }
        }

        for primary_index in 0..TABLE_SIZE {
            if max_bits[primary_index] == 0 {
                continue;
            }

            let secondary_bits = max_bits[primary_index] as usize - TABLE_BITS;
            let offset = self.secondary.len();

            self.secondary.resize(offset + (1 << secondary_bits), HuffmanEntry::Invalid);
            self.table[primary_index] = HuffmanEntry::Secondary { offset: offset as u16, bits: secondary_bits as u8 };
        }
    }

    fn insert_primary(&mut self, code: u16, bits: usize, symbol: u16) {
        let fill_count = 1usize << (TABLE_BITS - bits);

        for i in 0..fill_count {
            self.table[code as usize | (i << bits)] = HuffmanEntry::Symbol { symbol, bits: bits as u8 };
        }
    }

    fn insert_secondary(&mut self, code: u16, bits: usize, symbol: u16) {
        let primary_index = code as usize & (TABLE_SIZE - 1);

        let HuffmanEntry::Secondary { offset, bits: table_bits } = self.table[primary_index] else {
            unreachable!("secondary table not allocated for primary index {primary_index}");
        };
        let offset = offset as usize;

        let secondary_bits = bits - TABLE_BITS;
        let remaining_code = (code as usize) >> TABLE_BITS;

        let fill_count = 1usize << (table_bits as usize - secondary_bits);

        for i in 0..fill_count {
            self.secondary[offset + (remaining_code | (i << secondary_bits))] = HuffmanEntry::Symbol { symbol, bits: bits as u8 };
        }
    }

    pub fn decode(&self, reader: &mut BitReader) -> Result<usize, String> {
        let primary_index = reader.peek_next_bits(TABLE_BITS as u32)? as usize;

        let entry = match self.table[primary_index] {
            HuffmanEntry::Secondary { offset, bits } => {
                let value = reader.peek_next_bits(TABLE_BITS as u32 + bits as u32)? as usize;
                self.secondary[offset as usize + (value >> TABLE_BITS)]
            }
            entry => entry,
        };

        match entry {
            HuffmanEntry::Symbol { symbol, bits } => {
                reader.skip_bits(bits as usize)?;
                Ok(symbol as usize)
            }
            _ => Err("invalid Huffman code".to_string()),
        }
    }
}
