use crate::Error;
use crate::io::bit_reader::BitReader;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_to_byte_from_byte_boundary() {
        let data = [0b10110001, 0b11001100];
        let mut reader = BitReader::new(&data);

        // Already at byte boundary.
        reader.align_to_byte();

        // Should still read from the first bit.
        assert_eq!(reader.read_next_bit().unwrap(), 1);
    }

    #[test]
    fn align_to_byte_from_middle_of_byte() {
        let data = [0b10110001, 0b11001100];
        let mut reader = BitReader::new(&data);

        // Read 3 bits from the first byte.
        // LSB first: 1, 0, 0
        assert_eq!(reader.read_next_bits(3).unwrap(), 0b001);

        reader.align_to_byte();

        // Remaining 5 bits of the first byte should be skipped.
        // We should now be at the beginning of the second byte.
        assert_eq!(reader.read_next_bits(8).unwrap(), 0b11001100);
    }

    #[test]
    fn align_to_byte_after_one_bit() {
        let data = [0b00000001, 0b00000010];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_next_bit().unwrap(), 1);

        reader.align_to_byte();

        // First byte's remaining 7 bits are skipped.
        assert_eq!(reader.read_next_bit().unwrap(), 0);
    }

    #[test]
    fn align_to_byte_after_seven_bits() {
        let data = [0b11111111, 0b00000001];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(7).unwrap();

        reader.align_to_byte();

        // Should be at byte 1.
        assert_eq!(reader.read_next_bit().unwrap(), 1);
    }

    #[test]
    fn read_next_bit_reads_lsb_first() {
        let data = [0b10110001];
        let mut reader = BitReader::new(&data);

        let bits: Vec<u8> = (0..8).map(|_| reader.read_next_bit().unwrap()).collect();

        assert_eq!(bits, vec![1, 0, 0, 0, 1, 1, 0, 1]);
    }

    #[test]
    fn read_next_bit_crosses_byte_boundary() {
        let data = [0b00000001, 0b00000010];
        let mut reader = BitReader::new(&data);

        // First byte, LSB first.
        assert_eq!(reader.read_next_bit().unwrap(), 1);

        for _ in 0..7 {
            assert_eq!(reader.read_next_bit().unwrap(), 0);
        }

        // Now we should be at the first bit of byte 1.
        assert_eq!(reader.read_next_bit().unwrap(), 0);
        assert_eq!(reader.read_next_bit().unwrap(), 1);
    }

    #[test]
    fn read_next_bit_returns_error_when_input_is_empty() {
        let data: [u8; 0] = [];
        let mut reader = BitReader::new(&data);

        let result = reader.read_next_bit();

        assert_eq!(result, Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn read_next_bit_returns_error_after_all_bits_are_consumed() {
        let data = [0b10110001];
        let mut reader = BitReader::new(&data);

        // Consume all 8 bits.
        for _ in 0..8 {
            assert!(reader.read_next_bit().is_ok());
        }

        assert_eq!(reader.read_next_bit(), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn read_next_bits_zero_bits() {
        let data = [0b10110001];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_next_bits(0).unwrap(), 0);

        // Reading zero bits should not advance the reader.
        assert_eq!(reader.read_next_bit().unwrap(), 1);
    }

    #[test]
    fn read_next_bits_one_bit() {
        let data = [0b00000001];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_next_bits(1).unwrap(), 1);
    }

    #[test]
    fn read_next_bits_reads_lsb_first() {
        let data = [0b10110001];
        let mut reader = BitReader::new(&data);

        // Bits: 1 0 0 0 1 1 0 1
        //
        // First 4 bits = 1000 as a bit sequence,
        // interpreted LSB-first => binary value 0001 = 1.
        assert_eq!(reader.read_next_bits(4).unwrap(), 1);

        // Remaining bits = 1 1 0 1
        // LSB-first => value 1011 = 11.
        assert_eq!(reader.read_next_bits(4).unwrap(), 11);
    }

    #[test]
    fn read_next_bits_reads_across_bytes() {
        let data = [0b00000001, 0b00000010];
        let mut reader = BitReader::new(&data);

        // Read 9 bits:
        //
        // First byte: 00000001 -> LSB bits:
        // 1 0 0 0 0 0 0 0
        //
        // Then first bit of second byte:
        // 0
        //
        // Result = 1
        assert_eq!(reader.read_next_bits(9).unwrap(), 1);
    }

    #[test]
    fn read_next_bits_reads_exactly_32_bits() {
        let data = [0xFF, 0xFF, 0xFF, 0xFF];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_next_bits(32).unwrap(), u32::MAX);
    }

    #[test]
    fn read_next_bits_can_read_multiple_chunks() {
        let data = [0b10110001, 0b11001100];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_next_bits(4).unwrap(), 1);
        assert_eq!(reader.read_next_bits(4).unwrap(), 11);
        assert_eq!(reader.read_next_bits(8).unwrap(), 0b11001100);
    }

    #[test]
    fn read_next_bits_returns_error_when_input_is_empty() {
        let data: [u8; 0] = [];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_next_bits(1), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn read_next_bits_returns_error_when_not_enough_bits() {
        let data = [0b00000001];
        let mut reader = BitReader::new(&data);

        // Only 8 bits available, asking for 9.
        assert_eq!(reader.read_next_bits(9), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn read_next_bits_returns_error_after_consuming_input() {
        let data = [0b00000001];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(8).unwrap();

        assert_eq!(reader.read_next_bits(1), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    #[should_panic(expected = "cannot read more than 32 bits")]
    fn read_next_bits_rejects_more_than_32_bits() {
        let data = [0xFF; 8];
        let mut reader = BitReader::new(&data);

        let _ = reader.read_next_bits(33);
    }

    #[test]
    #[should_panic(expected = "cannot read more than 32 bits")]
    fn read_next_bits_rejects_large_values() {
        let data = [0xFF; 8];
        let mut reader = BitReader::new(&data);

        let _ = reader.read_next_bits(100);
    }

    #[test]
    fn peek_next_bits_pads_with_zeros_past_end_of_input() {
        let data = [0b10110001];
        let reader = BitReader::new(&data);

        // Only 8 bits available; the missing high bits read as 0.
        assert_eq!(reader.peek_next_bits(12), 0b10110001);
    }

    #[test]
    fn peek_next_bits_on_empty_input_returns_zero() {
        let data: [u8; 0] = [];
        let reader = BitReader::new(&data);

        assert_eq!(reader.peek_next_bits(9), 0);
    }

    // Reference: bit `i` of the stream, LSB first.
    fn bit_at(data: &[u8], i: usize) -> u32 {
        ((data[i / 8] >> (i % 8)) & 1) as u32
    }

    fn bits_at(data: &[u8], start: usize, n: usize) -> u32 {
        (0..n).fold(0, |acc, k| acc | (bit_at(data, start + k) << k))
    }

    fn sample_data(len: usize) -> Vec<u8> {
        // Deterministic pseudo-random bytes so reads cross refill boundaries with varied values.
        let mut x = 0x2545F491u32;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    #[test]
    fn mixed_reads_across_refills_match_reference() {
        let data = sample_data(64);
        let mut reader = BitReader::new(&data);
        let widths = [1, 3, 7, 9, 13, 15, 16, 32, 5, 2, 31, 8];

        let mut pos = 0;
        for &n in widths.iter().cycle() {
            if pos + n > data.len() * 8 {
                break;
            }

            assert_eq!(
                reader.peek_next_bits(n as u32),
                bits_at(&data, pos, n),
                "peek at bit {pos}"
            );
            assert_eq!(
                reader.read_next_bits(n as u32).unwrap(),
                bits_at(&data, pos, n),
                "read at bit {pos}"
            );
            pos += n;
        }
    }

    #[test]
    fn skip_bits_past_buffer_lands_on_correct_bit() {
        let data = sample_data(64);
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(3).unwrap();
        reader.skip_bits(200).unwrap();

        assert_eq!(reader.read_next_bits(16).unwrap(), bits_at(&data, 203, 16));
    }

    #[test]
    fn skip_bits_to_exact_end_then_read_fails() {
        let data = sample_data(20);
        let mut reader = BitReader::new(&data);

        reader.skip_bits(20 * 8).unwrap();

        assert_eq!(reader.read_next_bit(), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn skip_bits_past_end_fails() {
        let data = sample_data(20);
        let mut reader = BitReader::new(&data);

        assert_eq!(
            reader.skip_bits(20 * 8 + 1),
            Err(Error::UnexpectedEndOfInput)
        );
    }

    #[test]
    fn align_to_byte_after_refill_lands_on_next_byte() {
        let data = sample_data(32);
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(32).unwrap();
        reader.read_next_bits(29).unwrap();
        reader.align_to_byte();

        assert_eq!(reader.read_next_bits(8).unwrap(), data[8] as u32);
    }

    #[test]
    fn peek_next_bits_does_not_consume() {
        let data = [0b10110001, 0b11001100];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.peek_next_bits(5), 0b10001);
        assert_eq!(reader.peek_next_bits(5), 0b10001);
        assert_eq!(reader.read_next_bits(5).unwrap(), 0b10001);
    }

    #[test]
    fn peek_next_bits_zero_bits_returns_zero() {
        let data = [0xFF];
        let reader = BitReader::new(&data);

        assert_eq!(reader.peek_next_bits(0), 0);
    }

    #[test]
    #[should_panic(expected = "cannot read more than 32 bits")]
    fn peek_next_bits_rejects_more_than_32_bits() {
        let data = [0xFF; 8];
        let reader = BitReader::new(&data);

        let _ = reader.peek_next_bits(33);
    }

    #[test]
    fn peek_next_bits_after_partial_read_pads_with_zeros() {
        let data = [0xFF, 0xFF];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(12).unwrap();

        // 4 real 1-bits left, the rest reads as 0.
        assert_eq!(reader.peek_next_bits(9), 0b000001111);
    }

    #[test]
    fn read_next_bits_32_bits_at_unaligned_offset() {
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(7).unwrap();

        assert_eq!(reader.read_next_bits(32).unwrap(), u32::MAX);
        assert_eq!(reader.read_next_bit().unwrap(), 1);
    }

    #[test]
    fn read_next_bits_failure_does_not_consume() {
        let data = [0b00000101];
        let mut reader = BitReader::new(&data);

        assert!(reader.read_next_bits(9).is_err());

        // Nothing was consumed, so all 8 bits can still be read.
        assert_eq!(reader.read_next_bits(8).unwrap(), 0b00000101);
    }

    #[test]
    fn read_next_bits_reads_whole_long_input_then_fails() {
        let data = sample_data(100);
        let mut reader = BitReader::new(&data);

        for &byte in &data {
            assert_eq!(reader.read_next_bits(8).unwrap(), byte as u32);
        }

        assert_eq!(reader.read_next_bit(), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn reads_match_reference_at_every_offset_and_width() {
        // Lengths around the 8-byte fast-path refill boundary.
        for len in [1, 7, 8, 9, 15, 16, 17, 40] {
            let data = sample_data(len);
            let total_bits = len * 8;

            for start in 0..total_bits {
                for n in 0..=32usize {
                    let mut reader = BitReader::new(&data);
                    reader.skip_bits(start).unwrap();

                    if start + n <= total_bits {
                        let expected = bits_at(&data, start, n);
                        assert_eq!(
                            reader.peek_next_bits(n as u32),
                            expected,
                            "peek len={len} start={start} n={n}"
                        );
                        assert_eq!(
                            reader.read_next_bits(n as u32).unwrap(),
                            expected,
                            "read len={len} start={start} n={n}"
                        );
                    } else {
                        let available = total_bits - start;
                        let expected = bits_at(&data, start, available);
                        assert_eq!(
                            reader.peek_next_bits(n as u32),
                            expected,
                            "padded peek len={len} start={start} n={n}"
                        );
                        assert!(
                            reader.read_next_bits(n as u32).is_err(),
                            "read past end len={len} start={start} n={n}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn skip_bits_zero_does_not_advance() {
        let data = [0b00000001];
        let mut reader = BitReader::new(&data);

        reader.skip_bits(0).unwrap();

        assert_eq!(reader.read_next_bit().unwrap(), 1);
    }

    #[test]
    fn skip_bits_within_buffer() {
        let data = [0b10110001, 0b11001100];
        let mut reader = BitReader::new(&data);

        reader.skip_bits(4).unwrap();

        assert_eq!(reader.read_next_bits(8).unwrap(), 0b11001011);
    }

    #[test]
    fn skip_bits_failure_does_not_advance() {
        let data = sample_data(20);
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(5).unwrap();
        assert!(reader.skip_bits(20 * 8).is_err());

        assert_eq!(reader.read_next_bits(16).unwrap(), bits_at(&data, 5, 16));
    }

    #[test]
    fn skip_bits_large_skip_on_long_input() {
        let data = sample_data(1000);
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(1).unwrap();
        reader.skip_bits(7000 + 3).unwrap();

        assert_eq!(reader.read_next_bits(32).unwrap(), bits_at(&data, 7004, 32));
    }

    #[test]
    fn empty_input_edge_cases() {
        let data: [u8; 0] = [];
        let mut reader = BitReader::new(&data);

        reader.align_to_byte();
        assert_eq!(reader.read_next_bits(0).unwrap(), 0);
        assert_eq!(reader.skip_bits(0), Ok(()));
        assert_eq!(reader.skip_bits(1), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn align_to_byte_twice_is_idempotent() {
        let data = [0xFF, 0b00000010, 0b00000011];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(3).unwrap();
        reader.align_to_byte();
        reader.align_to_byte();

        assert_eq!(reader.read_next_bits(8).unwrap(), 0b00000010);
    }

    #[test]
    fn align_to_byte_at_end_of_input() {
        let data = [0xFF];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(5).unwrap();
        reader.align_to_byte();

        assert_eq!(reader.read_next_bit(), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn read_bytes_at_start() {
        let data = [1, 2, 3, 4];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_bytes(3).unwrap(), &[1, 2, 3]);
        assert_eq!(reader.read_next_bits(8).unwrap(), 4);
    }

    #[test]
    fn read_bytes_after_bit_reads_and_align() {
        let data = sample_data(40);
        let mut reader = BitReader::new(&data);

        // Bytes 0..3 are now sitting in the bit buffer; read_bytes must return them, not skip them.
        reader.read_next_bits(13).unwrap();
        reader.align_to_byte();

        assert_eq!(reader.read_bytes(20).unwrap(), &data[2..22]);
        assert_eq!(
            reader.read_next_bits(16).unwrap(),
            bits_at(&data, 22 * 8, 16)
        );
    }

    #[test]
    fn read_bytes_zero_bytes() {
        let data = [0xAB];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_bytes(0).unwrap(), &[] as &[u8]);
        assert_eq!(reader.read_next_bits(8).unwrap(), 0xAB);
    }

    #[test]
    fn read_bytes_to_exact_end() {
        let data = sample_data(20);
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(8).unwrap();

        assert_eq!(reader.read_bytes(19).unwrap(), &data[1..]);
        assert_eq!(reader.read_next_bit(), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn read_bytes_past_end_fails_without_consuming() {
        let data = [1, 2, 3];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_bytes(4), Err(Error::UnexpectedEndOfInput));
        assert_eq!(reader.read_bytes(3).unwrap(), &[1, 2, 3]);
    }

    #[test]
    #[should_panic(expected = "reader is not byte-aligned")]
    fn read_bytes_rejects_unaligned_reader() {
        let data = [0xFF, 0xFF];
        let mut reader = BitReader::new(&data);

        reader.read_next_bits(3).unwrap();

        let _ = reader.read_bytes(1);
    }
}
