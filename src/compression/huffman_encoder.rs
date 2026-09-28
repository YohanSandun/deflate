use crate::compression::lz77::Token;
use crate::compression::tables::{
    CODE_LENGTH_ORDER, DISTANCE_BASE, DISTANCE_EXTRA_BITS, LENGTH_BASE, LENGTH_EXTRA_BITS,
    MAX_LITERAL_LENGTH_CODES,
};

pub(crate) const LITERAL_LENGTH_SYMBOLS: usize = 288;
pub(crate) const DISTANCE_SYMBOLS: usize = 30;

pub(crate) const END_OF_BLOCK: usize = 256;

pub(crate) const MAX_BITS: usize = 15;

pub(crate) const CODE_LENGTH_SYMBOLS: usize = 19;

pub(crate) const MAX_CODE_LENGTH_BITS: usize = 7;

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

pub(crate) fn build_lengths(frequencies: &[u32], max_bits: usize, lengths: &mut [u8]) {
    debug_assert_eq!(frequencies.len(), lengths.len(), "one length per symbol");
    debug_assert!(frequencies.len() >= 2, "need room for two codes");
    debug_assert!(frequencies.len() <= 1 << max_bits, "too many symbols for {max_bits} bits");

    lengths.fill(0);

    // Used symbols, least frequent first (ties by symbol, so the result is stable).
    let mut symbols: Vec<usize> = (0..frequencies.len())
        .filter(|&symbol| frequencies[symbol] > 0)
        .collect();
    symbols.sort_unstable_by_key(|&symbol| (frequencies[symbol], symbol));

    if symbols.len() < 2 {
        let used = symbols.first().copied();
        let other = if used == Some(0) { 1 } else { 0 };
        lengths[used.unwrap_or(1)] = 1;
        lengths[other] = 1;
        return;
    }

    let counts = length_counts(frequencies, &symbols, max_bits);

    // The longest codes go to the least frequent symbols.
    let mut next = symbols.iter();
    for len in (1..=max_bits).rev() {
        for &symbol in next.by_ref().take(counts[len] as usize) {
            lengths[symbol] = len as u8;
        }
    }
}

fn length_counts(frequencies: &[u32], symbols: &[usize], max_bits: usize) -> [u32; MAX_BITS + 1] {
    let n = symbols.len();
    let mut weight: Vec<u64> = symbols.iter().map(|&s| frequencies[s] as u64).collect();
    let mut parent = vec![0usize; 2 * n - 1];
    weight.reserve(n - 1);

    let (mut leaf, mut merged) = (0, n);
    for node in n..2 * n - 1 {
        let mut lightest = || {
            // On a tie, the leaf: it keeps the tree shallower.
            let pick = if leaf < n && (merged >= node || weight[leaf] <= weight[merged]) {
                leaf += 1;
                leaf - 1
            } else {
                merged += 1;
                merged - 1
            };
            (pick, weight[pick])
        };
        let (a, weight_a) = lightest();
        let (b, weight_b) = lightest();
        parent[a] = node;
        parent[b] = node;
        weight.push(weight_a + weight_b);
    }

    // Parents come after their children, so one backward pass gives every depth.
    let mut depth = vec![0usize; 2 * n - 1];
    let mut counts = [0u32; MAX_BITS + 1];
    for node in (0..2 * n - 2).rev() {
        depth[node] = depth[parent[node]] + 1;
        if node < n {
            counts[depth[node].min(max_bits)] += 1;
        }
    }

    let mut kraft: u64 = (1..=max_bits)
        .map(|len| (counts[len] as u64) << (max_bits - len))
        .sum();
    while kraft > 1 << max_bits {
        counts[max_bits] -= 1;
        let len = (1..max_bits).rev().find(|&len| counts[len] > 0).unwrap();
        counts[len] -= 1;
        counts[len + 1] += 2;
        kraft -= 1;
    }
    counts
}

pub(crate) fn encode_code_lengths(lengths: &[u8], out: &mut Vec<Symbol>) {
    let plain = |len: u8| Symbol {
        symbol: len as u16,
        extra_bits: 0,
        extra: 0,
    };
    let repeat = |symbol: u16, extra_bits: u8, extra: usize| Symbol {
        symbol,
        extra_bits,
        extra: extra as u16,
    };

    let mut pos = 0;
    while pos < lengths.len() {
        let len = lengths[pos];
        let run = lengths[pos..].iter().take_while(|&&l| l == len).count();
        pos += run;

        let mut left = run;
        if len == 0 {
            while left >= 11 {
                let count = left.min(138);
                out.push(repeat(18, 7, count - 11));
                left -= count;
            }
            if left >= 3 {
                out.push(repeat(17, 3, left - 3));
                left = 0;
            }
        } else {
            out.push(plain(len));
            left -= 1;
            while left >= 3 {
                let count = left.min(6);
                out.push(repeat(16, 2, count - 3));
                left -= count;
            }
        }
        out.extend(std::iter::repeat_n(plain(len), left));
    }
}

