use crate::io::bit_reader::BitReader;

const MAX_BITS: usize = 15;
const TABLE_BITS: usize = 9;
const TABLE_SIZE: usize = 1 << TABLE_BITS;
const SECONDARY_BITS: usize = MAX_BITS - TABLE_BITS;

struct HuffmanSecondaryEntry {
    bits: usize,
    table: [Option<HuffmanEntry>; 1 << SECONDARY_BITS],
}

struct HuffmanEntry {
    symbol: usize,
    bits: usize,
    secondary: Option<Box<HuffmanSecondaryEntry>>,
}

pub struct HuffmanDecoder {
    table: [Option<HuffmanEntry>; TABLE_SIZE]
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

        let mut table: [Option<HuffmanEntry>; TABLE_SIZE] = [const { None }; TABLE_SIZE];
        for symbol in 0..code_lengths.len() {
            let bits = code_lengths[symbol] as usize;

            if bits == 0 {
                continue;
            }

            let canonical_code = next_code[bits];
            next_code[bits] += 1;

            let reversed_code = Self::reverse_bits(canonical_code, bits);

            if bits <= TABLE_BITS {
                Self::insert_primary(&mut table, reversed_code, bits, symbol);
            } else {
                Self::insert_secondary(&mut table, reversed_code, bits, symbol);
            }
        }

        Ok(Self {
            table
        })
    }

    fn reverse_bits(mut code: u16, length: usize) -> u16 {
        let mut reversed = 0u16;

        for _ in 0..length {
            reversed = (reversed << 1) | (code & 1);
            code >>= 1;
        }

        reversed
    }

    fn insert_primary(table: &mut [Option<HuffmanEntry>], code: u16, bits: usize, symbol: usize) {
        let fill_count = 1usize << (TABLE_BITS - bits);

        for i in 0..fill_count {
            table[code as usize | (i << bits)] = Some(HuffmanEntry {
                symbol,
                bits,
                secondary: None
            });
        }
    }

    fn insert_secondary(table: &mut [Option<HuffmanEntry>], code: u16, bits: usize, symbol: usize) {
        let primary_index = code as usize & (TABLE_SIZE - 1);

        let entry = table[primary_index].get_or_insert(HuffmanEntry {
            symbol: 0,
            bits: 0,
            secondary: None,
        });

        let secondary = entry.secondary.get_or_insert_with(|| Box::new(HuffmanSecondaryEntry {
            bits: SECONDARY_BITS,
            table: [const { None }; 1 << SECONDARY_BITS],
        }));

        let secondary_bits = bits - TABLE_BITS;
        let remaining_code = (code as usize) >> TABLE_BITS;

        let fill_count = 1usize << (secondary.bits - secondary_bits);

        for i in 0..fill_count {
            secondary.table[remaining_code | (i << secondary_bits)] = Some(HuffmanEntry {
                symbol,
                bits,
                secondary: None
            });
        }
    }

    pub fn decode(&self, reader: &mut BitReader) -> Result<usize, String> {
        let primary_index = reader.peek_next_bits(TABLE_BITS as u32)?;

        if self.table[primary_index as usize].is_none() {
            return Err("invalid Huffman code".to_string());
        }

        let entry = self.table[primary_index as usize].as_ref().unwrap();

        if entry.secondary.is_none() {
            reader.skip_bits(entry.bits)?;
            return Ok(entry.symbol);
        }

        let entry_secondary = entry.secondary.as_ref().unwrap();

        let value = reader.peek_next_bits((TABLE_BITS + entry_secondary.bits) as u32)?;
        let secondary_index = (value >> TABLE_BITS) as usize;

        if let Some(secondary_entry) = &entry_secondary.table[secondary_index] {
            reader.skip_bits(secondary_entry.bits)?;
            Ok(secondary_entry.symbol)
        } else {
            Err("invalid Huffman code".to_string())
        }
    }
}