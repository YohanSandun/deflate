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

        if self.byte_pos + 8 <= self.data.len() {
            // Fast path: one unaligned 8-byte load, keep as many whole bytes as fit.
            let word = u64::from_le_bytes(self.data[self.byte_pos..self.byte_pos + 8].try_into().unwrap());
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

    pub fn read_next_bit(&mut self) -> Result<u8, String> {
        Ok(self.read_next_bits(1)? as u8)
    }

    pub fn read_next_bits(&mut self, n: u32) -> Result<u32, String> {
        if n > 32 {
            return Err("cannot read more than 32 bits".to_string());
        }

        if n > self.bit_count {
            return Err("unexpected end of input".to_string());
        }

        let result = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.consume(n);

        Ok(result)
    }

    #[inline]
    pub fn peek_next_bits(&self, n: u32) -> Result<u32, String> {
        if n > 32 {
            return Err("cannot read more than 32 bits".to_string());
        }

        Ok((self.bit_buf & ((1u64 << n) - 1)) as u32)
    }

    #[inline]
    pub fn skip_bits(&mut self, n: usize) -> Result<(), String> {
        if n <= self.bit_count as usize {
            self.consume(n as u32);
            return Ok(());
        }

        let remaining_bits = self.bit_count as usize + (self.data.len() - self.byte_pos) * 8;
        if n > remaining_bits {
            return Err("unexpected end of input".to_string());
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
}
