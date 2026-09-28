use super::*;
use crate::compression::tables::CODE_LENGTH_ORDER;

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

// --- build_lengths ---------------------------------------------------------------

fn lengths_for(frequencies: &[u32], max_bits: usize) -> Vec<u8> {
    let mut lengths = vec![0; frequencies.len()];
    build_lengths(frequencies, max_bits, &mut lengths);
    lengths
}

// Checks the lengths form a complete code within `max_bits`: the sum of 2^-length
// over used symbols is exactly 1.
fn assert_complete(lengths: &[u8], max_bits: usize) {
    let mut kraft = 0u32;
    for (symbol, &len) in lengths.iter().enumerate() {
        assert!(len as usize <= max_bits, "symbol {symbol}: length {len}");
        if len > 0 {
            kraft += 1 << (MAX_BITS - len as usize);
        }
    }
    assert_eq!(kraft, 1 << MAX_BITS, "not a complete code: {lengths:?}");
}

// Checks a more frequent symbol never has a longer code than a less frequent one.
fn assert_ordered(frequencies: &[u32], lengths: &[u8]) {
    for (a, (&freq_a, &len_a)) in frequencies.iter().zip(lengths).enumerate() {
        for (b, (&freq_b, &len_b)) in frequencies.iter().zip(lengths).enumerate() {
            if freq_a > freq_b && freq_b > 0 {
                assert!(len_a <= len_b, "symbols {a} and {b}: {lengths:?}");
            }
        }
    }
}

fn fibonacci(n: usize) -> Vec<u32> {
    let mut out = vec![1, 1];
    while out.len() < n {
        out.push(out[out.len() - 1] + out[out.len() - 2]);
    }
    out.truncate(n);
    out
}

// Deterministic pseudo-random numbers below `bound`.
fn pseudo_random(n: usize, bound: u32) -> Vec<u32> {
    let mut state = 0x2545_F491u32;
    (0..n)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) % bound
        })
        .collect()
}

#[test]
fn build_lengths_textbook_example() {
    // Merges 5+9, 12+13, 14+16, 25+30, then 45+55.
    assert_eq!(
        lengths_for(&[5, 9, 12, 13, 16, 45], MAX_BITS),
        [4, 4, 3, 3, 3, 1]
    );
}

#[test]
fn equal_frequencies_give_equal_lengths() {
    assert_eq!(lengths_for(&[7; 4], MAX_BITS), [2; 4]);
    assert_eq!(lengths_for(&[1; 8], MAX_BITS), [3; 8]);
}

#[test]
fn unused_symbols_get_length_0() {
    assert_eq!(lengths_for(&[3, 0, 1, 0, 1], MAX_BITS), [1, 0, 2, 0, 2]);
}

#[test]
fn one_used_symbol_still_gets_a_one_bit_code() {
    let lengths = lengths_for(&[0, 0, 5, 0], MAX_BITS);

    assert_eq!(lengths[2], 1, "{lengths:?}");
    assert_eq!(lengths.iter().filter(|&&len| len == 1).count(), 2, "{lengths:?}");
    assert_eq!(lengths.iter().filter(|&&len| len == 0).count(), 2, "{lengths:?}");
}

#[test]
fn no_used_symbols_gives_two_one_bit_codes() {
    // A block with no matches still has to describe a distance code.
    let lengths = lengths_for(&[0; DISTANCE_SYMBOLS], MAX_BITS);

    assert_eq!(lengths.iter().filter(|&&len| len == 1).count(), 2, "{lengths:?}");
    assert_eq!(lengths.iter().filter(|&&len| len == 0).count(), DISTANCE_SYMBOLS - 2);
}

#[test]
fn lengths_are_limited_to_max_bits() {
    // Fibonacci frequencies make the deepest possible tree: 29 levels unlimited.
    let frequencies = fibonacci(30);
    let lengths = lengths_for(&frequencies, MAX_BITS);
    assert_complete(&lengths, MAX_BITS);
    assert_ordered(&frequencies, &lengths);
}

#[test]
fn code_length_code_lengths_are_limited_to_7_bits() {
    let frequencies = fibonacci(CODE_LENGTH_SYMBOLS);
    let lengths = lengths_for(&frequencies, MAX_CODE_LENGTH_BITS);
    assert_complete(&lengths, MAX_CODE_LENGTH_BITS);
    assert_ordered(&frequencies, &lengths);
}

