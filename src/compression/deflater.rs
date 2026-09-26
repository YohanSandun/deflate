use crate::io::bit_writer::BitWriter;

pub(crate) const MAX_STORED_BLOCK: usize = 65_535;

pub(crate) fn output_bound(data_len: usize) -> usize {
    data_len + 5 * data_len.div_ceil(MAX_STORED_BLOCK).max(1)
}

/// Compresses data into a raw DEFLATE stream (RFC 1951).
pub(crate) struct Deflater {}

impl Deflater {
    pub(crate) fn new() -> Self {
       Self {}
    }

    /// Writes a complete raw DEFLATE stream for `data` to `writer`, ending with a
    /// block that has BFINAL set.
    pub(crate) fn deflate_into(&mut self, data: &[u8], writer: &mut BitWriter) {
        if data.is_empty() {
            Self::write_stored_block(writer, data, true);
            return;
        }

        let blocks = data.chunks(MAX_STORED_BLOCK);
        for (i, block) in blocks.enumerate() {
            Self::write_stored_block(writer, block, i == (data.len() - 1) / MAX_STORED_BLOCK);
        }
    }

    /// Writes one stored block (RFC 1951 section 3.2.4):
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
