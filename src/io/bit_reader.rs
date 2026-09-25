use crate::Error;

#[derive(Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u64,
    bit_count: u32,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let mut reader = Self {
            data,
            byte_pos: 0,
            bit_buf: 0,
            bit_count: 0,
        };
        reader.refill();
        reader
    }

    #[inline]
    fn refill(&mut self) {
        if self.bit_count >= 32 {
            return;
        }

        self.refill_full();
    }

    #[inline]
    pub(crate) fn refill_full(&mut self) {
        if let Some(chunk) = self.data[self.byte_pos..].first_chunk::<8>() {
            // Fast path: one unaligned 8-byte load, keep as many whole bytes as fit.
            let word = u64::from_le_bytes(*chunk);
            let bytes = (63 - self.bit_count) / 8;
            let new_count = self.bit_count + bytes * 8;

            self.bit_buf |= (word << self.bit_count) & ((1u64 << new_count) - 1);
            self.byte_pos += bytes as usize;
            self.bit_count = new_count;
        } else {
            // Near the end of input: load byte by byte.
            while self.bit_count <= 56 && self.byte_pos < self.data.len() {
                self.bit_buf |= (self.data[self.byte_pos] as u64) << self.bit_count;
                self.byte_pos += 1;
                self.bit_count += 8;
            }
        }
    }

    #[inline]
    fn consume(&mut self, n: u32) {
        self.bit_buf >>= n;
        self.bit_count -= n;
        self.refill();
    }

    pub fn align_to_byte(&mut self) {
        // Whole bytes are loaded, so the unread part of the current byte is bit_count % 8.
        self.consume(self.bit_count % 8);
    }

    pub fn read_next_bit(&mut self) -> Result<u8, Error> {
        Ok(self.read_next_bits(1)? as u8)
    }

    pub fn read_next_bits(&mut self, n: u32) -> Result<u32, Error> {
        // Internal API: callers never ask for more; it's a bug if they do.
        assert!(n <= 32, "cannot read more than 32 bits");

        if n > self.bit_count {
            return Err(Error::UnexpectedEndOfInput);
        }

        let result = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.consume(n);

        Ok(result)
    }

    #[inline]
    #[cfg(test)] // only the tests use this
    pub fn peek_next_bits(&self, n: u32) -> u32 {
        // Internal API: callers never ask for more; it's a bug if they do.
        assert!(n <= 32, "cannot read more than 32 bits");

        (self.bit_buf & ((1u64 << n) - 1)) as u32
    }

    #[inline]
    pub(crate) fn peek_buffer(&self) -> u64 {
        self.bit_buf
    }

    #[inline]
    pub(crate) fn consume_buffered(&mut self, n: u32) -> Result<(), Error> {
        if n > self.bit_count {
            return Err(Error::UnexpectedEndOfInput);
        }

        self.bit_buf >>= n;
        self.bit_count -= n;

        Ok(())
    }

    #[inline]
    pub(crate) fn take_buffered(&mut self, n: u32) -> Result<u32, Error> {
        let value = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.consume_buffered(n)?;

        Ok(value)
    }

    #[inline]
    pub(crate) fn finish_buffered(&mut self) {
        self.refill();
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        // Internal API: only called right after align_to_byte.
        assert!(self.bit_count % 8 == 0, "reader is not byte-aligned");

        let start = self.byte_pos - (self.bit_count / 8) as usize;

        if n > self.data.len() - start {
            return Err(Error::UnexpectedEndOfInput);
        }

        let data = self.data;
        self.byte_pos = start + n;
        self.bit_buf = 0;
        self.bit_count = 0;
        self.refill();

        Ok(&data[start..start + n])
    }

    #[inline]
    pub fn skip_bits(&mut self, n: usize) -> Result<(), Error> {
        if n <= self.bit_count as usize {
            self.consume(n as u32);
            return Ok(());
        }

        let remaining_bits = self.bit_count as usize + (self.data.len() - self.byte_pos) * 8;
        if n > remaining_bits {
            return Err(Error::UnexpectedEndOfInput);
        }

        // Skipping past the buffer: drop it, jump whole bytes, then the leftover bits.
        let rest = n - self.bit_count as usize;
        self.bit_buf = 0;
        self.bit_count = 0;
        self.byte_pos += rest / 8;
        self.refill();
        self.consume((rest % 8) as u32);

        Ok(())
    }

    pub(crate) fn position(&self) -> usize {
        debug_assert!(self.bit_count % 8 == 0, "call align_to_byte first");
        self.byte_pos - (self.bit_count / 8) as usize
    }
    
    pub(crate) fn at_bit(data: &'a [u8], bit_position: usize) -> Self {
        debug_assert!(bit_position <= data.len() * 8);

        let mut reader = Self {
            data,
            byte_pos: bit_position / 8,
            bit_buf: 0,
            bit_count: 0,
        };
        reader.refill();

        reader.consume((bit_position % 8) as u32);
        reader
    }
    
    pub(crate) fn bit_position(&self) -> usize {
        self.byte_pos * 8 - self.bit_count as usize
    }

    #[inline]
    pub(crate) fn buffered_bits(&self) -> u32 {
        self.bit_count
    }
}

#[cfg(test)]
mod tests;