#[test]
fn lengths_for_a_full_literal_length_alphabet() {
    // Some symbols unused, the rest spread over a wide range.
    let frequencies: Vec<u32> = pseudo_random(MAX_LITERAL_LENGTH_CODES, 5000)
        .into_iter()
        .map(|f| if f < 500 { 0 } else { f })
        .collect();
    let lengths = lengths_for(&frequencies, MAX_BITS);

    assert_complete(&lengths, MAX_BITS);
    assert_ordered(&frequencies, &lengths);
    for (symbol, (&freq, &len)) in frequencies.iter().zip(&lengths).enumerate() {
        assert_eq!(freq == 0, len == 0, "symbol {symbol}");
    }
}

// --- encode_code_lengths ---------------------------------------------------------

fn run_lengths(lengths: &[u8]) -> Vec<Symbol> {
    let mut out = Vec::new();
    encode_code_lengths(lengths, &mut out);
    out
}

fn plain(lengths: &[u8]) -> Vec<Symbol> {
    lengths.iter().map(|&len| symbol(len as u16, 0, 0)).collect()
}

// Expands code length symbols back into lengths, as a decoder does.
fn expand(symbols: &[Symbol]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for s in symbols {
        let (repeat, value) = match s.symbol {
            0..=15 => (1, s.symbol as u8),
            16 => (3 + s.extra, *out.last().expect("16 with nothing to repeat")),
            17 => (3 + s.extra, 0),
            18 => (11 + s.extra, 0),
            other => panic!("code length symbol {other}"),
        };
        let expected_bits = [0, 2, 3, 7][s.symbol.saturating_sub(15) as usize];
        assert_eq!(s.extra_bits, expected_bits, "extra bits of {s:?}");
        assert!(u32::from(s.extra) < 1 << s.extra_bits, "extra of {s:?}");
        out.extend(std::iter::repeat_n(value, repeat as usize));
    }
    out
}

#[test]
fn short_sequences_are_sent_as_themselves() {
    let lengths = [1, 2, 3, 0, 0, 5, 5];
    assert_eq!(run_lengths(&lengths), plain(&lengths));
    assert_eq!(run_lengths(&[]), []);
}

#[test]
fn runs_of_3_to_10_zeros_use_17() {
    assert_eq!(run_lengths(&[0; 3]), [symbol(17, 3, 0)]);
    assert_eq!(run_lengths(&[0; 10]), [symbol(17, 3, 7)]);
}

#[test]
fn runs_of_11_to_138_zeros_use_18() {
    assert_eq!(run_lengths(&[0; 11]), [symbol(18, 7, 0)]);
    assert_eq!(run_lengths(&[0; 138]), [symbol(18, 7, 127)]);
}

#[test]
fn longer_zero_runs_are_split() {
    assert_eq!(run_lengths(&[0; 139]), [symbol(18, 7, 127), symbol(0, 0, 0)]);
    assert_eq!(run_lengths(&[0; 141]), [symbol(18, 7, 127), symbol(17, 3, 0)]);
    assert_eq!(run_lengths(&[0; 150]), [symbol(18, 7, 127), symbol(18, 7, 1)]);
    assert_eq!(run_lengths(&[0; 276]), [symbol(18, 7, 127), symbol(18, 7, 127)]);
}

#[test]
fn repeats_of_a_non_zero_length_use_16_after_the_first() {
    assert_eq!(run_lengths(&[8; 3]), plain(&[8; 3]));
    assert_eq!(run_lengths(&[8; 4]), [symbol(8, 0, 0), symbol(16, 2, 0)]);
    assert_eq!(run_lengths(&[8; 7]), [symbol(8, 0, 0), symbol(16, 2, 3)]);
}

#[test]
fn longer_non_zero_runs_are_split() {
    // 13 repeats: 6 + 6, then 1 left over as itself.
    assert_eq!(
        run_lengths(&[4; 14]),
        [symbol(4, 0, 0), symbol(16, 2, 3), symbol(16, 2, 3), symbol(4, 0, 0)]
    );
    // 10 repeats: 6, then 4.
    assert_eq!(
        run_lengths(&[4; 11]),
        [symbol(4, 0, 0), symbol(16, 2, 3), symbol(16, 2, 1)]
    );
}

#[test]
fn mixed_runs() {
    assert_eq!(
        run_lengths(&[3, 3, 3, 3, 0, 0, 0, 7]),
        [symbol(3, 0, 0), symbol(16, 2, 0), symbol(17, 3, 0), symbol(7, 0, 0)]
    );
}

#[test]
fn encode_code_lengths_appends() {
    let mut out = vec![symbol(9, 0, 0)];
    encode_code_lengths(&[0; 3], &mut out);
    assert_eq!(out, [symbol(9, 0, 0), symbol(17, 3, 0)]);
}

