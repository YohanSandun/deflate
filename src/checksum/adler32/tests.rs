use super::{Adler32, compute_adler32};

#[test]
fn empty_input_is_one() {
    assert_eq!(compute_adler32(&[]), 1);
}

#[test]
fn known_values() {
    // Reference values from zlib.adler32.
    assert_eq!(compute_adler32(b"a"), 0x0062_0062);
    assert_eq!(compute_adler32(b"abc"), 0x024D_0127);
    assert_eq!(compute_adler32(b"Wikipedia"), 0x11E6_0398);
    assert_eq!(compute_adler32(b"hello hello hello hello"), 0x6803_08B1);
}

#[test]
fn long_input_wraps_the_modulus() {
    // 1 MB of 0xFF pushes both sums far past 65521 many times over.
    // zlib.adler32(b"\xff" * (1 << 20)) == 0x8E88_EF11
    assert_eq!(compute_adler32(&vec![0xFF; 1 << 20]), 0x8E88_EF11);
}

// The plain per-byte definition, reducing after every byte.
fn reference_adler32(data: &[u8]) -> u32 {
    let (mut s1, mut s2) = (1u32, 0u32);
    for &byte in data {
        s1 = (s1 + u32::from(byte)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

#[test]
fn matches_reference_around_block_and_chunk_boundaries() {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let random: Vec<u8> = (0..20_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let ones = vec![0xFF; 20_000]; // largest sums, so the most overflow-prone input

    // Every length up to 64 (all 8-byte block remainders), and both sides of the
    // 5552-byte reduction points.
    let lengths = (0..=64).chain([5551, 5552, 5553, 5560, 11103, 11104, 11105, 16656, 20_000]);

    for len in lengths {
        assert_eq!(
            compute_adler32(&random[..len]),
            reference_adler32(&random[..len]),
            "random, len {len}"
        );
        assert_eq!(
            compute_adler32(&ones[..len]),
            reference_adler32(&ones[..len]),
            "0xFF, len {len}"
        );
    }
}

#[test]
fn incremental_updates_match_one_shot_for_any_split() {
    let data: Vec<u8> = (0..30_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let expected = compute_adler32(&data);

    // Splits at every block remainder, around the 5552-byte reduction points, and
    // many small pieces.
    for split in (0..=20).chain([5551, 5552, 5553, 11104, 29_999]) {
        let mut adler = Adler32::new();
        adler.update(&data[..split]);
        adler.update(&data[split..]);
        assert_eq!(adler.finish(), expected, "split at {split}");
    }

    let mut adler = Adler32::new();
    for piece in data.chunks(7) {
        adler.update(piece);
    }
    assert_eq!(adler.finish(), expected, "7-byte pieces");
}
