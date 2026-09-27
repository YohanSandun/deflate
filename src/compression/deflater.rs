use crate::compression::huffman_encoder::{self, Code, END_OF_BLOCK, FixedCodes};
use crate::compression::lz77::{MatchFinder, Token};
use crate::io::bit_writer::BitWriter;
use crate::options::{CompressionOptions, Strategy};

pub(crate) const MAX_STORED_BLOCK: usize = 65_535;

pub(crate) fn output_bound(data_len: usize) -> usize {
    data_len + 5 * data_len.div_ceil(MAX_STORED_BLOCK).max(1)
}

/// Compresses data into a raw DEFLATE stream (RFC 1951).
pub(crate) struct Deflater {
    match_finder: MatchFinder,
    tokens: Vec<Token>,
    fixed_codes: Option<Box<FixedCodes>>,
}

impl Deflater {
    pub(crate) fn new() -> Self {
        Self {
            match_finder: MatchFinder::new(),
            tokens: Vec::new(),
            fixed_codes: None,
        }
    }

    pub(crate) fn deflate_into(
        &mut self,
        data: &[u8],
        options: CompressionOptions,
        writer: &mut BitWriter,
    ) {
        match options.get_strategy() {
            Strategy::Stored => Self::write_stored_blocks(writer, data),
            Strategy::Fixed => {
                self.match_finder
                    .find_tokens(data, options.get_level(), &mut self.tokens);
                let codes = self
                    .fixed_codes
                    .get_or_insert_with(|| Box::new(FixedCodes::new()));
                Self::write_fixed_block(writer, codes, &self.tokens, true);
            }
            Strategy::Dynamic => todo!("dynamic Huffman blocks"),
        }
    }

    fn write_stored_blocks(writer: &mut BitWriter, data: &[u8]) {
        if data.is_empty() {
            Self::write_stored_block(writer, data, true);
            return;
        }

        let blocks = data.chunks(MAX_STORED_BLOCK);
        for (i, block) in blocks.enumerate() {
            Self::write_stored_block(writer, block, i == (data.len() - 1) / MAX_STORED_BLOCK);
        }
    }

    fn write_fixed_block(writer: &mut BitWriter, codes: &FixedCodes, tokens: &[Token], last: bool) {
        writer.write_bits(last as u32 | 0b01 << 1, 3); // BFINAL, then BTYPE = 01
        Self::write_tokens(writer, &codes.literal_length, &codes.distance, tokens);
    }

    /// Writes `tokens` with the given codes, then the end-of-block symbol. Shared by
    /// fixed and dynamic blocks, which differ only in their codes.
    ///
    /// - a literal: its literal/length code
    /// - a match: the code for `huffman_encoder::length_symbol(length)` and its extra
    ///   bits, then the distance code for `huffman_encoder::distance_symbol(distance)`
    ///   and its extra bits
    /// - finally `literal_length[END_OF_BLOCK]`
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
