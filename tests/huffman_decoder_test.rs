use deflate::compression::huffman_decoder::HuffmanDecoder;
use deflate::io::bit_reader::BitReader;

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
}
