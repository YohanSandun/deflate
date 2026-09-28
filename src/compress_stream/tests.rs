use super::*;
use crate::options::CompressionLevel;

// Deterministic pseudo-random bytes: incompressible, so one token per byte.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 7u32;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        })
        .collect()
}

#[test]
fn buffer_never_holds_more_than_a_window_and_a_chunk() {
    for level in [1, 6, 9] {
        let options = CompressionOptions::new().level(CompressionLevel::new(level));
        let mut stream = DeflateStream::new(Format::Raw, options);
        let limit = MAX_DISTANCE + stream.chunk + MAX_MATCH;
        let data = noise(3 * limit);

        let mut out = Vec::new();
        // One huge push, then small ones.
        stream.push(&data, &mut out);
        assert!(stream.buffer.len() <= limit, "level {level}: {}", stream.buffer.len());
        for piece in data.chunks(10_000) {
            stream.push(piece, &mut out);
            assert!(stream.buffer.len() <= limit, "level {level}: {}", stream.buffer.len());
        }
    }
}

#[test]
fn history_is_kept_between_chunks() {
    // The second chunk repeats the end of the first: matches must reach back.
    let options = CompressionOptions::new();
    let mut stream = DeflateStream::new(Format::Raw, options);
    let data = noise(CHUNK + MAX_MATCH);

    let mut out = Vec::new();
    stream.push(&data, &mut out);
    assert!(stream.start > 0, "the first chunk was compressed");
    let first = out.len();

    stream.push(&data[data.len() - 20_000..], &mut out);
    stream.finish(&mut out);
    let rest = out.len() - first;
    assert!(rest < 5_000, "{rest} bytes for 20 KB seen just before");
}

#[test]
fn reset_forgets_the_last_stream() {
    let mut stream = DeflateStream::new(Format::Zlib, CompressionOptions::new());
    let mut first = Vec::new();
    stream.push(b"some data some data", &mut first);
    stream.finish(&mut first);

    stream.reset();
    assert!(!stream.is_done());
    let mut second = Vec::new();
    stream.push(b"some data some data", &mut second);
    stream.finish(&mut second);
    assert_eq!(first, second);
}
