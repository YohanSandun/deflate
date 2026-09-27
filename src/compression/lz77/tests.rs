use super::*;

fn tokens(data: &[u8], level: u8) -> Vec<Token> {
    let mut tokens = Vec::new();
    MatchFinder::new().find_tokens(data, level, &mut tokens);
    tokens
}

// Rebuilds the input from the tokens, checking every match is in range.
fn replay(tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::new();
    for &token in tokens {
        match token {
            Token::Literal(byte) => out.push(byte),
            Token::Match { length, distance } => {
                let (length, distance) = (length as usize, distance as usize);
                assert!((MIN_MATCH..=MAX_MATCH).contains(&length), "length {length}");
                assert!(
                    (1..=MAX_DISTANCE).contains(&distance),
                    "distance {distance}"
                );
                assert!(
                    distance <= out.len(),
                    "distance {distance} before the start"
                );

                // Byte by byte: a match may overlap the bytes it produces.
                let start = out.len() - distance;
                for i in 0..length {
                    out.push(out[start + i]);
                }
            }
        }
    }
    out
}

fn inputs() -> Vec<Vec<u8>> {
    let mut x = 0x2545_F491u32;
    vec![
        Vec::new(),
        b"a".to_vec(),
        b"ab".to_vec(),
        b"abcabcabcabc".to_vec(),
        b"hello hello hello hello".to_vec(),
        vec![0; 100_000],
        (0..=255u8).cycle().take(70_000).collect(),
        (0..100_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect(),
    ]
}

#[test]
fn tokens_replay_to_the_input_at_every_level() {
    for level in 0..=9 {
        for data in inputs() {
            assert!(
                replay(&tokens(&data, level)) == data,
                "level {level}, length {}",
                data.len()
            );
        }
    }
}

#[test]
fn level_0_emits_only_literals() {
    let data = b"hello hello hello hello";
    let expected: Vec<Token> = data.iter().map(|&b| Token::Literal(b)).collect();

    assert_eq!(tokens(data, 0), expected);
}

#[test]
fn repeats_become_matches() {
    for level in 1..=9 {
        let tokens = tokens(b"abcabcabcabc", level);
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Match { .. })),
            "level {level}: {tokens:?}"
        );
        assert!(tokens.len() < 12, "level {level}");
    }
}

#[test]
fn long_runs_use_maximum_length_matches() {
    let tokens = tokens(&[7; 10_000], 6);

    assert!(tokens.contains(&Token::Match {
        length: MAX_MATCH as u16,
        distance: 1
    }));
}

#[test]
fn match_finder_can_be_reused() {
    let mut finder = MatchFinder::new();
    let mut reused = Vec::new();

    for data in inputs() {
        finder.find_tokens(&data, 6, &mut reused);
        assert_eq!(reused, tokens(&data, 6), "length {}", data.len());
    }
}

// --- greedy parsing (levels 1-3) ---------------------------------------------------

const GREEDY: [u8; 3] = [1, 2, 3];

fn lit(bytes: &[u8]) -> Vec<Token> {
    bytes.iter().map(|&b| Token::Literal(b)).collect()
}

fn m(length: u16, distance: u16) -> Token {
    Token::Match { length, distance }
}

#[test]
fn tiny_inputs_are_literals() {
    for level in GREEDY {
        assert_eq!(tokens(b"", level), [], "level {level}");
        assert_eq!(tokens(b"a", level), lit(b"a"), "level {level}");
        assert_eq!(tokens(b"ab", level), lit(b"ab"), "level {level}");
        assert_eq!(tokens(b"abc", level), lit(b"abc"), "level {level}");
    }
}

#[test]
fn simple_repeat_is_one_match() {
    for level in GREEDY {
        // A distance (3 back), not the position of the earlier copy (0).
        let mut want = lit(b"abc");
        want.push(m(3, 3));
        assert_eq!(tokens(b"abcabc", level), want, "level {level}");
    }
}

#[test]
fn matches_can_overlap_themselves() {
    for level in GREEDY {
        // One match covers the run, and the bytes it covers aren't emitted again.
        assert_eq!(
            tokens(b"aaaaaa", level),
            [Token::Literal(b'a'), m(5, 1)],
            "level {level}"
        );
    }
}

#[test]
fn bytes_after_a_match_are_kept() {
    for level in GREEDY {
        let mut want = lit(b"xyz");
        want.push(m(3, 3));
        want.extend(lit(b"qr"));
        assert_eq!(tokens(b"xyzxyzqr", level), want, "level {level}");
    }
}

#[test]
fn two_byte_repeats_are_not_matches() {
    for level in GREEDY {
        // "ab" repeats, but 2 bytes is below MIN_MATCH.
        assert_eq!(tokens(b"abab", level), lit(b"abab"), "level {level}");
        assert_eq!(tokens(b"abcab", level), lit(b"abcab"), "level {level}");
    }
}

#[test]
fn hash_collisions_are_not_matches() {
    // [1, 0, 1] and [1, 46, 20] have the same hash3 but only share their first
    // byte, so there's no match. (Pick a new colliding pair if the hash changes.)
    assert_eq!(hash3(&[1, 0, 1], 0), hash3(&[1, 46, 20], 0), "pair must collide");

    let data = [1, 0, 1, 1, 46, 20];
    for level in GREEDY {
        assert_eq!(tokens(&data, level), lit(&data), "level {level}");
    }
}

#[test]
fn nearest_match_wins_a_tie() {
    for level in GREEDY {
        // The last "abc" matches both earlier ones; the nearer (4 back) is cheaper.
        let got = tokens(b"abcXabcYabc", level);
        assert_eq!(got.last(), Some(&m(3, 4)), "level {level}: {got:?}");
    }
}

