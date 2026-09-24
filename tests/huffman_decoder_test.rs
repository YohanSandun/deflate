use rust_deflate::compression::huffman_decoder::HuffmanDecoder;
use rust_deflate::io::bit_reader::BitReader;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_short_codes_when_fewer_than_table_bits_remain() {
        // Lengths 1, 2, 2 -> canonical codes A=0, B=10, C=11.
        // Stream bits (LSB first): 0 | 1 0 | 1 1 -> 0b00011010.
        let decoder = HuffmanDecoder::new(&[1, 2, 2]).unwrap();
        let data = [0b00011010];
        let mut reader = BitReader::new(&data);

        assert_eq!(decoder.decode(&mut reader).unwrap(), 0);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 1);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 2);
    }

    #[test]
    fn decodes_secondary_code_when_fewer_than_max_bits_remain() {
        // Lengths 1..=9, 10, 10 -> symbol 10 is ten 1-bits.
        // Stream: symbol 0 (bit 0), then ten 1s = 11 bits, so only 15 bits remain
        // when the 15-bit secondary peek happens.
        let decoder = HuffmanDecoder::new(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10]).unwrap();
        let data = [0b11111110, 0b00000111];
        let mut reader = BitReader::new(&data);

        assert_eq!(decoder.decode(&mut reader).unwrap(), 0);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 10);
    }

    #[test]
    fn decode_fails_when_code_runs_past_end_of_input() {
        let decoder = HuffmanDecoder::new(&[1, 1]).unwrap();
        let data = [0xFF];
        let mut reader = BitReader::new(&data);

        for _ in 0..8 {
            assert_eq!(decoder.decode(&mut reader).unwrap(), 1);
        }

        assert_eq!(decoder.decode(&mut reader), Err("unexpected end of input".to_string()));
    }

    #[test]
    fn new_rejects_over_subscribed_code() {
        assert_eq!(
            HuffmanDecoder::new(&[1, 1, 1]).err(),
            Some("over-subscribed Huffman code".to_string())
        );
    }

    #[test]
    fn new_rejects_code_length_above_max_bits() {
        assert_eq!(
            HuffmanDecoder::new(&[16]).err(),
            Some("invalid Huffman code length".to_string())
        );
    }

    #[test]
    fn new_accepts_incomplete_code() {
        // A single distance code of length 1 is legal in DEFLATE.
        assert!(HuffmanDecoder::new(&[1]).is_ok());
    }

    // Test-side encoder: canonical codes per RFC 1951, written MSB-first into an LSB-first stream.
    fn encode(code_lengths: &[u8], symbols: &[usize]) -> Vec<u8> {
        let mut bl_counts = [0u32; 16];
        for &len in code_lengths {
            if len > 0 {
                bl_counts[len as usize] += 1;
            }
        }

        let mut next_code = [0u32; 16];
        let mut code = 0;
        for bits in 1..16 {
            code = (code + bl_counts[bits - 1]) << 1;
            next_code[bits] = code;
        }

        let mut codes = vec![0u32; code_lengths.len()];
        for (symbol, &len) in code_lengths.iter().enumerate() {
            if len > 0 {
                codes[symbol] = next_code[len as usize];
                next_code[len as usize] += 1;
            }
        }

        let mut out = Vec::new();
        let mut bit_pos = 0;
        for &symbol in symbols {
            let len = code_lengths[symbol];
            for k in (0..len).rev() {
                if bit_pos % 8 == 0 {
                    out.push(0);
                }
                *out.last_mut().unwrap() |= (((codes[symbol] >> k) & 1) as u8) << (bit_pos % 8);
                bit_pos += 1;
            }
        }

        out
    }

    #[test]
    fn round_trips_every_symbol_across_many_secondary_tables() {
        // 3 short codes + 255 ten-bit + 2 eleven-bit codes (a complete code).
        // The long codes spread over 128 different primary prefixes.
        let mut lengths = vec![2u8, 2, 2];
        lengths.extend(std::iter::repeat_n(10u8, 255));
        lengths.extend([11u8, 11]);

        let decoder = HuffmanDecoder::new(&lengths).unwrap();

        let symbols: Vec<usize> = (0..lengths.len()).chain((0..lengths.len()).rev()).collect();
        let data = encode(&lengths, &symbols);
        let mut reader = BitReader::new(&data);

        for &expected in &symbols {
            assert_eq!(decoder.decode(&mut reader).unwrap(), expected);
        }
    }

    #[test]
    fn round_trips_fifteen_bit_codes() {
        let lengths = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 15];

        let decoder = HuffmanDecoder::new(&lengths).unwrap();

        let symbols: Vec<usize> = (0..lengths.len()).rev().collect();
        let data = encode(&lengths, &symbols);
        let mut reader = BitReader::new(&data);

        for &expected in &symbols {
            assert_eq!(decoder.decode(&mut reader).unwrap(), expected);
        }
    }

    #[test]
    fn decode_rejects_unused_code_in_incomplete_table() {
        // Only code "0" exists; a 1 bit must be rejected.
        let decoder = HuffmanDecoder::new(&[1]).unwrap();
        let data = [0b00000001];
        let mut reader = BitReader::new(&data);

        assert_eq!(decoder.decode(&mut reader), Err("invalid Huffman code".to_string()));
    }

    #[test]
    fn empty_decoder_rejects_every_code() {
        let decoder = HuffmanDecoder::empty();
        let data = [0x00, 0xFF];
        let mut reader = BitReader::new(&data);

        assert_eq!(decoder.decode(&mut reader), Err("invalid Huffman code".to_string()));
    }

    #[test]
    fn rebuild_replaces_previous_codes() {
        // Start with long codes so the secondary tables are populated, then rebuild
        // with a short code set; nothing from the first table may leak through.
        let mut lengths = vec![2u8, 2, 2];
        lengths.extend(std::iter::repeat_n(10u8, 255));
        lengths.extend([11u8, 11]);

        let mut decoder = HuffmanDecoder::new(&lengths).unwrap();
        decoder.rebuild(&[1, 2, 2]).unwrap();

        let data = [0b00011010];
        let mut reader = BitReader::new(&data);

        assert_eq!(decoder.decode(&mut reader).unwrap(), 0);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 1);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 2);

        // All-ones used to lead into a secondary table; now it's the 2-bit code for symbol 2.
        let data = [0xFF, 0xFF];
        let mut reader = BitReader::new(&data);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 2);
    }

    #[test]
    fn rebuild_after_short_codes_supports_long_codes() {
        let mut decoder = HuffmanDecoder::new(&[1, 2, 2]).unwrap();

        let lengths = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 15];
        decoder.rebuild(&lengths).unwrap();

        let symbols: Vec<usize> = (0..lengths.len()).rev().collect();
        let data = encode(&lengths, &symbols);
        let mut reader = BitReader::new(&data);

        for &expected in &symbols {
            assert_eq!(decoder.decode(&mut reader).unwrap(), expected);
        }
    }

    #[test]
    fn rebuild_failure_leaves_decoder_unchanged() {
        let mut decoder = HuffmanDecoder::new(&[1, 2, 2]).unwrap();

        assert_eq!(
            decoder.rebuild(&[1, 1, 1]),
            Err("over-subscribed Huffman code".to_string())
        );

        let data = [0b00011010];
        let mut reader = BitReader::new(&data);

        assert_eq!(decoder.decode(&mut reader).unwrap(), 0);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 1);
        assert_eq!(decoder.decode(&mut reader).unwrap(), 2);
    }
}
