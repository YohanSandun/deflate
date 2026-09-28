use std::io::{self, Read, Write};

use rust_deflate::{
    CompressionLevel, CompressionOptions, DeflateEncoder, StreamCompressor, StreamDecompressor,
    Strategy, ZlibDecoder, ZlibEncoder, compress_with, compress_zlib_with, decompress,
    decompress_zlib,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> &'static [u8] {
        include_bytes!("data/dynamic_text.txt")
    }

    // Deterministic pseudo-random bytes.
    fn noise(len: usize) -> Vec<u8> {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    // About 2.5 MB of text, noise and zeros: several stream chunks.
    fn large() -> Vec<u8> {
        let mut data = Vec::new();
        for round in 0..8 {
            data.extend(text());
            data.extend(noise(50_000 + round * 1000));
            data.extend(include_bytes!("data/matches.raw"));
            data.extend(vec![0; 20_000]);
        }
        data
    }

    fn inputs() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("empty", Vec::new()),
            ("one byte", vec![0x42]),
            ("text", text().to_vec()),
            ("noise", noise(300_000)),
            ("zeros", vec![0; 1 << 20]),
            ("large", large()),
        ]
    }

    // Pushes `data` in `chunk`-byte pieces (with an empty push before each), then
    // finishes. Collects all output.
    fn push_all(mut stream: StreamCompressor, data: &[u8], chunk: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for piece in data.chunks(chunk) {
            stream.push(&[], &mut out);
            stream.push(piece, &mut out);
        }
        stream.finish(&mut out);
        out
    }

    // --- StreamCompressor ---------------------------------------------------------

    #[test]
    fn raw_streams_round_trip() {
        for (name, data) in inputs() {
            let out = push_all(StreamCompressor::deflate(), &data, 10_000);
            assert!(decompress(&out).unwrap() == data, "{name}");
        }
    }

    #[test]
    fn zlib_streams_round_trip() {
        for (name, data) in inputs() {
            let out = push_all(StreamCompressor::zlib(), &data, 10_000);
            assert!(decompress_zlib(&out).unwrap() == data, "{name}");
        }
    }

    #[test]
    fn every_level_and_strategy_round_trips() {
        let data = large();
        for level in 0..=9 {
            for strategy in [Strategy::Stored, Strategy::Fixed, Strategy::Dynamic] {
                let options = CompressionOptions::new()
                    .strategy(strategy)
                    .level(CompressionLevel::new(level));
                let out = push_all(StreamCompressor::zlib_with(options), &data, 100_000);
                assert!(
                    decompress_zlib(&out).unwrap() == data,
                    "level {level}, {strategy:?}"
                );
            }
        }
    }

    #[test]
    fn output_does_not_depend_on_how_input_is_split() {
        let data = large();
        let whole = push_all(StreamCompressor::deflate(), &data, data.len());
        for chunk in [1, 1000, 65_536, 300_000] {
            let data = if chunk == 1 { &data[..200_000] } else { &data[..] };
            let want = push_all(StreamCompressor::deflate(), data, data.len());
            assert!(push_all(StreamCompressor::deflate(), data, chunk) == want, "{chunk}");
        }
        assert!(decompress(&whole).unwrap() == data);
    }

    #[test]
    fn small_streams_match_one_shot_compression() {
        for level in [CompressionLevel::NONE, CompressionLevel::FAST, CompressionLevel::BEST] {
            for data in [&b""[..], b"hello hello hello hello", text()] {
                assert_eq!(
                    push_all(StreamCompressor::deflate_with(level), data, 777),
                    compress_with(data, level),
                    "{level:?}"
                );
                assert_eq!(
                    push_all(StreamCompressor::zlib_with(level), data, 777),
                    compress_zlib_with(data, level),
                    "{level:?}"
                );
            }
        }
    }

    #[test]
    fn large_streams_compress_about_as_well_as_one_shot() {
        let data = large();
        for level in [CompressionLevel::FAST, CompressionLevel::MEDIUM, CompressionLevel::BEST] {
            let stream = push_all(StreamCompressor::deflate_with(level), &data, 50_000).len();
            let one_shot = compress_with(&data, level).len();
            assert!(
                stream * 100 <= one_shot * 101,
                "{level:?}: stream {stream}, one-shot {one_shot}"
            );
        }
    }

    #[test]
    fn output_decodes_with_the_streaming_decoders() {
        let data = large();
        let compressed = push_all(StreamCompressor::zlib(), &data, 64_000);

        let mut out = Vec::new();
        ZlibDecoder::new(&compressed[..]).read_to_end(&mut out).unwrap();
        assert!(out == data);

        let mut stream = StreamDecompressor::zlib();
        let mut out = Vec::new();
        for piece in compressed.chunks(1000) {
            stream.push(piece, &mut out).unwrap();
        }
        stream.finish(&mut out).unwrap();
        assert!(out == data);
    }

    #[test]
    fn flush_makes_everything_so_far_decodable() {
        let data = large();
        let mut compressor = StreamCompressor::zlib();
        let mut decompressor = StreamDecompressor::zlib();
        let (mut compressed, mut received) = (Vec::new(), Vec::new());

        let mut sent = 0;
        for piece in data.chunks(123_457) {
            compressor.push(piece, &mut compressed);
            compressor.flush(&mut compressed);
            sent += piece.len();

            // A sync flush ends with an empty stored block.
            assert_eq!(compressed[compressed.len() - 4..], [0x00, 0x00, 0xFF, 0xFF]);
            decompressor.push(&compressed, &mut received).unwrap();
            compressed.clear();
            assert!(received == data[..sent], "after {sent} bytes");
        }

        compressor.finish(&mut compressed);
        decompressor.push(&compressed, &mut received).unwrap();
        decompressor.finish(&mut received).unwrap();
        assert!(received == data);
    }

    #[test]
    fn flush_with_nothing_new_still_decodes() {
        let mut compressor = StreamCompressor::deflate();
        let mut out = Vec::new();
        compressor.flush(&mut out);
        compressor.push(b"abc", &mut out);
        compressor.flush(&mut out);
        compressor.flush(&mut out);
        compressor.finish(&mut out);
        assert_eq!(decompress(&out).unwrap(), b"abc");
    }

    #[test]
    fn finish_is_idempotent() {
        let mut compressor = StreamCompressor::zlib();
        let mut out = Vec::new();
        compressor.push(b"data", &mut out);
        assert!(!compressor.is_done());
        assert!(compressor.finish(&mut out) > 0);
        assert!(compressor.is_done());
        assert_eq!(compressor.finish(&mut out), 0);
        assert_eq!(decompress_zlib(&out).unwrap(), b"data");
    }

    #[test]
    fn reset_starts_a_new_stream() {
        let mut compressor = StreamCompressor::zlib_with(CompressionLevel::BEST);
        let mut first = Vec::new();
        compressor.push(text(), &mut first);
        compressor.finish(&mut first);

        compressor.reset();
        let mut second = Vec::new();
        compressor.push(b"another stream", &mut second);
        compressor.finish(&mut second);

        assert!(decompress_zlib(&first).unwrap() == text());
        assert_eq!(decompress_zlib(&second).unwrap(), b"another stream");
    }

    #[test]
    #[should_panic(expected = "finished")]
    fn push_after_finish_panics() {
        let mut compressor = StreamCompressor::deflate();
        let mut out = Vec::new();
        compressor.finish(&mut out);
        compressor.push(b"late", &mut out);
    }

    // --- DeflateEncoder and ZlibEncoder -------------------------------------------

    #[test]
    fn encoders_round_trip() {
        for (name, data) in inputs() {
            let mut encoder = DeflateEncoder::new(Vec::new());
            for piece in data.chunks(7_777) {
                encoder.write_all(piece).unwrap();
            }
            assert!(decompress(&encoder.finish().unwrap()).unwrap() == data, "{name}");

            let mut encoder = ZlibEncoder::new(Vec::new());
            encoder.write_all(&data).unwrap();
            assert!(decompress_zlib(&encoder.finish().unwrap()).unwrap() == data, "{name}");
        }
    }

    #[test]
    fn encoders_match_the_stream_compressor() {
        let data = large();
        for level in [CompressionLevel::NONE, CompressionLevel::MEDIUM, CompressionLevel::BEST] {
            let mut encoder = ZlibEncoder::with_options(Vec::new(), level);
            encoder.write_all(&data).unwrap();
            let want = push_all(StreamCompressor::zlib_with(level), &data, 50_000);
            assert!(encoder.finish().unwrap() == want, "{level:?}");

            let mut encoder = DeflateEncoder::with_options(Vec::new(), level);
            encoder.write_all(&data).unwrap();
            let want = push_all(StreamCompressor::deflate_with(level), &data, 50_000);
            assert!(encoder.finish().unwrap() == want, "{level:?}");
        }
    }

    #[test]
    fn copying_a_reader_into_an_encoder() {
        let data = large();
        let mut encoder = ZlibEncoder::new(Vec::new());
        io::copy(&mut &data[..], &mut encoder).unwrap();
        assert!(decompress_zlib(&encoder.finish().unwrap()).unwrap() == data);
    }

    #[test]
    fn dropping_an_encoder_finishes_the_stream() {
        let mut out = Vec::new();
        {
            let mut encoder = ZlibEncoder::new(&mut out);
            encoder.write_all(text()).unwrap();
        }
        assert!(decompress_zlib(&out).unwrap() == text());
    }

    #[test]
    fn encoder_flush_writes_everything_so_far() {
        let mut encoder = DeflateEncoder::new(Vec::new());
        encoder.write_all(b"first part, ").unwrap();
        encoder.flush().unwrap();

        let mut decompressor = StreamDecompressor::deflate();
        let mut received = Vec::new();
        decompressor.push(encoder.get_ref(), &mut received).unwrap();
        assert_eq!(received, b"first part, ");

        encoder.write_all(b"second part").unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(decompress(&compressed).unwrap(), b"first part, second part");
    }

    #[test]
    fn get_mut_reaches_the_writer() {
        let mut encoder = DeflateEncoder::new(Vec::new());
        encoder.write_all(b"abc").unwrap();
        encoder.flush().unwrap();
        assert!(!encoder.get_mut().is_empty());
    }

    // A writer that accepts `room` bytes, then fails.
    #[derive(Debug)]
    struct Failing {
        room: usize,
    }

    impl Write for Failing {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.room == 0 {
                return Err(io::Error::other("disk full"));
            }
            let n = buf.len().min(self.room);
            self.room -= n;
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn writer_errors_are_passed_through() {
        let mut encoder = ZlibEncoder::new(Failing { room: 10 });
        let result = encoder
            .write_all(&noise(2_000_000))
            .and_then(|()| encoder.flush());
        assert_eq!(result.unwrap_err().to_string(), "disk full");

        let encoder = DeflateEncoder::new(Failing { room: 0 });
        assert_eq!(encoder.finish().unwrap_err().to_string(), "disk full");
    }
}
