use crate::compression::deflater::MAX_STORED_BLOCK;
use crate::compression::huffman_encoder::{DynamicCodes, FixedCodes, Frequencies};
use crate::compression::lz77::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockType {
    Stored,
    Fixed,
    Dynamic,
}

/// Levels 0 to 7 grow each block by this many tokens at a time.
const CHUNK_TOKENS: usize = 4096;

/// How hard levels 8 and 9 search for the best split.
#[derive(Clone, Copy)]
struct Search {
    /// Block boundaries are first chosen among at most this many evenly spaced
    /// positions: the search takes time quadratic in it.
    max_candidates: usize,
    /// The spacing of those positions is never below this many tokens.
    min_spacing: usize,
    /// Then move each boundary to the best position near it, token by token.
    refine: bool,
}

const LEVEL_8: Search = Search {
    max_candidates: 128,
    min_spacing: 1024,
    refine: false,
};

const LEVEL_9: Search = Search {
    max_candidates: 256,
    min_spacing: 128,
    refine: true,
};

/// Where a stored block is assumed to start within a byte while the real position
/// isn't known: its header and padding then take 7 bits, about the average.
const ASSUMED_BIT_POSITION: usize = 1;

/// The size of the stored blocks for `byte_len` bytes, starting at bit
/// `bit_position` of a byte. More than `MAX_STORED_BLOCK` bytes take several.
pub(crate) fn stored_bits(byte_len: usize, bit_position: usize) -> u64 {
    let blocks = byte_len.div_ceil(MAX_STORED_BLOCK).max(1) as u64;
    let padding = (8 - (bit_position + 3) % 8) % 8;
    // Blocks after the first start byte-aligned: 3 header bits, then 5 of padding.
    (3 + padding) as u64 + (blocks - 1) * 8 + blocks * 32 + byte_len as u64 * 8
}

/// Sizes up blocks as each type. Holds the codes it builds along the way: after
/// `cheapest` picks `Dynamic`, `dynamic` holds that block's codes.
pub(crate) struct BlockCosts {
    pub(crate) fixed: FixedCodes,
    pub(crate) dynamic: DynamicCodes,
}

impl BlockCosts {
    pub(crate) fn new() -> Self {
        Self {
            fixed: FixedCodes::new(),
            dynamic: DynamicCodes::new(),
        }
    }

    /// The smallest block type for tokens with these counts, covering `byte_len`
    /// bytes and starting at bit `bit_position` of a byte, and its size in bits.
    pub(crate) fn cheapest(
        &mut self,
        frequencies: &Frequencies,
        byte_len: usize,
        bit_position: usize,
    ) -> (BlockType, u64) {
        let cheapest = self.sizes(frequencies, byte_len, bit_position);
        if cheapest.0 == BlockType::Dynamic {
            self.dynamic.build_from(frequencies);
        }
        cheapest
    }

    fn estimate(&mut self, frequencies: &Frequencies, byte_len: usize) -> u64 {
        self.sizes(frequencies, byte_len, ASSUMED_BIT_POSITION).1
    }

    /// Like `cheapest`, without building the dynamic codes.
    fn sizes(
        &mut self,
        frequencies: &Frequencies,
        byte_len: usize,
        bit_position: usize,
    ) -> (BlockType, u64) {
        let stored = stored_bits(byte_len, bit_position);
        let fixed = self.fixed.block_bits(frequencies);
        let dynamic = self.dynamic.estimate_bits(frequencies);

        if dynamic < fixed && dynamic < stored {
            (BlockType::Dynamic, dynamic)
        } else if fixed <= stored {
            (BlockType::Fixed, fixed)
        } else {
            (BlockType::Stored, stored)
        }
    }
}

/// Splits `tokens` into blocks, writing where each one ends into `ends`: token
/// indices, the last being `tokens.len()`. There's always at least one block.
pub(crate) fn split(tokens: &[Token], level: u8, costs: &mut BlockCosts, ends: &mut Vec<usize>) {
    ends.clear();
    match level {
        9.. => optimal(tokens, costs, LEVEL_9, ends),
        8 => optimal(tokens, costs, LEVEL_8, ends),
        _ => greedy(tokens, costs, ends),
    }
}

pub(crate) fn byte_len(tokens: &[Token]) -> usize {
    tokens.iter().map(|token| token.byte_len()).sum()
}

/// Grows a block chunk by chunk, starting a new one whenever the next chunk is
/// smaller on its own than added to the block.
fn greedy(tokens: &[Token], costs: &mut BlockCosts, ends: &mut Vec<usize>) {
    let mut block = Frequencies::new();
    let mut block_bytes = 0;
    let mut block_bits = 0;

    let mut pos = 0;
    for chunk in tokens.chunks(CHUNK_TOKENS) {
        let frequencies = Frequencies::of(chunk);
        let bytes = byte_len(chunk);
        let bits = costs.estimate(&frequencies, bytes);

        if pos > 0 {
            let mut merged = block.clone();
            merged.add_all(&frequencies);
            let merged_bits = costs.estimate(&merged, block_bytes + bytes);
            if merged_bits <= block_bits + bits {
                block = merged;
                block_bytes += bytes;
                block_bits = merged_bits;
                pos += chunk.len();
                continue;
            }
            ends.push(pos);
        }

        block = frequencies;
        block_bytes = bytes;
        block_bits = bits;
        pos += chunk.len();
    }

    ends.push(tokens.len());
}

