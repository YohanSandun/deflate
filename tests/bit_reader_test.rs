use deflate::io::bit_reader::BitReader;

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

    let bits: Vec<u8> = (0..8)
        .map(|_| reader.read_next_bit().unwrap())
        .collect();

    assert_eq!(
        bits,
        vec![1, 0, 0, 0, 1, 1, 0, 1]
    );
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

    assert_eq!(
        result,
        Err("unexpected end of input".to_string())
    );
}

#[test]
fn read_next_bit_returns_error_after_all_bits_are_consumed() {
    let data = [0b10110001];
    let mut reader = BitReader::new(&data);

    // Consume all 8 bits.
    for _ in 0..8 {
        assert!(reader.read_next_bit().is_ok());
    }

    assert_eq!(
        reader.read_next_bit(),
        Err("unexpected end of input".to_string())
    );
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

    assert_eq!(
        reader.read_next_bits(1),
        Err("unexpected end of input".to_string())
    );
}

#[test]
fn read_next_bits_returns_error_when_not_enough_bits() {
    let data = [0b00000001];
    let mut reader = BitReader::new(&data);

    // Only 8 bits available, asking for 9.
    assert_eq!(
        reader.read_next_bits(9),
        Err("unexpected end of input".to_string())
    );
}

#[test]
fn read_next_bits_returns_error_after_consuming_input() {
    let data = [0b00000001];
    let mut reader = BitReader::new(&data);

    reader.read_next_bits(8).unwrap();

    assert_eq!(
        reader.read_next_bits(1),
        Err("unexpected end of input".to_string())
    );
}

#[test]
fn read_next_bits_rejects_more_than_32_bits() {
    let data = [0xFF; 8];
    let mut reader = BitReader::new(&data);

    assert_eq!(
        reader.read_next_bits(33),
        Err("cannot read more than 32 bits".to_string())
    );
}

#[test]
fn read_next_bits_rejects_large_values() {
    let data = [0xFF; 8];
    let mut reader = BitReader::new(&data);

    assert_eq!(
        reader.read_next_bits(100),
        Err("cannot read more than 32 bits".to_string())
    );
}

#[test]
fn read_next_bits_does_not_consume_data_when_n_is_too_large() {
    let data = [0b00000001];
    let mut reader = BitReader::new(&data);

    assert!(reader.read_next_bits(33).is_err());

    // The invalid request should not advance the reader.
    assert_eq!(reader.read_next_bit().unwrap(), 1);
}

#[test]
fn peek_next_bits_pads_with_zeros_past_end_of_input() {
    let data = [0b10110001];
    let reader = BitReader::new(&data);

    // Only 8 bits available; the missing high bits read as 0.
    assert_eq!(reader.peek_next_bits(12).unwrap(), 0b10110001);
}

#[test]
fn peek_next_bits_on_empty_input_returns_zero() {
    let data: [u8; 0] = [];
    let reader = BitReader::new(&data);

    assert_eq!(reader.peek_next_bits(9).unwrap(), 0);
}