#[test]
fn run_length_symbols_expand_to_the_lengths() {
    // Runs of every length from 1 to 150 of zeros and non-zeros, back to back.
    let mut lengths = Vec::new();
    for (i, run) in pseudo_random(200, 150).into_iter().enumerate() {
        let value = if i % 2 == 0 { 0 } else { 1 + (i % 15) as u8 };
        lengths.extend(std::iter::repeat_n(value, run as usize + 1));
    }
    assert!(expand(&run_lengths(&lengths)) == lengths);
}

// --- DynamicCodes ----------------------------------------------------------------

fn built(tokens: &[Token]) -> Box<DynamicCodes> {
    let mut codes = Box::new(DynamicCodes::new());
    codes.build(tokens);
    codes
}

fn literals(bytes: &[u8]) -> Vec<Token> {
    bytes.iter().map(|&b| Token::Literal(b)).collect()
}

fn sample_tokens() -> Vec<Vec<Token>> {
    let mut with_matches = literals(b"abcabc");
    with_matches.push(Token::Match { length: 3, distance: 3 });
    with_matches.push(Token::Match { length: 258, distance: 1 });
    with_matches.push(Token::Match { length: 17, distance: 32_768 });
    with_matches.extend(literals(b"zz"));

    vec![
        Vec::new(),
        literals(b"a"),
        literals(b"aab"),
        literals(&(0..=255).collect::<Vec<u8>>()),
        with_matches,
    ]
}

#[test]
fn counts_are_in_range() {
    for tokens in sample_tokens() {
        let codes = built(&tokens);
        assert!((257..=286).contains(&codes.literal_length_count), "{tokens:?}");
        assert!((1..=30).contains(&codes.distance_count), "{tokens:?}");
        assert!((4..=19).contains(&codes.code_length_count), "{tokens:?}");
    }
}

#[test]
fn literals_only_block() {
    let codes = built(&literals(b"aab"));

    // End-of-block is the highest symbol used. No distances are used, but two
    // still get a code.
    assert_eq!(codes.literal_length_count, 257);
    assert_eq!(codes.distance_count, 2);

    assert!(codes.literal_length[b'a' as usize].len > 0);
    assert!(codes.literal_length[b'b' as usize].len > 0);
    assert!(codes.literal_length[END_OF_BLOCK].len > 0);
    assert_eq!(codes.literal_length[b'c' as usize].len, 0);
    assert!(codes.literal_length[b'a' as usize].len <= codes.literal_length[b'b' as usize].len);
}

#[test]
fn empty_block_still_has_an_end_of_block_code() {
    let codes = built(&[]);
    assert_eq!(codes.literal_length_count, 257);
    assert_eq!(codes.literal_length[END_OF_BLOCK].len, 1);
}

#[test]
fn matches_count_their_length_and_distance_symbols() {
    let mut tokens = literals(b"abc");
    tokens.push(Token::Match { length: 3, distance: 3 });
    let codes = built(&tokens);

    // Length 3 is symbol 257, distance 3 is symbol 2.
    assert_eq!(codes.literal_length_count, 258);
    assert_eq!(codes.distance_count, 3);
    assert!(codes.literal_length[257].len > 0);
    assert!(codes.distance[2].len > 0);

    tokens.push(Token::Match { length: 258, distance: 32_768 });
    let codes = built(&tokens);
    assert_eq!(codes.literal_length_count, 286);
    assert_eq!(codes.distance_count, 30);
}

#[test]
fn codes_are_canonical_and_complete() {
    for tokens in sample_tokens() {
        let codes = built(&tokens);
        for (name, table, max_bits) in [
            ("literal/length", &codes.literal_length[..], MAX_BITS),
            ("distance", &codes.distance[..], MAX_BITS),
            ("code length", &codes.code_length[..], MAX_CODE_LENGTH_BITS),
        ] {
            let lengths: Vec<u8> = table.iter().map(|code| code.len).collect();
            assert_complete(&lengths, max_bits);

            let mut canonical = vec![Code::default(); table.len()];
            canonical_codes(&lengths, &mut canonical);
            assert_eq!(table, canonical, "{name} codes for {tokens:?}");
        }
    }
}

#[test]
fn code_length_symbols_expand_to_the_sent_lengths() {
    for tokens in sample_tokens() {
        let codes = built(&tokens);
        let mut want: Vec<u8> = codes.literal_length[..codes.literal_length_count]
            .iter()
            .map(|code| code.len)
            .collect();
        want.extend(codes.distance[..codes.distance_count].iter().map(|code| code.len));

        assert_eq!(expand(&codes.code_length_symbols), want, "{tokens:?}");
        for s in &codes.code_length_symbols {
            assert!(codes.code_length[s.symbol as usize].len > 0, "{s:?} has no code");
        }
    }
}

