use rust_deflate::{Error, StreamDecompressor, decompress};

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_RAW: [u8; 10] = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
    const HELLO_ZLIB: [u8; 16] = [
        0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08,
        0xB1,
    ];

    fn text() -> &'static [u8] {
        include_bytes!("data/dynamic_text.txt")
    }

    // Pushes `data` in `chunk`-byte pieces (with an empty push before each), then
    // finishes. Collects all output.
    fn push_all(
        mut stream: StreamDecompressor,
        data: &[u8],
        chunk: usize,
    ) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        for piece in data.chunks(chunk) {
            stream.push(&[], &mut out)?;
            stream.push(piece, &mut out)?;
        }
        stream.finish(&mut out)?;
        Ok(out)
    }

    // --- output --------------------------------------------------------------------

    #[test]
    fn raw_streams_decode_in_any_chunk_size() {
        let fixtures: [&[u8]; 4] = [
            include_bytes!("data/dynamic_text.deflate"),
            include_bytes!("data/fixed_text.deflate"),
            include_bytes!("data/matches.deflate"),
            include_bytes!("data/dynamic_multi.deflate"),
        ];

        for (i, compressed) in fixtures.into_iter().enumerate() {
            let expected = decompress(compressed).unwrap();
            for chunk in [1, 2, 7, 1000, 1 << 20] {
                let output = push_all(StreamDecompressor::deflate(), compressed, chunk).unwrap();
                assert!(output == expected, "fixture {i}, chunk {chunk}");
            }
        }
    }

    #[test]
    fn zlib_streams_decode_in_any_chunk_size() {
        let fixtures: [&[u8]; 4] = [
            include_bytes!("data/zlib_text_l9.zz"),
            include_bytes!("data/zlib_text_l1_w9.zz"),
            include_bytes!("data/zlib_text_stored.zz"),
            include_bytes!("data/zlib_text_fixed.zz"),
        ];

        for (i, compressed) in fixtures.into_iter().enumerate() {
            for chunk in [1, 3, 4096, 1 << 20] {
                let output = push_all(StreamDecompressor::zlib(), compressed, chunk).unwrap();
                assert!(output == text(), "fixture {i}, chunk {chunk}");
            }
        }
    }

    #[test]
    fn output_can_be_consumed_and_cleared_after_each_push() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let mut stream = StreamDecompressor::zlib();
        let mut out = Vec::new();
        let mut collected = Vec::new();

        for piece in compressed.chunks(1000) {
            let written = stream.push(piece, &mut out).unwrap();
            assert_eq!(written, out.len());
            collected.extend_from_slice(&out);
            out.clear();
        }
        stream.finish(&mut out).unwrap();
        collected.extend_from_slice(&out);

        assert!(collected == text());
    }

    #[test]
    fn push_appends_after_existing_output() {
        let mut stream = StreamDecompressor::deflate();
        let mut out = b"prefix:".to_vec();

        stream.push(&HELLO_RAW, &mut out).unwrap();
        stream.finish(&mut out).unwrap();

        assert_eq!(out, b"prefix:hello hello hello hello");
    }

    #[test]
    fn long_stream_with_small_pushes() {
        // A zero run far longer than the window: 1 byte then 40 000 matches of 258.
        let mut bits: Vec<u32> = vec![1, 1, 0];
        let mut code = |value: u32, length: u32| {
            for i in (0..length).rev() {
                bits.push((value >> i) & 1);
            }
        };
        code(0x30, 8);
        for _ in 0..40_000 {
            code(0b1100_0101, 8);
            code(0, 5);
        }
        code(0, 7);
        let mut compressed = vec![0u8; bits.len().div_ceil(8)];
        for (i, bit) in bits.iter().enumerate() {
            compressed[i / 8] |= (*bit as u8) << (i % 8);
        }

        let mut stream = StreamDecompressor::deflate();
        let mut out = Vec::new();
        let mut total = 0;
        for piece in compressed.chunks(4096) {
            total += stream.push(piece, &mut out).unwrap();
            assert!(out.iter().all(|&b| b == 0));
            out.clear();
        }
        total += stream.finish(&mut out).unwrap();

        assert_eq!(total, 1 + 40_000 * 258);
    }

    // --- end of stream -------------------------------------------------------------

    #[test]
    fn stream_can_finish_during_a_push() {
        let mut stream = StreamDecompressor::zlib();
        let mut out = Vec::new();

        assert!(!stream.is_done());
        stream.push(&HELLO_ZLIB, &mut out).unwrap();
        // The whole stream was there, but a push can't know no more data follows the
        // last block's end until the zlib trailer is read; here it is.
        assert!(stream.is_done());
        assert_eq!(out, b"hello hello hello hello");

        assert_eq!(stream.finish(&mut out), Ok(0));
        assert_eq!(stream.finish(&mut out), Ok(0));
    }

    #[test]
    fn input_after_the_end_is_ignored() {
        let mut stream = StreamDecompressor::zlib();
        let mut out = Vec::new();

        let mut data = HELLO_ZLIB.to_vec();
        data.extend_from_slice(b"trailing garbage");

        stream.push(&data, &mut out).unwrap();
        assert_eq!(stream.push(b"more garbage", &mut out), Ok(0));
        stream.finish(&mut out).unwrap();

        assert_eq!(out, b"hello hello hello hello");
    }

    // --- errors --------------------------------------------------------------------

    #[test]
    fn finish_on_an_incomplete_stream_fails() {
        let compressed = include_bytes!("data/dynamic_text.deflate");

        for cut in [0, 1, 20, compressed.len() / 2, compressed.len() - 1] {
            let mut stream = StreamDecompressor::deflate();
            let mut out = Vec::new();

            stream.push(&compressed[..cut], &mut out).unwrap();
            let before = out.len();

            assert_eq!(
                stream.finish(&mut out),
                Err(Error::UnexpectedEndOfInput),
                "cut {cut}"
            );
            assert_eq!(
                out.len(),
                before,
                "cut {cut}: finish must not leave partial output"
            );
        }
    }

    #[test]
    fn checksum_mismatch_is_reported_by_finish() {
        let mut data = include_bytes!("data/zlib_text_l9.zz").to_vec();
        let last = data.len() - 1;
        data[last] ^= 0x01;

        let mut stream = StreamDecompressor::zlib();
        let mut out = Vec::new();

        // The checksum is the very end, so every push up to it succeeds.
        let body_end = data.len() - 4;
        stream.push(&data[..body_end], &mut out).unwrap();
        assert_eq!(
            stream.push(&data[body_end..], &mut out),
            Err(Error::ChecksumMismatch)
        );
    }

    #[test]
    fn errors_persist_until_reset() {
        let mut stream = StreamDecompressor::deflate();
        let mut out = Vec::new();

        stream.push(&[0x07], &mut out).unwrap(); // not enough input to decide yet
        assert_eq!(stream.finish(&mut out), Err(Error::InvalidBlockType));
        assert_eq!(
            stream.push(&HELLO_RAW, &mut out),
            Err(Error::InvalidBlockType)
        );
        assert_eq!(stream.finish(&mut out), Err(Error::InvalidBlockType));
        assert!(out.is_empty());

        stream.reset();
        stream.push(&HELLO_RAW, &mut out).unwrap();
        stream.finish(&mut out).unwrap();
        assert_eq!(out, b"hello hello hello hello");
    }

    #[test]
    fn corrupt_data_fails_during_push() {
        // A dynamic block whose header is corrupt, with plenty of data after it, so
        // the error comes from push, not finish.
        let mut data = include_bytes!("data/dynamic_text.deflate").to_vec();
        data[1] = 0xFF;
        data[2] = 0xFF;

        let mut stream = StreamDecompressor::deflate();
        let mut out = Vec::new();

        assert!(stream.push(&data, &mut out).is_err());
        assert!(out.is_empty());
    }

    // --- reuse ---------------------------------------------------------------------

    #[test]
    fn reset_reuses_the_decompressor_for_new_streams() {
        let mut stream = StreamDecompressor::zlib();
        let level9 = include_bytes!("data/zlib_text_l9.zz");

        for _ in 0..3 {
            let mut out = Vec::new();
            stream.push(level9, &mut out).unwrap();
            stream.finish(&mut out).unwrap();
            assert!(out == text());

            stream.reset();
            let mut out = Vec::new();
            stream.push(&HELLO_ZLIB, &mut out).unwrap();
            stream.finish(&mut out).unwrap();
            assert_eq!(out, b"hello hello hello hello");
            stream.reset();
        }
    }

    #[test]
    fn a_reset_stream_cannot_reach_back_into_the_previous_one() {
        let mut stream = StreamDecompressor::deflate();
        let mut out = Vec::new();
        stream.push(&HELLO_RAW, &mut out).unwrap();
        stream.finish(&mut out).unwrap();

        stream.reset();
        // Hand-encoded: a copy at distance 1 before this stream has written anything.
        // The previous stream's output must not count as history.
        stream.push(&[0x03, 0x02, 0x00], &mut out).unwrap();
        assert_eq!(stream.finish(&mut out), Err(Error::DistanceTooFarBack));
    }

    #[test]
    fn debug_shows_progress() {
        let mut stream = StreamDecompressor::deflate();
        assert_eq!(
            format!("{stream:?}"),
            "StreamDecompressor { done: false, .. }"
        );

        stream.push(&HELLO_RAW, &mut Vec::new()).unwrap();
        stream.finish(&mut Vec::new()).unwrap();
        assert_eq!(
            format!("{stream:?}"),
            "StreamDecompressor { done: true, .. }"
        );
    }

    // --- low latency ---------------------------------------------------------------

    #[test]
    fn sync_flushed_messages_are_available_immediately() {
        // Three messages compressed by zlib with Z_SYNC_FLUSH after each (as WebSocket
        // compression and streamed HTTP do), then the end of the stream. Each part must
        // decompress completely as soon as it's pushed, without waiting for finish.
        let parts: [&[u8]; 4] = [
            &[
                0x78, 0x9C, 0x4A, 0xCB, 0x2C, 0x2A, 0x2E, 0x51, 0xC8, 0x4D, 0x2D, 0x2E, 0x4E, 0x4C,
                0x4F, 0xB5, 0x52, 0xC8, 0x48, 0xCD, 0xC9, 0xC9, 0x47, 0x26, 0x01, 0x00, 0x00, 0x00,
                0xFF, 0xFF,
            ],
            &[
                0x2A, 0x4E, 0x4D, 0xCE, 0xCF, 0x4B, 0x81, 0x29, 0xD0, 0x51, 0x48, 0x54, 0x48, 0xCA,
                0x2C, 0x51, 0xC8, 0xC9, 0xCF, 0x4B, 0x4F, 0x2D, 0x82, 0x29, 0x4F, 0x4C, 0x4F, 0xCC,
                0xCC, 0x53, 0x48, 0x04, 0x2A, 0x43, 0x63, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF,
            ],
            &[
                0x2A, 0xC9, 0xC8, 0x2C, 0x4A, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF,
            ],
            &[0x03, 0x00, 0xF7, 0xB1, 0x23, 0xC7],
        ];
        let messages: [&[u8]; 4] = [
            b"first message: hello hello hello",
            b"second message, a bit longer: hello again and again and again",
            b"third",
            b"",
        ];

        let mut stream = StreamDecompressor::zlib();
        for (part, message) in parts.iter().zip(messages) {
            let mut out = Vec::new();
            stream.push(part, &mut out).unwrap();
            assert_eq!(out, message);
        }

        // The last part holds the final block and the checksum, so the stream is
        // already complete.
        assert!(stream.is_done());
        assert_eq!(stream.finish(&mut Vec::new()), Ok(0));
    }

    #[test]
    fn raw_stream_is_done_as_soon_as_its_last_block_ends() {
        let mut stream = StreamDecompressor::deflate();
        let mut out = Vec::new();

        stream.push(&HELLO_RAW, &mut out).unwrap();

        assert!(stream.is_done());
        assert_eq!(out, b"hello hello hello hello");
    }

    #[test]
    fn one_byte_pushes_decode_correctly() {
        // How far behind the input the output can lag is checked exactly in the crate's
        // unit tests (src/stream/tests.rs); here, just that 1-byte pushes work.
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let mut stream = StreamDecompressor::zlib();
        let mut out = Vec::new();

        for &byte in compressed.iter() {
            stream.push(&[byte], &mut out).unwrap();
        }

        assert!(out == text());
        assert!(stream.is_done());
    }
}
