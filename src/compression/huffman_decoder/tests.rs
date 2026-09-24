use super::{Alphabet, Entry, HuffmanDecoder};
use crate::Error;
use crate::io::bit_reader::BitReader;

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

    assert_eq!(
        decoder.decode(&mut reader),
        Err(Error::UnexpectedEndOfInput)
    );
}

#[test]
fn new_rejects_over_subscribed_code() {
    assert_eq!(
        HuffmanDecoder::new(&[1, 1, 1]).err(),
        Some(Error::OverSubscribedCode)
    );
}

#[test]
#[should_panic(expected = "invalid Huffman code length")]
fn new_rejects_code_length_above_max_bits() {
    // Can't come from input (lengths are at most 15), so it's an internal assert.
    let _ = HuffmanDecoder::new(&[16]);
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

    assert_eq!(decoder.decode(&mut reader), Err(Error::InvalidCode));
}

#[test]
fn empty_decoder_rejects_every_code() {
    let decoder = HuffmanDecoder::empty();
    let data = [0x00, 0xFF];
    let mut reader = BitReader::new(&data);

    assert_eq!(decoder.decode(&mut reader), Err(Error::InvalidCode));
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

    assert_eq!(decoder.rebuild(&[1, 1, 1]), Err(Error::OverSubscribedCode));

    let data = [0b00011010];
    let mut reader = BitReader::new(&data);

    assert_eq!(decoder.decode(&mut reader).unwrap(), 0);
    assert_eq!(decoder.decode(&mut reader).unwrap(), 1);
    assert_eq!(decoder.decode(&mut reader).unwrap(), 2);
}

// Literal/length lengths giving 'a' = 00, 'b' = 01, 'c' = 10, end of block = 11.
fn two_bit_literal_lengths() -> [u8; 257] {
    let mut lengths = [0u8; 257];
    lengths[b'a' as usize] = 2;
    lengths[b'b' as usize] = 2;
    lengths[b'c' as usize] = 2;
    lengths[256] = 2;
    lengths
}

#[test]
fn two_short_literals_share_one_entry() {
    let decoder =
        HuffmanDecoder::new_for(&two_bit_literal_lengths(), Alphabet::LiteralLength).unwrap();

    // Stream bits (LSB first): 'a' = 0 0, then 'b' = 0 1.
    let entry = decoder.lookup(0b1000);

    assert_eq!(entry.kind(), Entry::LITERAL);
    assert_eq!(entry.extra_bits(), 2, "two literals");
    assert_eq!(entry.code_length(), 4, "both codes consumed");
    assert_eq!(entry.value(), u32::from(b'a') | u32::from(b'b') << 8);
}

#[test]
fn literal_followed_by_non_literal_stays_single() {
    let decoder =
        HuffmanDecoder::new_for(&two_bit_literal_lengths(), Alphabet::LiteralLength).unwrap();

    // 'a' = 0 0, then end of block = 1 1: only the literal goes in the entry.
    let entry = decoder.lookup(0b1100);

    assert_eq!(entry.kind(), Entry::LITERAL);
    assert_eq!(entry.extra_bits(), 1);
    assert_eq!(entry.code_length(), 2);
    assert_eq!(entry.value(), u32::from(b'a'));
}

#[test]
fn literals_too_long_to_pair_stay_single() {
    // 32 literals with 5-bit codes: two of them need 10 bits, more than the
    // 9-bit primary table can see.
    let mut lengths = [0u8; 257];
    lengths[..32].fill(5);
    lengths[256] = 0;

    let decoder = HuffmanDecoder::new_for(&lengths, Alphabet::LiteralLength).unwrap();

    for index in 0..512u64 {
        let entry = decoder.lookup(index);
        assert_eq!(entry.extra_bits(), 1, "index {index}");
        assert_eq!(entry.code_length(), 5, "index {index}");
    }
}

#[test]
fn every_pair_entry_matches_two_single_lookups() {
    // A realistic literal/length table, checked
    // entry by entry: a pair must decode exactly like two separate lookups.
    let mut lengths = [0u8; 286];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            32 | 101 | 116 => 4, // ' ', 'e', 't'
            97..=122 => 6,
            32..=126 => 8,
            256 => 7,
            257..=264 => 7,
            _ => 0,
        };
    }
    // Make the code valid (not over-subscribed) by dropping symbols until it fits.
    while HuffmanDecoder::new_for(&lengths, Alphabet::LiteralLength).is_err() {
        let last = lengths.iter().rposition(|&l| l == 8).unwrap();
        lengths[last] = 0;
    }

    let pairs = HuffmanDecoder::new_for(&lengths, Alphabet::LiteralLength).unwrap();
    let singles = HuffmanDecoder::new(&lengths).unwrap();

    let mut pair_count = 0;
    for index in 0..512u64 {
        let entry = pairs.lookup(index);
        if entry.kind() != Entry::LITERAL || entry.extra_bits() != 2 {
            continue;
        }
        pair_count += 1;

        let first = singles.lookup(index);
        let second = singles.lookup(index >> first.code_length());

        assert_eq!(entry.value() & 0xFF, first.value(), "index {index}");
        assert_eq!(entry.value() >> 8, second.value(), "index {index}");
        assert_eq!(
            entry.code_length(),
            first.code_length() + second.code_length(),
            "index {index}"
        );
        assert!(entry.code_length() <= 9, "index {index}");
    }
    assert!(
        pair_count > 0,
        "expected some pairs with 4-bit codes present"
    );
}

#[test]
fn only_literal_length_tables_get_pairs() {
    // Raw-symbol and distance tables never combine entries.
    let symbols = HuffmanDecoder::new(&[2, 2, 2, 2]).unwrap();
    let distances = HuffmanDecoder::new_for(&[2, 2, 2, 2], Alphabet::Distance).unwrap();

    for index in 0..512u64 {
        assert_eq!(symbols.lookup(index).code_length(), 2);
        assert_eq!(distances.lookup(index).code_length(), 2);
    }
}
