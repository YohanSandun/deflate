use super::{Deflater, MAX_STORED_BLOCK};
use crate::compression::block_splitter::{BlockCosts, BlockType, stored_bits};
use crate::compression::huffman_encoder::{DynamicCodes, Frequencies};
use crate::compression::lz77::{MatchFinder, Token};
use crate::io::bit_writer::BitWriter;
use crate::options::{CompressionLevel, CompressionOptions, Strategy};

const STORED: CompressionOptions = CompressionOptions::new().strategy(Strategy::Stored);
const FIXED: CompressionOptions = CompressionOptions::new().strategy(Strategy::Fixed);
const DYNAMIC: CompressionOptions = CompressionOptions::new().strategy(Strategy::Dynamic);

fn deflate(data: &[u8]) -> Vec<u8> {
    deflate_with(data, STORED)
}

fn deflate_with(data: &[u8], options: CompressionOptions) -> Vec<u8> {
    let mut writer = BitWriter::new();
    Deflater::new().deflate_into(data, options, &mut writer);
    writer.finish()
}

fn stored_block(block: &[u8], last: bool) -> Vec<u8> {
    let mut writer = BitWriter::new();
    Deflater::write_stored_block(&mut writer, block, last);
    writer.finish()
}

// --- write_stored_block ---------------------------------------------------------

