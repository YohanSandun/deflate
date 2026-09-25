use super::{Format, InflateStream, State};

// Bits of input received but not yet decoded.
fn undecoded_bits(stream: &InflateStream) -> usize {
    stream.input.len() * 8 - stream.bit_pos
}

// After every push, the decoder must be at most one incomplete unit behind the input:
// inside a Huffman block, less than one symbol (48 bits); inside a stored block,
// nothing (every available byte is copied); otherwise part of a header or trailer.
fn check_lag(format: Format, compressed: &[u8]) {
    let mut stream = InflateStream::new(format);
    let mut out = Vec::new();

    for (i, &byte) in compressed.iter().enumerate() {
        stream.push(&[byte], &mut out).unwrap();
        let undecoded = undecoded_bits(&stream);

        match stream.state {
            State::Huffman { .. } => assert!(
                undecoded < 48,
                "byte {i}: {undecoded} bits undecoded in a Huffman block"
            ),
            State::Stored { .. } => {
                assert_eq!(undecoded / 8, 0, "byte {i}: stored bytes not copied")
            }
            // A dynamic header is at most ~4500 bits; the zlib header 16, trailer 32 (+7 padding).
            State::BlockHeader | State::ZlibHeader | State::ZlibTrailer => {
                assert!(
                    undecoded < 4600,
                    "byte {i}: {undecoded} bits waiting for a header"
                )
            }
            State::Done => {}
            State::Failed(error) => panic!("byte {i}: {error}"),
        }
    }
    assert!(stream.is_done());
}

#[test]
fn output_lags_input_by_less_than_one_symbol() {
    check_lag(
        Format::Zlib,
        include_bytes!("../../tests/data/zlib_text_l9.zz"),
    );
    check_lag(
        Format::Zlib,
        include_bytes!("../../tests/data/zlib_text_fixed.zz"),
    );
    check_lag(
        Format::Zlib,
        include_bytes!("../../tests/data/zlib_text_stored.zz"),
    );
    check_lag(
        Format::Raw,
        include_bytes!("../../tests/data/matches.deflate"),
    );
}
