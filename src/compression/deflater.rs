use crate::compression::block_splitter::{self, BlockCosts, BlockType};
use crate::compression::huffman_encoder::{
    self, Code, DynamicCodes, END_OF_BLOCK, FixedCodes, Frequencies,
};
use crate::compression::lz77::{MAX_MATCH, MatchFinder, Token};
use crate::compression::tables::CODE_LENGTH_ORDER;
use crate::io::bit_writer::BitWriter;
use crate::options::{CompressionOptions, Strategy};

pub(crate) const MAX_STORED_BLOCK: usize = 65_535;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkEnd {
    /// More input follows.
    More,
    /// More input may follow, but everything so far must be written out.
    Flush,
    /// The end of the stream.
    Last,
}

pub(crate) fn output_bound(data_len: usize) -> usize {
    data_len + 5 * data_len.div_ceil(MAX_STORED_BLOCK).max(1)
}

/// Compresses data into a raw DEFLATE stream (RFC 1951).
pub(crate) struct Deflater {
    match_finder: MatchFinder,
    tokens: Vec<Token>,
    block_ends: Vec<usize>,
    costs: Option<Box<BlockCosts>>,
}

impl Deflater {
    pub(crate) fn new() -> Self {
        Self {
            match_finder: MatchFinder::new(),
            tokens: Vec::new(),
            block_ends: Vec::new(),
            costs: None,
        }
    }

    pub(crate) fn deflate_into(
        &mut self,
        data: &[u8],
        options: CompressionOptions,
        writer: &mut BitWriter,
    ) {
        self.deflate_chunk(data, 0, ChunkEnd::Last, options, writer);
    }
    
    pub(crate) fn deflate_chunk(
        &mut self,
        data: &[u8],
        start: usize,
        chunk_end: ChunkEnd,
        options: CompressionOptions,
        writer: &mut BitWriter,
    ) -> usize {
        let last = chunk_end == ChunkEnd::Last;
        let end = match chunk_end {
            ChunkEnd::More => data.len().saturating_sub(MAX_MATCH).max(start),
            ChunkEnd::Flush | ChunkEnd::Last => data.len(),
        };
        if end == start && !last {
            return start;
        }
        
        if options.get_strategy() == Strategy::Stored || options.get_level() == 0 {
            Self::write_stored_blocks(writer, &data[start..end], last);
            return end;
        }

        let end = self.match_finder.find_tokens_in(
            data,
            start,
            end,
            options.get_level(),
            &mut self.tokens,
        );
        let costs = self
            .costs
            .get_or_insert_with(|| Box::new(BlockCosts::new()));

        match options.get_strategy() {
            Strategy::Fixed => Self::write_fixed_block(writer, &costs.fixed, &self.tokens, last),
            _ => {
                block_splitter::split(
                    &self.tokens,
                    options.get_level(),
                    costs,
                    &mut self.block_ends,
                );

                let (mut token_start, mut byte_start) = (0, start);
                for (i, &token_end) in self.block_ends.iter().enumerate() {
                    let tokens = &self.tokens[token_start..token_end];
                    let byte_end = byte_start + block_splitter::byte_len(tokens);
                    let last_block = last && i == self.block_ends.len() - 1;
                    Self::write_cheapest_block(
                        writer,
                        costs,
                        tokens,
                        &data[byte_start..byte_end],
                        last_block,
                    );
                    (token_start, byte_start) = (token_end, byte_end);
                }
            }
        }
        end
    }
    
    pub(crate) fn write_sync_flush(writer: &mut BitWriter) {
        Self::write_stored_block(writer, &[], false);
    }