#[test]
fn longest_match_beats_nearest() {
    for level in GREEDY {
        // "abcY" (4 back) matches 3 bytes, "abcde" (10 back) matches 5.
        let got = tokens(b"abcdeXabcYabcde", level);
        assert_eq!(got.last(), Some(&m(5, 10)), "level {level}: {got:?}");
    }
}

#[test]
fn match_at_exactly_the_window_size() {
    // "WXYZ", filler, then "WXYZ" again exactly 32 KB later: still in reach.
    let mut data = b"WXYZ".to_vec();
    data.resize(MAX_DISTANCE, b'.');
    data.extend(b"WXYZ");

    for level in GREEDY {
        let got = tokens(&data, level);
        assert!(replay(&got) == data, "level {level}");
        assert_eq!(got.last(), Some(&m(4, MAX_DISTANCE as u16)), "level {level}");
    }
}

#[test]
fn nothing_matches_beyond_the_window() {
    // One byte further: "WXYZ" is out of reach, so it must be literals. (`replay`
    // also checks every distance is at most MAX_DISTANCE.)
    let mut data = b"WXYZ".to_vec();
    data.resize(MAX_DISTANCE + 1, b'.');
    data.extend(b"WXYZ");

    for level in GREEDY {
        let got = tokens(&data, level);
        assert!(replay(&got) == data, "level {level}");
        assert_eq!(got[got.len() - 4..], lit(b"WXYZ"), "level {level}");
    }
}

#[test]
fn inputs_longer_than_the_window_are_parsed_from_the_start() {
    let data: Vec<u8> = (0..3 * MAX_DISTANCE as u32).map(|i| (i % 251) as u8).collect();

    for level in GREEDY {
        assert!(replay(&tokens(&data, level)) == data, "level {level}");
    }
}

#[test]
fn every_greedy_level_reaches_maximum_length() {
    for level in GREEDY {
        // nice_length stops the chain search early; it doesn't cap the length.
        let got = tokens(&[7; 1000], level);
        assert!(got.contains(&m(MAX_MATCH as u16, 1)), "level {level}");
    }
}

#[test]
fn greedy_output_replays_to_the_input() {
    // The shared inputs, at the greedy levels only (the every-level test also
    // needs lazy matching).
    for level in GREEDY {
        for data in inputs() {
            assert!(
                replay(&tokens(&data, level)) == data,
                "level {level}, length {}",
                data.len()
            );
        }
    }
}

#[test]
fn reuse_after_a_longer_input() {
    // Chain entries left from a longer input must not leak into a shorter one.
    let mut finder = MatchFinder::new();
    let mut reused = Vec::new();
    let long: Vec<u8> = (0..50_000u32).map(|i| (i % 97) as u8).collect();

    for level in GREEDY {
        finder.find_tokens(&long, level, &mut reused);
        finder.find_tokens(b"abcabc", level, &mut reused);
        assert_eq!(reused, tokens(b"abcabc", level), "level {level}");
    }
}

#[test]
fn text_compresses_to_well_under_one_token_per_byte() {
    let text = include_bytes!("../../../tests/data/dynamic_text.txt");

    for level in GREEDY {
        let got = tokens(text, level);
        assert!(replay(&got) == text, "level {level}");
        assert!(
            got.len() < text.len() / 2,
            "level {level}: {} tokens for {} bytes",
            got.len(),
            text.len()
        );
    }
}

#[test]
fn positions_inside_a_match_can_be_matched_later() {
    // "abcdef" twice (the second a match covering positions 6..12), then "cdef":
    // it's 4 back inside that match, or 10 back in the original. Only reachable
    // at 4 if the positions a match covers go into the hash chains too.
    for level in GREEDY {
        let got = tokens(b"abcdefabcdefcdef", level);
        assert_eq!(got.last(), Some(&m(4, 4)), "level {level}: {got:?}");
    }
}

// --- lazy parsing (levels 4-9) -----------------------------------------------------

const LAZY: [u8; 6] = [4, 5, 6, 7, 8, 9];

#[test]
fn lazy_prefers_a_longer_match_one_byte_later() {
    // At the second "a", "abc" matches 3; one byte later, "bcde" matches 4.
    // Greedy takes the 3, lazy emits "a" as a literal and takes the 4.
    let data = b"abcbcde|abcde";

    let mut greedy = lit(b"abcbcde|");
    greedy.push(m(3, 8));
    greedy.extend(lit(b"de"));
    for level in GREEDY {
        assert_eq!(tokens(data, level), greedy, "level {level}");
    }

    let mut lazy = lit(b"abcbcde|a");
    lazy.push(m(4, 6));
    for level in LAZY {
        assert_eq!(tokens(data, level), lazy, "level {level}");
    }
}

#[test]
fn lazy_keeps_the_match_when_the_next_one_is_not_longer() {
    for level in LAZY {
        let mut want = lit(b"xyz");
        want.push(m(3, 3));
        want.extend(lit(b"qr"));
        assert_eq!(tokens(b"xyzxyzqr", level), want, "level {level}");
        assert_eq!(
            tokens(b"aaaaaa", level),
            [Token::Literal(b'a'), m(5, 1)],
            "level {level}"
        );
    }
}

#[test]
fn lazy_drops_short_far_matches() {
    // A 3-byte match more than 4096 back costs more than the literals.
    let mut data = b"abc".to_vec();
    data.extend((0..5000u32).map(|i| b'A' + (i % 26) as u8));
    data.extend(b"abc");
    for level in LAZY {
        assert!(
            !matches!(tokens(&data, level).last(), Some(Token::Match { .. })),
            "level {level}"
        );
    }
}
