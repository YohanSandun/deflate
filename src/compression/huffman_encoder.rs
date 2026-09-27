use crate::compression::tables::{
    DISTANCE_BASE, DISTANCE_EXTRA_BITS, LENGTH_BASE, LENGTH_EXTRA_BITS,
};

pub(crate) const LITERAL_LENGTH_SYMBOLS: usize = 288;
pub(crate) const DISTANCE_SYMBOLS: usize = 30;

pub(crate) const END_OF_BLOCK: usize = 256;

pub(crate) const MAX_BITS: usize = 15;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Code {
    pub(crate) bits: u16,
    pub(crate) len: u8,
}

pub(crate) fn canonical_codes(lengths: &[u8], codes: &mut [Code]) {
    debug_assert_eq!(lengths.len(), codes.len(), "one code per symbol");

    let mut bl_counts = [0u16; MAX_BITS + 1];
    for &len in lengths {
        debug_assert!(len as usize <= MAX_BITS, "code length {len} over {MAX_BITS}");

        if len > 0 {
            bl_counts[len as usize] += 1;
        }
    }

    let mut code = 0u32;
    let mut next_code = [0u32; MAX_BITS + 1];
    for bits in 1..=MAX_BITS {
        code = (code + bl_counts[bits - 1] as u32) << 1;
        next_code[bits] = code;
    }

    for (&len, out) in lengths.iter().zip(codes.iter_mut()) {
        if len == 0 {
            *out = Code::default();
            continue;
        }

        let code = next_code[len as usize];
        next_code[len as usize] += 1;

        // Only holds if the lengths are a valid (not over-subscribed) code.
        debug_assert!(code < 1 << len, "over-subscribed code lengths");

        *out = Code {
            bits: (code as u16).reverse_bits() >> (16 - len as u32),
            len,
        };
    }
}

/// The fixed Huffman codes (RFC 1951 section 3.2.6).
pub(crate) struct FixedCodes {
    pub(crate) literal_length: [Code; LITERAL_LENGTH_SYMBOLS],
    pub(crate) distance: [Code; DISTANCE_SYMBOLS],
}

impl FixedCodes {
    pub(crate) fn new() -> Self {
        let mut lengths = [0u8; LITERAL_LENGTH_SYMBOLS];
        lengths[0..144].fill(8);
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        lengths[280..288].fill(8);

        let mut codes = Self {
            literal_length: [Code::default(); LITERAL_LENGTH_SYMBOLS],
            distance: [Code::default(); DISTANCE_SYMBOLS],
        };
        canonical_codes(&lengths, &mut codes.literal_length);
        canonical_codes(&[5; DISTANCE_SYMBOLS], &mut codes.distance);
        codes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Symbol {
    pub(crate) symbol: u16,
    pub(crate) extra_bits: u8,
    pub(crate) extra: u16,
}

const LENGTH_INDEX: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut index = 0;
    // In order, so 258 (its own code) overwrites the end of 284's range.
    while index < LENGTH_BASE.len() {
        let start = LENGTH_BASE[index] as usize - 3;
        let mut offset = 0;
        while offset < 1 << LENGTH_EXTRA_BITS[index] && start + offset < table.len() {
            table[start + offset] = index as u8;
            offset += 1;
        }
        index += 1;
    }
    table
};

const DISTANCE_INDEX: [u8; 512] = {
    let mut table = [0u8; 512];
    let mut index = 0;
    while index < DISTANCE_BASE.len() {
        let start = DISTANCE_BASE[index] as usize - 1;
        let end = start + (1 << DISTANCE_EXTRA_BITS[index]);
        let mut value = start;
        while value < end {
            if value < 256 {
                table[value] = index as u8;
                value += 1;
            } else {
                table[256 + (value >> 7)] = index as u8;
                value += 128;
            }
        }
        index += 1;
    }
    table
};

#[inline]
pub(crate) fn length_symbol(length: u16) -> Symbol {
    debug_assert!((3..=258).contains(&length), "match length {length}");

    let index = LENGTH_INDEX[(length - 3) as usize] as usize;
    Symbol {
        symbol: 257 + index as u16,
        extra_bits: LENGTH_EXTRA_BITS[index],
        extra: length - LENGTH_BASE[index],
    }
}

#[inline]
pub(crate) fn distance_symbol(distance: u16) -> Symbol {
    debug_assert!((1..=32_768).contains(&distance), "match distance {distance}");

    let value = (distance - 1) as usize;
    let index = if value < 256 {
        DISTANCE_INDEX[value]
    } else {
        DISTANCE_INDEX[256 + (value >> 7)]
    } as usize;
    Symbol {
        symbol: index as u16,
        extra_bits: DISTANCE_EXTRA_BITS[index],
        extra: distance - DISTANCE_BASE[index],
    }
}

#[cfg(test)]
mod tests;
