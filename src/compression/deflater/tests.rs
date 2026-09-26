use super::{Deflater, MAX_STORED_BLOCK};
use crate::io::bit_writer::BitWriter;

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::new();
    Deflater::new().deflate_into(data, &mut writer);
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
        deflater.deflate_into(data, &mut writer);
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
