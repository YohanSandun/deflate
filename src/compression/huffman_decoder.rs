use crate::compression::tables::{DISTANCE_BASE, DISTANCE_EXTRA_BITS, LENGTH_BASE, LENGTH_EXTRA_BITS};
use crate::io::bit_reader::BitReader;

const MAX_BITS: usize = 15;
const TABLE_BITS: usize = 9;
const TABLE_SIZE: usize = 1 << TABLE_BITS;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alphabet {
    Symbols,
    LiteralLength,
    Distance,
}

/// One table entry packed into a u32, keeping the primary table at 2 KB:
///
/// | bits   | field                                                        |
/// |--------|--------------------------------------------------------------|
/// | 0..5   | code length; for `SECONDARY`, the secondary table's index width |
/// | 5..9   | extra bits to read after the code (`BASE` only)               |
/// | 9..12  | kind                                                         |
/// | 16..32 | value: symbol, literal byte, base, or secondary table offset  |
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry(u32);

impl Entry {
    pub(crate) const INVALID: u32 = 0;
    pub(crate) const LITERAL: u32 = 1;
    pub(crate) const END_OF_BLOCK: u32 = 2;
    pub(crate) const BASE: u32 = 3;
    pub(crate) const SECONDARY: u32 = 4;
    pub(crate) const BAD_LENGTH: u32 = 5;
    pub(crate) const BAD_DISTANCE: u32 = 6;

    const INVALID_ENTRY: Entry = Entry(Self::INVALID << 9);

    #[inline]
    const fn new(kind: u32, code_length: usize, extra_bits: u8, value: u16) -> Self {
        Entry(code_length as u32 | (extra_bits as u32) << 5 | kind << 9 | (value as u32) << 16)
    }

    #[inline]
    pub(crate) fn code_length(self) -> u32 {
        self.0 & 0x1F
    }

    #[inline]
    pub(crate) fn extra_bits(self) -> u32 {
        (self.0 >> 5) & 0xF
    }

    #[inline]
    pub(crate) fn kind(self) -> u32 {
        (self.0 >> 9) & 0x7
    }

    #[inline]
    pub(crate) fn value(self) -> u32 {
        self.0 >> 16
    }

    #[cold]
    pub(crate) fn error(self) -> String {
        match self.kind() {
            Self::BAD_LENGTH => "invalid length symbol",
            Self::BAD_DISTANCE => "invalid distance symbol",
            _ => "invalid Huffman code",
        }
        .to_string()
    }
}

const _: () = assert!(size_of::<Entry>() == 4);

pub struct HuffmanDecoder {
    table: [Entry; TABLE_SIZE],
    secondary: Vec<Entry>,
}

impl HuffmanDecoder {
    pub fn new(code_lengths: &[u8]) -> Result<Self, String> {
        Self::new_for(code_lengths, Alphabet::Symbols)
    }

    pub(crate) fn new_for(code_lengths: &[u8], alphabet: Alphabet) -> Result<Self, String> {
        let mut decoder = Self::empty();
        decoder.rebuild_for(code_lengths, alphabet)?;
        Ok(decoder)
    }

    pub fn empty() -> Self {
        Self {
            table: [Entry::INVALID_ENTRY; TABLE_SIZE],
            secondary: Vec::new(),
        }
    }

    pub fn rebuild(&mut self, code_lengths: &[u8]) -> Result<(), String> {
        self.rebuild_for(code_lengths, Alphabet::Symbols)
    }

    pub(crate) fn rebuild_for(&mut self, code_lengths: &[u8], alphabet: Alphabet) -> Result<(), String> {
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

        self.table.fill(Entry::INVALID_ENTRY);
        self.secondary.clear();

        self.allocate_secondary_tables(code_lengths, next_code);

        for symbol in 0..code_lengths.len() {
            let bits = code_lengths[symbol] as usize;

            if bits == 0 {
                continue;
            }

            let canonical_code = next_code[bits];
            next_code[bits] += 1;

            let reversed_code = Self::reverse_bits(canonical_code, bits);
            let entry = Self::symbol_entry(alphabet, symbol, bits);

            if bits <= TABLE_BITS {
                self.insert_primary(reversed_code, bits, entry);
            } else {
                self.insert_secondary(reversed_code, bits, entry);
            }
        }

        Ok(())
    }

    fn symbol_entry(alphabet: Alphabet, symbol: usize, bits: usize) -> Entry {
        match alphabet {
            Alphabet::Symbols => Entry::new(Entry::LITERAL, bits, 0, symbol as u16),
            Alphabet::LiteralLength => match symbol {
                0..=255 => Entry::new(Entry::LITERAL, bits, 0, symbol as u16),
                256 => Entry::new(Entry::END_OF_BLOCK, bits, 0, 0),
                257..=285 => Entry::new(Entry::BASE, bits, LENGTH_EXTRA_BITS[symbol - 257], LENGTH_BASE[symbol - 257]),
                _ => Entry::new(Entry::BAD_LENGTH, bits, 0, 0),
            },
            Alphabet::Distance => match symbol {
                0..=29 => Entry::new(Entry::BASE, bits, DISTANCE_EXTRA_BITS[symbol], DISTANCE_BASE[symbol]),
                _ => Entry::new(Entry::BAD_DISTANCE, bits, 0, 0),
            },
        }
    }

    #[inline]
    fn reverse_bits(code: u16, length: usize) -> u16 {
        code.reverse_bits() >> (16 - length)
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

            self.secondary.resize(offset + (1 << secondary_bits), Entry::INVALID_ENTRY);
            self.table[primary_index] = Entry::new(Entry::SECONDARY, secondary_bits, 0, offset as u16);
        }
    }

    fn insert_primary(&mut self, code: u16, bits: usize, entry: Entry) {
        let fill_count = 1usize << (TABLE_BITS - bits);

        for i in 0..fill_count {
            self.table[code as usize | (i << bits)] = entry;
        }
    }

    fn insert_secondary(&mut self, code: u16, bits: usize, entry: Entry) {
        let primary_index = code as usize & (TABLE_SIZE - 1);

        let link = self.table[primary_index];
        if link.kind() != Entry::SECONDARY {
            unreachable!("secondary table not allocated");
        }
        let offset = link.value() as usize;
        let table_bits = link.code_length() as usize;

        let secondary_bits = bits - TABLE_BITS;
        let remaining_code = (code as usize) >> TABLE_BITS;

        let fill_count = 1usize << (table_bits - secondary_bits);

        for i in 0..fill_count {
            self.secondary[offset + (remaining_code | (i << secondary_bits))] = entry;
        }
    }

    #[inline(always)]
    pub(crate) fn lookup(&self, bits: u64) -> Entry {
        let entry = self.table[bits as usize & (TABLE_SIZE - 1)];

        if entry.kind() != Entry::SECONDARY {
            return entry;
        }

        let index = (bits as usize >> TABLE_BITS) & ((1 << entry.code_length()) - 1);
        self.secondary[entry.value() as usize + index]
    }

    pub fn decode(&self, reader: &mut BitReader) -> Result<usize, String> {
        let entry = self.lookup(reader.peek_buffer());

        if entry.kind() != Entry::LITERAL {
            return Err(entry.error());
        }

        reader.skip_bits(entry.code_length() as usize)?;
        Ok(entry.value() as usize)
    }
}