/// Splits long inputs into segments of this many tokens and searches each on its
/// own, so candidates stay close together and the time grows linearly.
const SEGMENT_TOKENS: usize = 1 << 18;

fn optimal(tokens: &[Token], costs: &mut BlockCosts, search: Search, ends: &mut Vec<usize>) {
    if tokens.is_empty() {
        ends.push(0);
        return;
    }

    let mut offset = 0;
    for segment in tokens.chunks(SEGMENT_TOKENS) {
        let first = ends.len();
        optimal_segment(segment, costs, search, ends);
        for end in &mut ends[first..] {
            *end += offset;
        }
        offset += segment.len();
    }
}

/// The cheapest split with boundaries among evenly spaced candidate positions
/// (dynamic programming over every pair), then optionally refined. Appends the
/// block ends, relative to `tokens`.
fn optimal_segment(
    tokens: &[Token],
    costs: &mut BlockCosts,
    search: Search,
    ends: &mut Vec<usize>,
) {
    let spacing = tokens
        .len()
        .div_ceil(search.max_candidates)
        .max(search.min_spacing);
    let totals = RunningTotals::new(tokens, spacing);

    let positions: Vec<usize> = (0..tokens.len())
        .step_by(spacing)
        .chain([tokens.len()])
        .collect();
    if positions.len() <= 2 {
        ends.push(tokens.len());
        return;
    }
    let at: Vec<(Frequencies, usize)> = positions.iter().map(|&pos| totals.at(pos)).collect();

    // `best[j]`: the smallest size of `tokens[..positions[j]]`, whose last block
    // starts at `positions[from[j]]`.
    let mut best = vec![u64::MAX; positions.len()];
    let mut from = vec![0; positions.len()];
    best[0] = 0;
    for j in 1..positions.len() {
        for i in 0..j {
            let frequencies = Frequencies::difference(&at[j].0, &at[i].0);
            let bits = best[i] + costs.estimate(&frequencies, at[j].1 - at[i].1);
            if bits < best[j] {
                best[j] = bits;
                from[j] = i;
            }
        }
    }

    let first = ends.len();
    let mut j = positions.len() - 1;
    while j > 0 {
        ends.push(positions[j]);
        j = from[j];
    }
    ends[first..].reverse();

    if search.refine {
        refine(&totals, costs, spacing, &mut ends[first..]);
    }
}

/// Moves each boundary to the position within `spacing` of it that makes the two
/// blocks around it smallest, searching coarse to fine.
fn refine(totals: &RunningTotals, costs: &mut BlockCosts, spacing: usize, ends: &mut [usize]) {
    let mut bits = |start: usize, end: usize| {
        let (frequencies, bytes) = totals.range(start, end);
        costs.estimate(&frequencies, bytes)
    };

    for k in 0..ends.len() - 1 {
        let start = if k == 0 { 0 } else { ends[k - 1] };
        let end = ends[k + 1];

        let mut best = ends[k];
        let mut best_bits = bits(start, best) + bits(best, end);
        let mut reach = spacing;
        loop {
            let step = (reach / 8).max(1);
            let low = best.saturating_sub(reach).max(start + 1);
            let high = (best + reach).min(end - 1);
            for pos in (low..=high).step_by(step) {
                let pos_bits = bits(start, pos) + bits(pos, end);
                if pos_bits < best_bits {
                    best = pos;
                    best_bits = pos_bits;
                }
            }
            if step == 1 {
                break;
            }
            reach = step;
        }
        ends[k] = best;
    }
}

/// Symbol counts and byte lengths of every prefix of the tokens, from totals kept
/// every `spacing` tokens.
struct RunningTotals<'a> {
    tokens: &'a [Token],
    spacing: usize,
    totals: Vec<(Frequencies, usize)>,
}

impl<'a> RunningTotals<'a> {
    fn new(tokens: &'a [Token], spacing: usize) -> Self {
        let mut totals = vec![(Frequencies::new(), 0)];
        for chunk in tokens.chunks(spacing) {
            let (mut frequencies, mut bytes) = totals.last().unwrap().clone();
            frequencies.add_tokens(chunk);
            bytes += byte_len(chunk);
            totals.push((frequencies, bytes));
        }
        Self {
            tokens,
            spacing,
            totals,
        }
    }

    /// The counts and byte length of `tokens[..pos]`.
    fn at(&self, pos: usize) -> (Frequencies, usize) {
        let saved = pos / self.spacing;
        let rest = &self.tokens[saved * self.spacing..pos];
        let (mut frequencies, bytes) = self.totals[saved].clone();
        frequencies.add_tokens(rest);
        (frequencies, bytes + byte_len(rest))
    }

    /// The counts and byte length of `tokens[start..end]`.
    fn range(&self, start: usize, end: usize) -> (Frequencies, usize) {
        let (later, later_bytes) = self.at(end);
        let (earlier, earlier_bytes) = self.at(start);
        (
            Frequencies::difference(&later, &earlier),
            later_bytes - earlier_bytes,
        )
    }
}

#[cfg(test)]
mod tests;
