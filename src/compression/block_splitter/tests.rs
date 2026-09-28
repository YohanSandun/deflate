use super::*;

fn literals(bytes: &[u8]) -> Vec<Token> {
    bytes.iter().map(|&b| Token::Literal(b)).collect()
}

// Deterministic pseudo-random bytes below `bound`.
fn pseudo_random(len: usize, bound: u32, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((state >> 16) % bound) as u8
        })
        .collect()
}

fn split_ends(tokens: &[Token], level: u8) -> Vec<usize> {
    let mut ends = Vec::new();
    split(tokens, level, &mut BlockCosts::new(), &mut ends);
    ends
}

// The estimated size of the blocks ending at `ends`.
fn total_bits(tokens: &[Token], ends: &[usize]) -> u64 {
    let mut costs = BlockCosts::new();
    let mut start = 0;
    let mut bits = 0;
    for &end in ends {
        let block = &tokens[start..end];
        bits += costs.estimate(&Frequencies::of(block), byte_len(block));
        start = end;
    }
    bits
}

// --- stored_bits ------------------------------------------------------------------

#[test]
fn stored_bits_of_one_block() {
    // 3 header bits, padding to the byte, LEN and NLEN, then the bytes.
    assert_eq!(stored_bits(0, 0), 3 + 5 + 32);
    assert_eq!(stored_bits(0, 5), 3 + 32);
    assert_eq!(stored_bits(0, 6), 3 + 7 + 32);
    assert_eq!(stored_bits(10, 0), 3 + 5 + 32 + 80);
    assert_eq!(
        stored_bits(MAX_STORED_BLOCK, 0),
        40 + 8 * MAX_STORED_BLOCK as u64
    );
}

#[test]
fn stored_bits_of_several_blocks() {
    // The second block starts byte-aligned: 3 header bits then 5 of padding.
    let len = MAX_STORED_BLOCK + 1;
    assert_eq!(stored_bits(len, 0), 2 * 40 + 8 * len as u64);
    assert_eq!(stored_bits(len, 5), 35 + 40 + 8 * len as u64);
}

// --- split --------------------------------------------------------------------------

#[test]
fn empty_input_is_one_empty_block() {
    for level in 0..=9 {
        assert_eq!(split_ends(&[], level), [0], "level {level}");
    }
}

#[test]
fn ends_are_increasing_and_cover_every_token() {
    let mut data = pseudo_random(20_000, 4, 1);
    data.extend(pseudo_random(20_000, 256, 2));
    data.extend(vec![b'z'; 5_000]);
    let tokens = literals(&data);

    for level in 0..=9 {
        let ends = split_ends(&tokens, level);
        assert_eq!(ends.last(), Some(&tokens.len()), "level {level}");
        assert!(ends[0] > 0, "level {level}: empty first block");
        assert!(ends.windows(2).all(|w| w[0] < w[1]), "level {level}: {ends:?}");
    }
}

#[test]
fn small_inputs_are_one_block() {
    let tokens = literals(&pseudo_random(100, 256, 3));
    for level in 0..=9 {
        assert_eq!(split_ends(&tokens, level), [tokens.len()], "level {level}");
    }
}

#[test]
fn uniform_input_stays_one_block() {
    // One distribution throughout: another header would only cost bits.
    let tokens = literals(&pseudo_random(60_000, 16, 4));
    for level in [8, 9] {
        assert_eq!(split_ends(&tokens, level), [tokens.len()], "level {level}");
    }
}

#[test]
fn boundary_lands_where_the_data_changes() {
    // 4 symbols, then all 256: the best split is right at the change.
    let mut data = pseudo_random(30_000, 4, 5);
    data.extend(pseudo_random(30_000, 256, 6));
    let tokens = literals(&data);

    let ends = split_ends(&tokens, 9);
    assert!(
        ends.iter().any(|&end| end.abs_diff(30_000) <= 64),
        "{ends:?}"
    );
}

#[test]
fn optimal_split_is_never_worse_than_one_block() {
    let mut data = pseudo_random(25_000, 3, 7);
    data.extend(pseudo_random(25_000, 256, 8));
    data.extend(pseudo_random(25_000, 20, 9));
    let tokens = literals(&data);

    let one_block = total_bits(&tokens, &[tokens.len()]);
    for level in [8, 9] {
        let ends = split_ends(&tokens, level);
        assert!(ends.len() >= 3, "level {level}: {ends:?}");
        assert!(total_bits(&tokens, &ends) < one_block, "level {level}");
    }
}

#[test]
fn level_9_finds_a_split_at_least_as_small_as_level_8() {
    let mut data = pseudo_random(40_000, 5, 10);
    data.extend(pseudo_random(10_000, 256, 11));
    data.extend(pseudo_random(40_000, 40, 12));
    let tokens = literals(&data);

    let level_8 = total_bits(&tokens, &split_ends(&tokens, 8));
    let level_9 = total_bits(&tokens, &split_ends(&tokens, 9));
    assert!(level_9 <= level_8, "{level_9} vs {level_8}");
}

#[test]
fn greedy_starts_a_block_when_the_data_changes() {
    let mut data = pseudo_random(4 * CHUNK_TOKENS, 4, 13);
    data.extend(pseudo_random(4 * CHUNK_TOKENS, 256, 14));
    let tokens = literals(&data);

    let ends = split_ends(&tokens, 6);
    assert!(ends.contains(&(4 * CHUNK_TOKENS)), "{ends:?}");
}

#[test]
fn search_segments_are_stitched_together() {
    let tokens = literals(&pseudo_random(SEGMENT_TOKENS + 5_000, 256, 15));
    for level in [8, 9] {
        let ends = split_ends(&tokens, level);
        assert!(ends.contains(&SEGMENT_TOKENS), "level {level}: {ends:?}");
        assert_eq!(ends.last(), Some(&tokens.len()));
        assert!(ends.windows(2).all(|w| w[0] < w[1]), "level {level}: {ends:?}");
    }
}

// --- RunningTotals ------------------------------------------------------------------

#[test]
fn running_totals_match_direct_counts() {
    let mut tokens = literals(b"abcabcabc");
    tokens.push(Token::Match {
        length: 10,
        distance: 3,
    });
    tokens.extend(literals(&pseudo_random(500, 256, 16)));

    let totals = RunningTotals::new(&tokens, 7);
    for (start, end) in [(0, 0), (0, 5), (3, 10), (9, 10), (0, tokens.len()), (13, 400)] {
        let (frequencies, bytes) = totals.range(start, end);
        assert_eq!(frequencies, Frequencies::of(&tokens[start..end]), "{start}..{end}");
        assert_eq!(bytes, byte_len(&tokens[start..end]), "{start}..{end}");
    }
}
