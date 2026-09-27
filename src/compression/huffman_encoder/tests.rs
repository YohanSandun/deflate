use super::*;

// The code as the RFC writes it (most significant bit first), for comparing.
fn unreversed(code: Code) -> (u16, u8) {
    let bits = code.bits.reverse_bits() >> (16 - code.len as u32);
    (bits, code.len)
}

// --- canonical_codes ------------------------------------------------------------

#[test]
fn canonical_codes_match_the_rfc_example() {
    // RFC 1951 section 3.2.2: lengths (3, 3, 3, 3, 3, 2, 4, 4) for A..H give
    // 010, 011, 100, 101, 110, 00, 1110, 1111.
    let mut codes = [Code::default(); 8];
    canonical_codes(&[3, 3, 3, 3, 3, 2, 4, 4], &mut codes);

    let expected = [
        (0b010, 3),
        (0b011, 3),
        (0b100, 3),
        (0b101, 3),
        (0b110, 3),
        (0b00, 2),
        (0b1110, 4),
        (0b1111, 4),
    ];
    for (symbol, &want) in expected.iter().enumerate() {
        assert_eq!(unreversed(codes[symbol]), want, "symbol {symbol}");
    }
}

#[test]
fn unused_symbols_get_no_code() {
    let mut codes = [Code { bits: 99, len: 99 }; 4];
    canonical_codes(&[1, 0, 1, 0], &mut codes);

    assert_eq!(unreversed(codes[0]), (0b0, 1));
    assert_eq!(codes[1], Code::default());
    assert_eq!(unreversed(codes[2]), (0b1, 1));
    assert_eq!(codes[3], Code::default());
}

#[test]
fn codes_are_stored_reversed() {
    let mut codes = [Code::default(); 2];
    canonical_codes(&[2, 1], &mut codes);

    // Symbol 1 is "0", symbol 0 is "10": reversed, that's 0b01.
    assert_eq!(codes[0], Code { bits: 0b01, len: 2 });
    assert_eq!(codes[1], Code { bits: 0b0, len: 1 });
}

// --- FixedCodes -----------------------------------------------------------------

#[test]
fn fixed_literal_length_codes_match_the_rfc_table() {
    let codes = FixedCodes::new();

    // RFC 1951 section 3.2.6: the first and last code of each range.
    for (symbol, want) in [
        (0, (0b0011_0000, 8)),
        (143, (0b1011_1111, 8)),
        (144, (0b1_1001_0000, 9)),
        (255, (0b1_1111_1111, 9)),
        (256, (0b000_0000, 7)),
        (279, (0b001_0111, 7)),
        (280, (0b1100_0000, 8)),
        (287, (0b1100_0111, 8)),
    ] {
        assert_eq!(
            unreversed(codes.literal_length[symbol]),
            want,
            "symbol {symbol}"
        );
    }
}

#[test]
fn fixed_distance_codes_are_their_own_5_bit_values() {
    let codes = FixedCodes::new();

    for symbol in 0..DISTANCE_SYMBOLS {
        assert_eq!(unreversed(codes.distance[symbol]), (symbol as u16, 5));
    }
}

// --- length_symbol / distance_symbol --------------------------------------------

fn symbol(symbol: u16, extra_bits: u8, extra: u16) -> Symbol {
    Symbol {
        symbol,
        extra_bits,
        extra,
    }
}

#[test]
fn length_symbols() {
    assert_eq!(length_symbol(3), symbol(257, 0, 0));
    assert_eq!(length_symbol(10), symbol(264, 0, 0));
    assert_eq!(length_symbol(11), symbol(265, 1, 0));
    assert_eq!(length_symbol(12), symbol(265, 1, 1));
    assert_eq!(length_symbol(13), symbol(266, 1, 0));
    assert_eq!(length_symbol(227), symbol(284, 5, 0));
    assert_eq!(length_symbol(257), symbol(284, 5, 30));
    // 258 has its own symbol with no extra bits, not 284 with extra 31.
    assert_eq!(length_symbol(258), symbol(285, 0, 0));
}

#[test]
fn every_length_round_trips_through_the_tables() {
    for length in 3..=258u16 {
        let s = length_symbol(length);
        let index = (s.symbol - 257) as usize;
        assert_eq!(s.extra_bits, LENGTH_EXTRA_BITS[index], "length {length}");
        assert!(u32::from(s.extra) < 1 << s.extra_bits, "length {length}");
        assert_eq!(LENGTH_BASE[index] + s.extra, length);
    }
}

#[test]
fn distance_symbols() {
    assert_eq!(distance_symbol(1), symbol(0, 0, 0));
    assert_eq!(distance_symbol(4), symbol(3, 0, 0));
    assert_eq!(distance_symbol(5), symbol(4, 1, 0));
    assert_eq!(distance_symbol(6), symbol(4, 1, 1));
    assert_eq!(distance_symbol(24_577), symbol(29, 13, 0));
    assert_eq!(distance_symbol(32_768), symbol(29, 13, 8191));
}

#[test]
fn every_distance_round_trips_through_the_tables() {
    for distance in 1..=32_768u16 {
        let s = distance_symbol(distance);
        let index = s.symbol as usize;
        assert_eq!(
            s.extra_bits, DISTANCE_EXTRA_BITS[index],
            "distance {distance}"
        );
        assert!(
            u32::from(s.extra) < 1 << s.extra_bits,
            "distance {distance}"
        );
        assert_eq!(DISTANCE_BASE[index] + s.extra, distance);
    }
}
