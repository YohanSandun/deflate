pub struct BitReader<'a> {
    bit_pos: u8,
    byte_pos: usize,
    data: &'a [u8],
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            bit_pos: 0,
            byte_pos: 0,
            data,
        }
    }

    pub fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.byte_pos += 1;
            self.bit_pos = 0;
        }
    }

    pub fn read_next_bit(&mut self) -> Result<u8, String> {
        if self.byte_pos >= self.data.len() {
            return Err("unexpected end of input".to_string());
        }

        let bit: u8 = (self.data[self.byte_pos] >> self.bit_pos) & 1;
        self.bit_pos += 1;

        if self.bit_pos == 8 {
            self.byte_pos += 1;
            self.bit_pos = 0;
        }

        Ok(bit)
    }

    pub fn read_next_bits(&mut self, n: u32) -> Result<u32, String> {
        if n > 32 {
            return Err("cannot read more than 32 bits".to_string());
        }

        let mut result = 0u32;
        let mut bits_read = 0u32;

        while bits_read < n {
            if self.byte_pos >= self.data.len() {
                return Err("unexpected end of input".to_string());
            }

            let available = 8 - self.bit_pos as u32;
            let needed = n - bits_read;
            let take = available.min(needed);

            let mask = if take == 8 { 0xFF } else { (1u8 << take) - 1 };

            let bits = (self.data[self.byte_pos] >> self.bit_pos) & mask;

            result |= (bits as u32) << bits_read;

            self.bit_pos += take as u8;
            bits_read += take;

            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }

        Ok(result)
    }

    pub fn peek_next_bits(&self, n: u32) -> Result<u32, String> {
        if n > 32 {
            return Err("cannot read more than 32 bits".to_string());
        }

        let mut result = 0u32;
        let mut bits_read = 0u32;

        let mut byte_pos = self.byte_pos;
        let mut bit_pos = self.bit_pos;

        while bits_read < n {
            if byte_pos >= self.data.len() {
                break;
            }

            let available = 8 - bit_pos as u32;
            let needed = n - bits_read;
            let take = available.min(needed);

            let mask = if take == 8 { 0xFF } else { (1u8 << take) - 1 };

            let bits = (self.data[byte_pos] >> bit_pos) & mask;

            result |= (bits as u32) << bits_read;

            bit_pos += take as u8;
            bits_read += take;

            if bit_pos == 8 {
                bit_pos = 0;
                byte_pos += 1;
            }
        }

        Ok(result)
    }

    pub fn skip_bits(&mut self, n: usize) -> Result<(), String> {
        let total = self.bit_pos as usize + n;
        self.byte_pos += total / 8;
        self.bit_pos = (total % 8) as u8;

        if (self.byte_pos > self.data.len() || (self.byte_pos == self.data.len() && self.bit_pos > 0)) {
            return Err("unexpected end of input".to_string());
        }

        Ok(())
    }
}
