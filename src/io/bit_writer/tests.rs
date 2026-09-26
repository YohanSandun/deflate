use super::BitWriter;
use crate::io::bit_reader::BitReader;

#[test]
fn new_writer_is_empty() {
    let writer = BitWriter::new();

    assert_eq!(writer.bit_len(), 0);
    assert_eq!(writer.finish(), Vec::<u8>::new());
}

#[test]
fn with_capacity_starts_empty() {
    let writer = BitWriter::with_capacity(1000);

    assert_eq!(writer.bit_len(), 0);
    assert_eq!(writer.finish(), Vec::<u8>::new());
}

#[test]
fn bits_are_packed_least_significant_first() {
    let mut writer = BitWriter::new();

    // 1, then 0, 0, then 1 0 1 (value 0b101 written LSB first: 1, 0, 1).
    writer.write_bits(1, 1);
    writer.write_bits(0, 2);
    writer.write_bits(0b101, 3);

    // Bits in order: 1 0 0 1 0 1 -> byte 0b00101001.
    assert_eq!(writer.bit_len(), 6);
    assert_eq!(writer.finish(), vec![0b0010_1001]);
}

#[test]
fn values_span_byte_boundaries() {
    let mut writer = BitWriter::new();

    writer.write_bits(0b111, 3);
    writer.write_bits(0x1FF, 9); // nine 1s: ends in the second byte
    writer.write_bits(0, 4);

    // 12 ones then 4 zeros: 0xFF, 0x0F.
    assert_eq!(writer.finish(), vec![0xFF, 0x0F]);
}

#[test]
fn sixteen_bit_values_are_little_endian() {
    let mut writer = BitWriter::new();

    writer.write_bits(0x1234, 16);

    assert_eq!(writer.finish(), vec![0x34, 0x12]);
}

#[test]
fn full_32_bit_values() {
    let mut writer = BitWriter::new();

    writer.write_bits(0xDEAD_BEEF, 32);
    writer.write_bits(1, 1);
    writer.write_bits(u32::MAX, 32);

    let mut expected = vec![0xEF, 0xBE, 0xAD, 0xDE];
    // A 1 bit, then 32 ones shifted one place: 0xFF 0xFF 0xFF 0xFF 0x01.
    expected.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x01]);
    assert_eq!(writer.finish(), expected);
}

#[test]
fn zero_bit_write_does_nothing() {
    let mut writer = BitWriter::new();

    writer.write_bits(0, 0);
    writer.write_bits(1, 1);
    writer.write_bits(0, 0);

    assert_eq!(writer.bit_len(), 1);
    assert_eq!(writer.finish(), vec![0x01]);
}

#[test]
fn finish_pads_the_last_byte_with_zeros() {
    let mut writer = BitWriter::new();

    writer.write_bits(0b11, 2);

    assert_eq!(writer.finish(), vec![0b0000_0011]);
}

#[test]
fn align_pads_to_the_next_byte_boundary() {
    let mut writer = BitWriter::new();

    writer.write_bits(0b101, 3);
    writer.align_to_byte();
    assert_eq!(writer.bit_len(), 8);

    writer.write_bits(0xAB, 8);

    assert_eq!(writer.finish(), vec![0b0000_0101, 0xAB]);
}

#[test]
fn align_when_already_aligned_does_nothing() {
    let mut writer = BitWriter::new();

    writer.align_to_byte();
    assert_eq!(writer.bit_len(), 0);

    writer.write_bits(0x5A, 8);
    writer.align_to_byte();
    writer.align_to_byte();

    assert_eq!(writer.bit_len(), 8);
    assert_eq!(writer.finish(), vec![0x5A]);
}

#[test]
fn write_bytes_appends_after_aligned_bits() {
    let mut writer = BitWriter::new();

    // Like a stored block: 3 header bits, padding, then raw bytes.
    writer.write_bits(0b001, 3);
    writer.align_to_byte();
    writer.write_bytes(b"abc");

    assert_eq!(writer.bit_len(), 32);
    assert_eq!(writer.finish(), vec![0x01, b'a', b'b', b'c']);
}

#[test]
fn write_bytes_then_more_bits() {
    let mut writer = BitWriter::new();

    writer.write_bytes(&[0x11, 0x22]);
    writer.write_bits(0b1, 1);
    writer.align_to_byte();
    writer.write_bytes(&[]);
    writer.write_bytes(&[0x33]);

    assert_eq!(writer.finish(), vec![0x11, 0x22, 0x01, 0x33]);
}

#[test]
fn large_write_bytes() {
    let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7) as u8).collect();
    let mut writer = BitWriter::new();

    writer.write_bits(0, 3);
    writer.align_to_byte();
    writer.write_bytes(&data);

    let out = writer.finish();
    assert_eq!(out.len(), 1 + data.len());
    assert!(out[1..] == data[..]);
}

#[test]
fn bit_len_counts_every_bit() {
    let mut writer = BitWriter::new();
    let mut expected = 0;

    for n in [1, 5, 7, 13, 16, 32, 3, 0, 9] {
        writer.write_bits(0, n);
        expected += n as usize;
        assert_eq!(writer.bit_len(), expected);
    }
}

#[test]
fn round_trips_through_bit_reader() {
    // Mixed widths, including every width 0..=32, read back with the decoder's reader.
    let mut x = 0x2545_F491u32;
    let mut values = Vec::new();
    for round in 0..20 {
        for n in 0..=32u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let value = if n == 32 { x } else { x & ((1u32 << n) - 1) };
            values.push((value, n));
        }
        // Occasionally realign, as block boundaries do.
        if round % 3 == 0 {
            values.push((u32::MAX, u32::MAX)); // marker: align
        }
    }

    let mut writer = BitWriter::new();
    for &(value, n) in &values {
        if n == u32::MAX {
            writer.align_to_byte();
        } else {
            writer.write_bits(value, n);
        }
    }
    let bytes = writer.finish();

    let mut reader = BitReader::new(&bytes);
    for &(value, n) in &values {
        if n == u32::MAX {
            reader.align_to_byte();
        } else {
            assert_eq!(reader.read_next_bits(n).unwrap(), value, "{n}-bit value");
        }
    }
}