#[test]
fn code_length_count_leaves_out_only_unused_codes() {
    for tokens in sample_tokens() {
        let codes = built(&tokens);
        let count = codes.code_length_count;

        for &symbol in &CODE_LENGTH_ORDER[count..] {
            assert_eq!(codes.code_length[symbol].len, 0, "{tokens:?}");
        }
        if count > 4 {
            assert!(codes.code_length[CODE_LENGTH_ORDER[count - 1]].len > 0, "{tokens:?}");
        }
    }
}

#[test]
fn rebuilding_forgets_the_previous_block() {
    let samples = sample_tokens();
    let mut codes = Box::new(DynamicCodes::new());

    for tokens in samples.iter().rev().chain(&samples) {
        codes.build(tokens);
        let fresh = built(tokens);

        assert_eq!(codes.literal_length, fresh.literal_length, "{tokens:?}");
        assert_eq!(codes.distance, fresh.distance, "{tokens:?}");
        assert_eq!(codes.code_length, fresh.code_length, "{tokens:?}");
        assert_eq!(codes.literal_length_count, fresh.literal_length_count);
        assert_eq!(codes.distance_count, fresh.distance_count);
        assert_eq!(codes.code_length_count, fresh.code_length_count);
        assert_eq!(codes.code_length_symbols, fresh.code_length_symbols);
    }
}

// --- Frequencies and block sizes ---------------------------------------------------

#[test]
fn frequencies_count_literal_length_and_distance_symbols() {
    let mut tokens = literals(b"aab");
    tokens.push(Token::Match { length: 3, distance: 3 });
    let frequencies = Frequencies::of(&tokens);

    assert_eq!(frequencies.literal_length[b'a' as usize], 2);
    assert_eq!(frequencies.literal_length[b'b' as usize], 1);
    assert_eq!(frequencies.literal_length[257], 1, "length 3");
    assert_eq!(frequencies.distance[2], 1, "distance 3");
    assert_eq!(frequencies.literal_length[END_OF_BLOCK], 0, "not counted");
}

#[test]
fn frequencies_add_and_subtract() {
    let first = literals(b"hello");
    let second = literals(b"world");
    let mut both = first.clone();
    both.extend(&second);

    let mut sum = Frequencies::of(&first);
    sum.add_all(&Frequencies::of(&second));
    assert_eq!(sum, Frequencies::of(&both));
    assert_eq!(
        Frequencies::difference(&sum, &Frequencies::of(&first)),
        Frequencies::of(&second)
    );
}

#[test]
fn fixed_block_bits() {
    // Header, "a" (8 bits), end-of-block (7 bits).
    let codes = FixedCodes::new();
    assert_eq!(codes.block_bits(&Frequencies::of(&literals(b"a"))), 3 + 8 + 7);

    // A match of length 11 (symbol 265, 7 bits + 1 extra) at distance 5 (5 bits +
    // 1 extra).
    let frequencies = Frequencies::of(&[Token::Match { length: 11, distance: 5 }]);
    assert_eq!(codes.block_bits(&frequencies), 3 + 8 + 6 + 7);
}

#[test]
fn estimate_matches_the_built_block() {
    let mut codes = DynamicCodes::new();
    let mut random = literals(&pseudo_random(3000, 256).iter().map(|&f| f as u8).collect::<Vec<_>>());
    random.push(Token::Match { length: 100, distance: 2000 });

    for tokens in sample_tokens().into_iter().chain([random]) {
        let frequencies = Frequencies::of(&tokens);
        let estimate = codes.estimate_bits(&frequencies);
        codes.build_from(&frequencies);
        assert_eq!(estimate, codes.block_bits(&frequencies), "{tokens:?}");
    }
}

#[test]
fn estimate_leaves_the_built_codes_alone() {
    let tokens = literals(b"built codes");
    let mut codes = built(&tokens);
    codes.estimate_bits(&Frequencies::of(&literals(b"something else entirely")));

    let fresh = built(&tokens);
    assert_eq!(codes.literal_length, fresh.literal_length);
    assert_eq!(codes.distance, fresh.distance);
    assert_eq!(codes.code_length, fresh.code_length);
    assert_eq!(codes.literal_length_count, fresh.literal_length_count);
    assert_eq!(codes.distance_count, fresh.distance_count);
    assert_eq!(codes.code_length_count, fresh.code_length_count);
    assert_eq!(codes.code_length_symbols, fresh.code_length_symbols);
}