/// The codes for one dynamic Huffman block (RFC 1951 section 3.2.7), built from the
/// block's tokens.
pub(crate) struct DynamicCodes {
    pub(crate) literal_length: [Code; LITERAL_LENGTH_SYMBOLS],
    pub(crate) distance: [Code; DISTANCE_SYMBOLS],
    pub(crate) code_length: [Code; CODE_LENGTH_SYMBOLS],
    pub(crate) literal_length_count: usize,
    pub(crate) distance_count: usize,
    pub(crate) code_length_count: usize,
    pub(crate) code_length_symbols: Vec<Symbol>,

    literal_length_frequencies: [u32; MAX_LITERAL_LENGTH_CODES],
    distance_frequencies: [u32; DISTANCE_SYMBOLS],
    code_length_frequencies: [u32; CODE_LENGTH_SYMBOLS],
    lengths: [u8; MAX_LITERAL_LENGTH_CODES + DISTANCE_SYMBOLS],
}

impl DynamicCodes {
    pub(crate) fn new() -> Self {
        Self {
            literal_length: [Code::default(); LITERAL_LENGTH_SYMBOLS],
            distance: [Code::default(); DISTANCE_SYMBOLS],
            code_length: [Code::default(); CODE_LENGTH_SYMBOLS],
            literal_length_count: 0,
            distance_count: 0,
            code_length_count: 0,
            code_length_symbols: Vec::new(),
            literal_length_frequencies: [0; MAX_LITERAL_LENGTH_CODES],
            distance_frequencies: [0; DISTANCE_SYMBOLS],
            code_length_frequencies: [0; CODE_LENGTH_SYMBOLS],
            lengths: [0; MAX_LITERAL_LENGTH_CODES + DISTANCE_SYMBOLS],
        }
    }

    /// Rebuilds every field for a block holding `tokens`.
    pub(crate) fn build(&mut self, tokens: &[Token]) {
        self.literal_length_frequencies.fill(0);
        self.distance_frequencies.fill(0);
        for token in tokens {
            match *token {
                Token::Literal(byte) => self.literal_length_frequencies[byte as usize] += 1,
                Token::Match { length, distance } => {
                    self.literal_length_frequencies[length_symbol(length).symbol as usize] += 1;
                    self.distance_frequencies[distance_symbol(distance).symbol as usize] += 1;
                }
            }
        }
        self.literal_length_frequencies[END_OF_BLOCK] += 1;

        let (literal_length_lengths, distance_lengths) =
            self.lengths.split_at_mut(MAX_LITERAL_LENGTH_CODES);
        build_lengths(&self.literal_length_frequencies, MAX_BITS, literal_length_lengths);
        build_lengths(&self.distance_frequencies, MAX_BITS, distance_lengths);
        canonical_codes(
            literal_length_lengths,
            &mut self.literal_length[..MAX_LITERAL_LENGTH_CODES],
        );
        self.literal_length[MAX_LITERAL_LENGTH_CODES..].fill(Code::default());
        canonical_codes(distance_lengths, &mut self.distance);

        self.literal_length_count = used_count(literal_length_lengths).max(257);
        self.distance_count = used_count(distance_lengths).max(1);

        let sent = self.literal_length_count + self.distance_count;
        self.lengths.copy_within(
            MAX_LITERAL_LENGTH_CODES..MAX_LITERAL_LENGTH_CODES + self.distance_count,
            self.literal_length_count,
        );
        self.code_length_symbols.clear();
        encode_code_lengths(&self.lengths[..sent], &mut self.code_length_symbols);

        self.code_length_frequencies.fill(0);
        for s in &self.code_length_symbols {
            self.code_length_frequencies[s.symbol as usize] += 1;
        }
        let mut code_length_lengths = [0u8; CODE_LENGTH_SYMBOLS];
        build_lengths(
            &self.code_length_frequencies,
            MAX_CODE_LENGTH_BITS,
            &mut code_length_lengths,
        );
        canonical_codes(&code_length_lengths, &mut self.code_length);

        let ordered = CODE_LENGTH_ORDER.map(|symbol| code_length_lengths[symbol]);
        self.code_length_count = used_count(&ordered).max(4);
    }
}

/// How many lengths are left once trailing zeros are dropped.
fn used_count(lengths: &[u8]) -> usize {
    lengths.iter().rposition(|&len| len > 0).map_or(0, |last| last + 1)
}

#[cfg(test)]
mod tests;
