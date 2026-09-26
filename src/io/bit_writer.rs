/// Writes a DEFLATE bit stream: the mirror of `BitReader`.
///
/// Bits are packed least significant bit first, so the first bit written becomes
/// bit 0 of the first byte. Multi-bit values are written least significant bit first
/// too.
pub(crate) struct BitWriter {
    out: Vec<u8>,
    bit_buf: u64,
    bit_count: u32,
}

impl BitWriter {
    pub(crate) fn new() -> Self {
        Self::with_capacity(0)
    }

    /// A writer whose output buffer has room for `bytes` bytes up front.
    pub(crate) fn with_capacity(bytes: usize) -> Self {
        Self {
            out: Vec::with_capacity(bytes),
            bit_buf: 0,
            bit_count: 0,
        }
    }

    /// Moves the low 32 bits of the buffer to `out` once there are that many.
    #[inline]
    fn flush_word(&mut self) {
        if self.bit_count < 32 {
            return;
        }

        self.out.extend_from_slice(&(self.bit_buf as u32).to_le_bytes());
        self.bit_buf >>= 32;
        self.bit_count -= 32;
    }

    /// Moves every whole byte in the buffer to `out`, leaving fewer than 8 bits.
    #[inline]
    fn flush_bytes(&mut self) {
        let bytes = (self.bit_count / 8) as usize;
        self.out.extend_from_slice(&self.bit_buf.to_le_bytes()[..bytes]);
        // `bytes` is at most 3, so the shift stays below 64.
        self.bit_buf >>= bytes * 8;
        self.bit_count -= bytes as u32 * 8;
    }

    /// Writes the low `n` bits of `value`, least significant first. `n` is at most
    /// 32, and `value` must fit in `n` bits.
    #[inline]
    pub(crate) fn write_bits(&mut self, value: u32, n: u32) {
        // Internal API: callers never break these; it's a bug if they do.
        debug_assert!(n <= 32, "cannot write more than 32 bits");
        debug_assert!((value as u64) >> n == 0, "value does not fit in {n} bits");

        // bit_count < 32 and n <= 32, so everything fits in the 64-bit buffer.
        self.bit_buf |= (value as u64) << self.bit_count;
        self.bit_count += n;
        self.flush_word();
    }

    /// Pads with zero bits up to the next byte boundary. Does nothing if already
    /// aligned.
    #[inline]
    pub(crate) fn align_to_byte(&mut self) {
        // Bits above bit_count are already zero, so padding is just a count bump.
        self.bit_count = (self.bit_count + 7) & !7;
        self.flush_word();
    }

    /// Appends whole bytes. The writer must be byte-aligned (call `align_to_byte`
    /// first), as it is before a stored block's data.
    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        debug_assert!(self.bit_count % 8 == 0, "call align_to_byte first");

        self.flush_bytes();
        self.out.extend_from_slice(bytes);
    }

    /// The number of bits written so far, including alignment padding.
    pub(crate) fn bit_len(&self) -> usize {
        self.out.len() * 8 + self.bit_count as usize
    }

    /// Pads the last partial byte with zero bits and returns everything written.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        self.align_to_byte();
        self.flush_bytes();
        self.out
    }
}

#[cfg(test)]
mod tests;