#[test]
fn stored_block_layout() {
    // BFINAL=1, BTYPE=00 -> header bits 1,0,0 -> 0x01 after padding; LEN=3 (03 00);
    // NLEN=!3 (FC FF); then the bytes.
    assert_eq!(
        stored_block(b"abc", true),
        vec![0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c']
    );
}

#[test]
fn non_final_stored_block_clears_bfinal() {
    assert_eq!(
        stored_block(b"abc", false),
        vec![0x00, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c']
    );
}

#[test]
fn empty_stored_block() {
    assert_eq!(stored_block(&[], true), vec![0x01, 0x00, 0x00, 0xFF, 0xFF]);
    assert_eq!(stored_block(&[], false), vec![0x00, 0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn largest_stored_block() {
    let block = vec![0xAB; MAX_STORED_BLOCK];
    let out = stored_block(&block, true);

    assert_eq!(&out[..5], &[0x01, 0xFF, 0xFF, 0x00, 0x00]);
    assert_eq!(out.len(), 5 + MAX_STORED_BLOCK);
    assert!(out[5..].iter().all(|&b| b == 0xAB));
}

#[test]
fn stored_block_after_unaligned_bits_is_padded() {
    // A block that starts mid-byte (as after a previous Huffman block): the 3 header
    // bits follow the existing bits, then padding up to the byte boundary.
    let mut writer = BitWriter::new();
    writer.write_bits(0b11111, 5);
    Deflater::write_stored_block(&mut writer, b"x", true);

    // Bits 0-4 are the five 1s, bit 5 is BFINAL = 1, bits 6-7 are BTYPE = 00, so the
    // first byte is 0x3F and no padding is needed. Then LEN = 1 and NLEN.
    assert_eq!(writer.finish(), vec![0x3F, 0x01, 0x00, 0xFE, 0xFF, b'x']);
}

#[test]
fn stored_block_header_crossing_a_byte_is_padded_after_the_header() {
    // 7 bits already written: the 3 header bits spill into the next byte, and the
    // padding comes after them.
    let mut writer = BitWriter::new();
    writer.write_bits(0, 7);
    Deflater::write_stored_block(&mut writer, b"y", false);

    // Byte 0: seven 0s then BFINAL=0 -> 0x00. Byte 1: BTYPE bits 0,0 then padding.
    assert_eq!(
        writer.finish(),
        vec![0x00, 0x00, 0x01, 0x00, 0xFE, 0xFF, b'y']
    );
}

// --- deflate_into ---------------------------------------------------------------

#[test]
fn empty_input_is_one_empty_final_block() {
    assert_eq!(deflate(&[]), vec![0x01, 0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn small_input_is_one_final_block() {
    assert_eq!(
        deflate(b"hello"),
        vec![0x01, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o']
    );
}

#[test]
fn input_of_exactly_one_block() {
    let data = vec![7u8; MAX_STORED_BLOCK];
    let out = deflate(&data);

    assert_eq!(out.len(), MAX_STORED_BLOCK + 5);
    assert_eq!(
        &out[..5],
        &[0x01, 0xFF, 0xFF, 0x00, 0x00],
        "one final block"
    );
}

#[test]
fn input_one_byte_over_a_block_splits_into_two() {
    let data: Vec<u8> = (0..MAX_STORED_BLOCK + 1).map(|i| i as u8).collect();
    let out = deflate(&data);

    // First block: not final, full size.
    assert_eq!(&out[..5], &[0x00, 0xFF, 0xFF, 0x00, 0x00]);
    assert!(out[5..5 + MAX_STORED_BLOCK] == data[..MAX_STORED_BLOCK]);

    // Second block: final, 1 byte.
    let second = 5 + MAX_STORED_BLOCK;
    assert_eq!(&out[second..second + 5], &[0x01, 0x01, 0x00, 0xFE, 0xFF]);
    assert_eq!(out[second + 5], data[MAX_STORED_BLOCK]);
    assert_eq!(out.len(), second + 6);
}

#[test]
fn many_blocks_only_the_last_is_final() {
    let len = 3 * MAX_STORED_BLOCK + 100;
    let data = vec![1u8; len];
    let out = deflate(&data);

    let mut offset = 0;
    let mut blocks = 0;
    let mut total = 0;
    while offset < out.len() {
        let bfinal = out[offset] & 1;
        let block_len = u16::from_le_bytes([out[offset + 1], out[offset + 2]]) as usize;
        let n_len = u16::from_le_bytes([out[offset + 3], out[offset + 4]]);
        assert_eq!(n_len, !(block_len as u16), "NLEN of block {blocks}");

        blocks += 1;
        total += block_len;
        offset += 5 + block_len;
        assert_eq!(
            bfinal == 1,
            offset == out.len(),
            "BFINAL only on the last block"
        );
    }

    assert_eq!(blocks, 4);
    assert_eq!(total, len);
}

#[test]
fn deflater_can_be_reused() {
    let mut deflater = Deflater::new();

    for data in [&b"first"[..], &[], &b"second stream"[..]] {
        let mut writer = BitWriter::new();
        deflater.deflate_into(data, STORED, &mut writer);
        assert_eq!(writer.finish(), deflate(data));
    }
}

#[test]
fn output_decodes_with_the_inflater() {
    for len in [
        0,
        1,
        1000,
        MAX_STORED_BLOCK - 1,
        MAX_STORED_BLOCK,
        MAX_STORED_BLOCK + 1,
        200_000,
    ] {
        let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        let decoded = crate::decompress(&deflate(&data)).unwrap();
        assert!(decoded == data, "length {len}");
    }
}

// --- fixed Huffman blocks -------------------------------------------------------

#[test]
fn fixed_empty_input_is_just_the_header_and_end_of_block() {
    // BFINAL = 1, BTYPE = 01, then end-of-block (seven 0 bits): what zlib writes.
    assert_eq!(deflate_with(&[], FIXED.level(CompressionLevel::FAST)), vec![0x03, 0x00]);
}

#[test]
fn fixed_literals_only() {
    // One byte has no matches. "a" is literal 97: fixed code 0x30 + 97 = 0x91 as 8
    // bits, sent most significant bit first.
    let options = FIXED.level(CompressionLevel::FAST);
    assert_eq!(deflate_with(b"a", options), vec![0x4B, 0x04, 0x00]);
}

#[test]
fn fixed_output_decodes_with_the_inflater() {
    let text = b"hello hello hello hello, the quick brown fox jumps over the lazy dog";
    let inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"a".to_vec(),
        text.to_vec(),
        vec![0; 100_000],
        (0..=255u8).cycle().take(70_000).collect(),
        (0..200_000u32).map(|i| (i * 31 % 251) as u8).collect(),
    ];

    for level in 0..=9 {
        for data in &inputs {
            let decoded = crate::decompress(&deflate_with(data, FIXED.level(CompressionLevel::new(level)))).unwrap();
            assert!(decoded == *data, "level {level}, length {}", data.len());
        }
    }
}

#[test]
fn fixed_shrinks_repetitive_input() {
    let data = vec![b'x'; 10_000];
    assert!(deflate_with(&data, FIXED).len() < 100);
}

#[test]
fn fixed_deflater_can_be_reused() {
    let mut deflater = Deflater::new();

    for data in [&b"first first first"[..], &[], &b"second stream"[..]] {
        let mut writer = BitWriter::new();
        deflater.deflate_into(data, FIXED, &mut writer);
        assert_eq!(writer.finish(), deflate_with(data, FIXED));
    }
}

// --- dynamic Huffman blocks -----------------------------------------------------

#[test]
fn dynamic_block_header() {
    // Only "a" and end-of-block are used, so 257 literal/length codes (HLIT = 0).
    // With no matches, distance codes 0 and 1 still get one bit each (HDIST = 1).
    let tokens = [Token::Literal(b'a')];
    let mut codes = DynamicCodes::new();
    codes.build(&tokens);
    let mut writer = BitWriter::new();
    Deflater::write_dynamic_block(&mut writer, &codes, &tokens, true);
    let out = writer.finish();

    assert_eq!(out[0] & 0b111, 0b101, "BFINAL = 1, BTYPE = 10");
    assert_eq!(out[0] >> 3, 0, "HLIT");
    assert_eq!(out[1] & 0x1F, 1, "HDIST");
}

#[test]
fn dynamic_output_decodes_with_the_inflater() {
    let text = b"hello hello hello hello, the quick brown fox jumps over the lazy dog";
    let inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"a".to_vec(),
        b"aaaa".to_vec(),
        text.to_vec(),
        vec![0; 100_000],
        (0..=255u8).cycle().take(70_000).collect(),
        (0..200_000u32).map(|i| (i * 31 % 251) as u8).collect(),
        skewed(50_000),
    ];

    for level in 0..=9 {
        for data in &inputs {
            let decoded = crate::decompress(&deflate_with(data, DYNAMIC.level(CompressionLevel::new(level)))).unwrap();
            assert!(decoded == *data, "level {level}, length {}", data.len());
        }
    }
}

// Pseudo-random bytes from a 4-letter alphabet: about 2 bits of information each.
fn skewed(len: usize) -> Vec<u8> {
    let mut state = 1u32;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            b"acgt"[(state >> 16) as usize % 4]
        })
        .collect()
}

#[test]
fn dynamic_beats_fixed_on_a_small_alphabet() {
    // Fixed codes spend 8 bits on each literal, dynamic about 2. Matches shrink
    // both, so the gap is smaller than 4 to 1.
    let data = skewed(10_000);
    let fixed = deflate_with(&data, FIXED.level(CompressionLevel::FAST)).len();
    let dynamic = deflate_with(&data, DYNAMIC.level(CompressionLevel::FAST)).len();
    assert!(dynamic * 3 < fixed * 2, "dynamic {dynamic} bytes, fixed {fixed}");
}

#[test]
fn dynamic_shrinks_repetitive_input() {
    let data = vec![b'x'; 10_000];
    assert!(deflate_with(&data, DYNAMIC).len() < 100);
}

#[test]
fn dynamic_deflater_can_be_reused() {
    let mut deflater = Deflater::new();

    for data in [&b"first first first"[..], &[], &b"second stream"[..]] {
        let mut writer = BitWriter::new();
        deflater.deflate_into(data, DYNAMIC, &mut writer);
        assert_eq!(writer.finish(), deflate_with(data, DYNAMIC));
    }
}

// --- mixed block types ----------------------------------------------------------

// Deterministic pseudo-random bytes: incompressible.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        })
        .collect()
}

// Text, then noise, then zeros, then text again.
fn mixed_input() -> Vec<u8> {
    let text: Vec<u8> = b"the quick brown fox jumps over the lazy dog, again and again. "
        .iter()
        .cycle()
        .take(30_000)
        .copied()
        .collect();
    let mut data = text.clone();
    data.extend(noise(30_000, 9));
    data.extend(vec![0; 20_000]);
    data.extend(skewed(30_000));
    data
}

fn tokens_for(data: &[u8], level: u8) -> Vec<Token> {
    let mut tokens = Vec::new();
    MatchFinder::new().find_tokens(data, level, &mut tokens);
    tokens
}

// The bits `write` adds after `offset` bits are already written.
fn bits_written(offset: u32, write: impl FnOnce(&mut BitWriter)) -> u64 {
    let mut writer = BitWriter::with_capacity(0);
    writer.write_bits(0, offset);
    let before = writer.bit_len();
    write(&mut writer);
    (writer.bit_len() - before) as u64
}

#[test]
fn stored_estimate_is_exact() {
    for len in [0, 1, 100, MAX_STORED_BLOCK, MAX_STORED_BLOCK + 1, 3 * MAX_STORED_BLOCK + 7] {
        let data = vec![5; len];
        for offset in 0..8 {
            let written = bits_written(offset, |w| Deflater::write_stored_blocks(w, &data, true));
            assert_eq!(written, stored_bits(len, offset as usize), "{len} bytes at bit {offset}");
        }
    }
}

#[test]
fn fixed_and_dynamic_estimates_are_exact() {
    let mut costs = BlockCosts::new();
    for data in [&b""[..], b"a", b"hello hello hello", &mixed_input(), &skewed(5000)] {
        for level in [0, 1, 6, 9] {
            let tokens = tokens_for(data, level);
            let frequencies = Frequencies::of(&tokens);

            let fixed = bits_written(0, |w| Deflater::write_fixed_block(w, &costs.fixed, &tokens, true));
            assert_eq!(fixed, costs.fixed.block_bits(&frequencies), "fixed, level {level}");

            let estimate = costs.dynamic.estimate_bits(&frequencies);
            costs.dynamic.build_from(&frequencies);
            let dynamic =
                bits_written(0, |w| Deflater::write_dynamic_block(w, &costs.dynamic, &tokens, true));
            assert_eq!(dynamic, estimate, "dynamic, level {level}");
        }
    }
}

#[test]
fn cheapest_block_writes_the_size_it_promised() {
    let mut costs = BlockCosts::new();
    for data in [noise(3000, 1), b"a".to_vec(), skewed(3000)] {
        let tokens = tokens_for(&data, 6);
        let frequencies = Frequencies::of(&tokens);
        for offset in 0..8 {
            let (_, promised) = costs.cheapest(&frequencies, data.len(), offset as usize);
            let written = bits_written(offset, |w| {
                Deflater::write_cheapest_block(w, &mut costs, &tokens, &data, true)
            });
            assert_eq!(written, promised, "at bit {offset}");
        }
    }
}

#[test]
fn each_block_type_is_chosen_when_smallest() {
    let mut costs = BlockCosts::new();
    let mut pick = |data: &[u8]| {
        let tokens = tokens_for(data, 6);
        costs.cheapest(&Frequencies::of(&tokens), data.len(), 0).0
    };

    assert_eq!(pick(&noise(5000, 2)), BlockType::Stored);
    assert_eq!(pick(b"a"), BlockType::Fixed);
    assert_eq!(pick(&skewed(5000)), BlockType::Dynamic);
}

#[test]
fn incompressible_input_barely_grows() {
    for level in 0..=9 {
        let data = noise(100_000, 3);
        let out = deflate_with(&data, DYNAMIC.level(CompressionLevel::new(level)));
        assert!(out.len() <= data.len() + 5 * 2 + 1, "level {level}: {}", out.len());
    }
}

#[test]
fn mixed_input_is_split_into_blocks() {
    let data = mixed_input();
    for level in [1, 6, 8, 9] {
        let mut deflater = Deflater::new();
        let mut writer = BitWriter::new();
        deflater.deflate_into(&data, DYNAMIC.level(CompressionLevel::new(level)), &mut writer);

        assert!(deflater.block_ends.len() > 1, "level {level}");
        assert!(crate::decompress(&writer.finish()).unwrap() == data, "level {level}");
    }
}

#[test]
fn splitting_beats_one_block() {
    // Each part wants different codes, so separate blocks are smaller.
    let data = mixed_input();
    for level in [8, 9] {
        let tokens = tokens_for(&data, level);
        let mut costs = BlockCosts::new();
        let (_, one_block) = costs.cheapest(&Frequencies::of(&tokens), data.len(), 0);
        let split = deflate_with(&data, DYNAMIC.level(CompressionLevel::new(level))).len() as u64 * 8;
        assert!(split < one_block * 95 / 100, "level {level}: {split} vs {one_block}");
    }
}

#[test]
fn higher_levels_split_at_least_as_well() {
    let data = mixed_input();
    let size = |level| deflate_with(&data, DYNAMIC.level(CompressionLevel::new(level))).len();
    assert!(size(9) <= size(8), "{} vs {}", size(9), size(8));
    assert!(size(8) <= size(7), "{} vs {}", size(8), size(7));
}

#[test]
fn long_input_spanning_several_search_segments_decodes() {
    // Noise gives one token per byte: more than one segment of the level 8 and 9
    // search.
    let mut data = noise(200_000, 4);
    data.extend(skewed(150_000));
    for level in [8, 9] {
        let out = deflate_with(&data, DYNAMIC.level(CompressionLevel::new(level)));
        assert!(crate::decompress(&out).unwrap() == data, "level {level}");
    }
}