    /// Writes `tokens`, which stand for `data`, as whichever block type is smallest.
    fn write_cheapest_block(
        writer: &mut BitWriter,
        costs: &mut BlockCosts,
        tokens: &[Token],
        data: &[u8],
        last: bool,
    ) {
        let frequencies = Frequencies::of(tokens);
        match costs.cheapest(&frequencies, data.len(), writer.bit_len() % 8).0 {
            BlockType::Stored => Self::write_stored_blocks(writer, data, last),
            BlockType::Fixed => Self::write_fixed_block(writer, &costs.fixed, tokens, last),
            BlockType::Dynamic => Self::write_dynamic_block(writer, &costs.dynamic, tokens, last),
        }
    }
    
    fn write_stored_blocks(writer: &mut BitWriter, data: &[u8], last: bool) {
        if data.is_empty() {
            Self::write_stored_block(writer, data, last);
            return;
        }

        let blocks = data.chunks(MAX_STORED_BLOCK);
        let final_block = (data.len() - 1) / MAX_STORED_BLOCK;
        for (i, block) in blocks.enumerate() {
            Self::write_stored_block(writer, block, last && i == final_block);
        }
    }

    fn write_fixed_block(writer: &mut BitWriter, codes: &FixedCodes, tokens: &[Token], last: bool) {
        writer.write_bits(last as u32 | 0b01 << 1, 3); // BFINAL, then BTYPE = 01
        Self::write_tokens(writer, &codes.literal_length, &codes.distance, tokens);
    }

    fn write_dynamic_block(
        writer: &mut BitWriter,
        codes: &DynamicCodes,
        tokens: &[Token],
        last: bool,
    ) {
        writer.write_bits(last as u32 | 0b10 << 1, 3); // BFINAL, then BTYPE = 10
        Self::write_dynamic_header(writer, codes);
        Self::write_tokens(writer, &codes.literal_length, &codes.distance, tokens);
    }

    fn write_dynamic_header(writer: &mut BitWriter, codes: &DynamicCodes) {
        let hlit = (codes.literal_length_count - 257) as u32;
        let hdist = (codes.distance_count - 1) as u32;
        let hclen = (codes.code_length_count - 4) as u32;
        writer.write_bits(hlit | hdist << 5 | hclen << 10, 14);

        for &symbol in &CODE_LENGTH_ORDER[..codes.code_length_count] {
            writer.write_bits(codes.code_length[symbol].len as u32, 3);
        }

        for s in &codes.code_length_symbols {
            let code = codes.code_length[s.symbol as usize];
            writer.write_bits(code.bits as u32, code.len as u32);
            writer.write_bits(s.extra as u32, s.extra_bits as u32);
        }
    }
    
    fn write_tokens(
        writer: &mut BitWriter,
        literal_length: &[Code],
        distance: &[Code],
        tokens: &[Token],
    ) {
        for token in tokens {
            match token {
                Token::Literal(byte) => {
                    let code = literal_length[*byte as usize];
                    writer.write_bits(code.bits as u32, code.len as u32);
                }
                Token::Match {
                    length: match_length,
                    distance: match_distance,
                } => {
                    let length = huffman_encoder::length_symbol(*match_length);
                    let code = literal_length[length.symbol as usize];
                    writer.write_bits(code.bits as u32, code.len as u32);
                    writer.write_bits(length.extra as u32, length.extra_bits as u32);

                    let dist = huffman_encoder::distance_symbol(*match_distance);
                    let code = distance[dist.symbol as usize];
                    writer.write_bits(code.bits as u32, code.len as u32);
                    writer.write_bits(dist.extra as u32, dist.extra_bits as u32);
                }
            }
        }

        let code = literal_length[END_OF_BLOCK];
        writer.write_bits(code.bits as u32, code.len as u32);
    }

    fn write_stored_block(writer: &mut BitWriter, block: &[u8], last: bool) {
        debug_assert!(block.len() <= MAX_STORED_BLOCK, "stored block too long");

        writer.write_bits(last as u32, 3); // BFINAL, then BTYPE = 00
        writer.align_to_byte();

        let len = block.len() as u32;
        writer.write_bits(len | (!len & 0xFFFF) << 16, 32); // LEN, then NLEN
        writer.write_bytes(block);
    }
}

#[cfg(test)]
mod tests;
