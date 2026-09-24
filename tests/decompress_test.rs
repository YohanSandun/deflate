use rust_deflate::{Decompressor, Error, decompress};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflate_stored_block() {
        let data = [
            0x01, 0x26, 0x00, 0xD9, 0xFF, 0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x2C, 0x20, 0x74, 0x68,
            0x69, 0x73, 0x20, 0x69, 0x73, 0x20, 0x61, 0x20, 0x73, 0x74, 0x6F, 0x72, 0x65, 0x64,
            0x20, 0x44, 0x45, 0x46, 0x4C, 0x41, 0x54, 0x45, 0x20, 0x62, 0x6C, 0x6F, 0x63, 0x6B,
            0x21,
        ];

        let inflated = decompress(&data);

        assert_eq!(inflated.unwrap(), b"Hello, this is a stored DEFLATE block!");
    }

    // Streams below were produced by zlib with the Z_FIXED strategy (raw deflate),
    // unless noted as hand-encoded.

    #[test]
    fn inflate_fixed_block_with_back_references() {
        let data = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];

        let inflated = decompress(&data);

        assert_eq!(inflated.unwrap(), b"hello hello hello hello");
    }

    #[test]
    fn inflate_fixed_block_with_overlapping_copies() {
        // Distance 1 and 2 copies that are longer than their distance.
        let data = [
            0x4B, 0x4C, 0x1C, 0x05, 0x44, 0x83, 0xA4, 0x51, 0x38, 0x98, 0x20, 0x00,
        ];

        let inflated = decompress(&data).unwrap();

        let mut expected = vec![b'a'; 300];
        expected.extend(b"ab".repeat(200));
        assert_eq!(inflated, expected);
    }

    #[test]
    fn inflate_multiple_fixed_blocks() {
        // Three fixed blocks, BFINAL set only on the last; later blocks copy from earlier ones.
        let data = [
            0x4A, 0xCB, 0x2C, 0x2A, 0x2E, 0x51, 0x48, 0xCA, 0xC9, 0x4F, 0xCE, 0xD6, 0x51, 0x48,
            0x43, 0x70, 0xF4, 0x14, 0x00, 0x2A, 0x4E, 0x4D, 0xCE, 0xCF, 0x4B, 0x81, 0x49, 0x21,
            0xF3, 0xF4, 0x14, 0x00, 0x2B, 0xC9, 0xC8, 0x2C, 0x82, 0x72, 0x14, 0x8A, 0x52, 0xD3,
            0x52, 0x8B, 0x8A, 0x15, 0x4A, 0xF2, 0x91, 0xB5, 0x03, 0x00,
        ];

        let inflated = decompress(&data);

        assert_eq!(
            inflated.unwrap(),
            b"first block, first block. second block, second block. third block refers to first block"
        );
    }

    #[test]
    fn inflate_large_fixed_stream() {
        // ~53 KB of text, so copies use distances across the whole 32 KB window.
        let data = include_bytes!("data/fixed_text.deflate");
        let expected = include_bytes!("data/fixed_text.txt");

        let inflated = decompress(data).unwrap();

        assert_eq!(inflated.len(), expected.len());
        assert!(
            inflated == expected,
            "inflated output differs from expected text"
        );
    }

    #[test]
    fn inflate_rejects_distance_with_empty_output() {
        // Hand-encoded: copy of length 3 at distance 1 before any output.
        let data = [0x03, 0x02, 0x00];

        let inflated = decompress(&data);

        assert_eq!(inflated, Err(Error::DistanceTooFarBack));
    }

    #[test]
    fn inflate_rejects_distance_one_past_output_start() {
        // Hand-encoded: literal 'a', then copy of length 3 at distance 2.
        let data = [0x4B, 0x04, 0x42, 0x00];

        let inflated = decompress(&data);

        assert_eq!(inflated, Err(Error::DistanceTooFarBack));
    }

    #[test]
    fn inflate_accepts_distance_equal_to_output_length() {
        // Hand-encoded: literal 'a', then copy of length 3 at distance 1.
        let data = [0x4B, 0x04, 0x02, 0x00];

        let inflated = decompress(&data);

        assert_eq!(inflated.unwrap(), b"aaaa");
    }

    #[test]
    fn inflate_fixed_block_with_longer_overlapping_copies() {
        // Copies at distances 3, 5 and 10 that are longer than their distance.
        let data = [
            0x4B, 0x4C, 0x4A, 0x4E, 0x1C, 0x45, 0xC4, 0xA1, 0x8A, 0xCA, 0xAA, 0xD2, 0xB2, 0x51,
            0x82, 0x28, 0xC2, 0xC0, 0xD0, 0xC8, 0xD8, 0xC4, 0xD4, 0xCC, 0xDC, 0xC2, 0x72, 0x94,
            0x45, 0x88, 0x05, 0x00,
        ];

        let inflated = decompress(&data).unwrap();

        let mut expected = b"abc".repeat(100);
        expected.extend(b"xyzuv".repeat(60));
        expected.extend(b"0123456789".repeat(30));
        assert_eq!(inflated, expected);
    }

    #[test]
    fn inflate_fixed_then_empty_stored_then_fixed() {
        // zlib sync flush: a fixed block ending mid-byte, an empty stored block, then a final fixed block.
        let data = [
            0x4A, 0x4A, 0x4D, 0xCB, 0x2F, 0x4A, 0x55, 0x28, 0xC9, 0x48, 0x55, 0x28, 0xAE, 0xCC,
            0x4B, 0x56, 0x48, 0xCB, 0x29, 0x2D, 0xCE, 0xD0, 0x51, 0x48, 0xC2, 0x26, 0xAC, 0xA7,
            0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x4B, 0x4C, 0x2B, 0x49, 0x2D, 0xC2, 0x50, 0x8C,
            0x4D, 0x54, 0x0F, 0x00,
        ];

        let inflated = decompress(&data);

        assert_eq!(
            inflated.unwrap(),
            b"before the sync flush, before the sync flush. after the sync flush, after the sync flush."
        );
    }

    #[test]
    fn inflate_stored_then_fixed() {
        // Hand-built: non-final stored block "12345", then the final fixed "hello" block.
        let data = [
            0x00, 0x05, 0x00, 0xFA, 0xFF, b'1', b'2', b'3', b'4', b'5', 0xCB, 0x48, 0xCD, 0xC9,
            0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01,
        ];

        let inflated = decompress(&data);

        assert_eq!(inflated.unwrap(), b"12345hello hello hello hello");
    }

    #[test]
    fn inflate_rejects_truncated_stored_block() {
        // LEN says 5 bytes but only 3 follow.
        let data = [0x01, 0x05, 0x00, 0xFA, 0xFF, b'1', b'2', b'3'];

        let inflated = decompress(&data);

        assert_eq!(inflated, Err(Error::UnexpectedEndOfInput));
    }

    // Dynamic Huffman blocks. zlib streams use the default strategy; the small
    // edge cases are hand-encoded (and checked against zlib when they are valid).

    #[test]
    fn inflate_dynamic_block_from_zlib() {
        // ~110 KB of text in dynamic blocks.
        let data = include_bytes!("data/dynamic_text.deflate");
        let expected = include_bytes!("data/dynamic_text.txt");

        let inflated = decompress(data).unwrap();

        assert_eq!(inflated.len(), expected.len());
        assert!(
            inflated == expected,
            "inflated output differs from expected text"
        );
    }

    #[test]
    fn inflate_multiple_dynamic_blocks_from_zlib() {
        // Three dynamic blocks with different code tables: text, all 256 byte values, more text.
        let data = include_bytes!("data/dynamic_multi.deflate");
        let text = include_bytes!("data/dynamic_text.txt");

        let inflated = decompress(data).unwrap();

        let mut expected = text[..3000].to_vec();
        for _ in 0..4 {
            expected.extend(0..=255u8);
        }
        expected.extend(&text[3000..9000]);
        assert!(
            inflated == expected,
            "inflated output differs from expected data"
        );
    }

    #[test]
    fn inflate_dynamic_repeat_previous_crosses_into_distance_lengths() {
        // Code 16 right after the last literal/length length repeats it into the
        // distance lengths. RFC 1951 treats both length lists as one sequence.
        let data = [
            0x0D, 0x83, 0x05, 0x01, 0x00, 0x00, 0x00, 0x40, 0xB6, 0xF2, 0x7F, 0x04, 0x85, 0x1B,
        ];

        let inflated = decompress(&data);

        assert_eq!(inflated.unwrap(), b"ababa");
    }

    #[test]
    fn inflate_dynamic_zero_run_crosses_into_distance_lengths() {
        // Code 17 zero run that starts in the literal/length lengths and ends in the distance lengths.
        let data = [
            0x15, 0xC3, 0x21, 0x01, 0x00, 0x00, 0x00, 0x80, 0xA0, 0xAD, 0xEA, 0xFF, 0x0F, 0x26,
            0xB0, 0x01,
        ];

        let inflated = decompress(&data);

        assert_eq!(inflated.unwrap(), b"abc");
    }

    #[test]
    fn inflate_dynamic_rejects_repeat_previous_as_first_code() {
        // Code 16 with no previous length to repeat.
        let data = [0x05, 0x80, 0x03, 0x00, 0x00, 0x00, 0x00, 0x40, 0x02];

        assert_eq!(decompress(&data), Err(Error::RepeatWithoutPreviousLength));
    }

    #[test]
    fn inflate_dynamic_rejects_repeat_past_end_of_lengths() {
        // Two 138-zero runs for only 258 lengths.
        let data = [
            0x05, 0x80, 0x81, 0x00, 0x00, 0x00, 0x00, 0x40, 0xFE, 0xFF, 0x01,
        ];

        assert_eq!(decompress(&data), Err(Error::RepeatPastEnd));
    }

    #[test]
    fn inflate_dynamic_rejects_too_many_literal_length_codes() {
        // HLIT = 30 -> 287 codes; the maximum is 286.
        let data = [
            0xF5, 0x80, 0x81, 0x00, 0x00, 0x00, 0x00, 0x40, 0xFE, 0xFF, 0x03, 0x00,
        ];

        assert_eq!(decompress(&data), Err(Error::TooManyCodes));
    }

    #[test]
    fn inflate_dynamic_rejects_too_many_distance_codes() {
        // HDIST = 30 -> 31 codes; the maximum is 30.
        let data = [
            0x05, 0x9E, 0x81, 0x00, 0x00, 0x00, 0x00, 0x40, 0xFE, 0xFF, 0x07, 0x00,
        ];

        assert_eq!(decompress(&data), Err(Error::TooManyCodes));
    }

    #[test]
    fn inflate_dynamic_rejects_over_subscribed_code_length_code() {
        // Three code-length symbols of length 1.
        let data = [0x05, 0x80, 0x81, 0x04, 0x00, 0x00, 0x00, 0x40, 0x00];

        let inflated = decompress(&data);

        assert_eq!(inflated, Err(Error::OverSubscribedCode));
    }

    #[test]
    fn inflate_dynamic_rejects_truncated_header() {
        let data = include_bytes!("data/dynamic_text.deflate");

        assert_eq!(decompress(&data[..20]), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn inflate_dynamic_rejects_missing_end_of_block_code() {
        // Hand-encoded: literals 'a' and 'b' have codes, symbol 256 does not.
        let data = [
            0x05, 0xC0, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x90, 0x56, 0xFE, 0x27, 0x00,
        ];

        assert_eq!(decompress(&data), Err(Error::MissingEndOfBlockCode));
    }

    #[test]
    fn inflate_matches_at_every_short_distance() {
        // ~300 KB built from copies at distances 1..40 (plus far ones) with lengths
        // 3..258, mostly not multiples of 8, compressed by zlib at level 9. Covers the
        // 8-byte chunked copy, the overlapping copy, and output growing past 64 KB.
        let data = include_bytes!("data/matches.deflate");
        let expected = include_bytes!("data/matches.raw");

        let inflated = decompress(data).unwrap();

        assert_eq!(inflated.len(), expected.len());
        assert!(
            inflated == expected,
            "inflated output differs from expected data"
        );
    }

    #[test]
    fn inflate_rejects_invalid_length_symbol() {
        // Hand-encoded fixed block: literal 'a', then length symbol 286, which has a
        // fixed code but no meaning.
        let data = [0x4B, 0x1C, 0x03, 0x00];

        assert_eq!(decompress(&data), Err(Error::InvalidLengthSymbol));
    }

    #[test]
    fn inflate_rejects_invalid_distance_symbol() {
        // Hand-encoded fixed block: literal 'a', then a length with distance symbol 30.
        let data = [0x4B, 0x04, 0x3E, 0x00];

        assert_eq!(decompress(&data), Err(Error::InvalidDistanceSymbol));
    }

    #[test]
    fn inflate_output_has_no_trailing_slack() {
        // The decoder writes into a zero-filled buffer with spare room; none of it may
        // leak into the result, even across several blocks.
        let data = include_bytes!("data/dynamic_multi.deflate");

        let inflated = decompress(data).unwrap();

        assert_eq!(inflated.len(), 3000 + 4 * 256 + 6000);
    }

    #[test]
    fn inflate_truncated_inside_a_match_fails() {
        let data = include_bytes!("data/matches.deflate");

        for cut in [data.len() / 3, data.len() / 2, data.len() - 1] {
            assert!(
                decompress(&data[..cut]).is_err(),
                "cut at {cut} should fail"
            );
        }
    }

    // Streams covering every block type, with different dynamic tables.
    fn mixed_streams() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
        let mut multi_expected = include_bytes!("data/dynamic_text.txt")[..3000].to_vec();
        for _ in 0..4 {
            multi_expected.extend(0..=255u8);
        }
        multi_expected.extend(&include_bytes!("data/dynamic_text.txt")[3000..9000]);

        vec![
            (
                "dynamic text",
                include_bytes!("data/dynamic_text.deflate").to_vec(),
                include_bytes!("data/dynamic_text.txt").to_vec(),
            ),
            (
                "fixed text",
                include_bytes!("data/fixed_text.deflate").to_vec(),
                include_bytes!("data/fixed_text.txt").to_vec(),
            ),
            (
                "matches",
                include_bytes!("data/matches.deflate").to_vec(),
                include_bytes!("data/matches.raw").to_vec(),
            ),
            (
                "three dynamic blocks",
                include_bytes!("data/dynamic_multi.deflate").to_vec(),
                multi_expected,
            ),
            (
                "stored then fixed",
                vec![
                    0x00, 0x05, 0x00, 0xFA, 0xFF, b'1', b'2', b'3', b'4', b'5', 0xCB, 0x48, 0xCD,
                    0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01,
                ],
                b"12345hello hello hello hello".to_vec(),
            ),
            (
                "empty stored",
                vec![0x01, 0x00, 0x00, 0xFF, 0xFF],
                Vec::new(),
            ),
        ]
    }

    #[test]
    fn decompressor_reused_across_streams() {
        let streams = mixed_streams();
        let mut decompressor = Decompressor::new();

        // Every stream after every other one, so each sees tables left by a different stream.
        for _ in 0..2 {
            for (name, compressed, expected) in &streams {
                let output = decompressor.decompress(compressed).unwrap();
                assert!(output == *expected, "{name}: wrong output");
            }
            for (name, compressed, expected) in streams.iter().rev() {
                let output = decompressor.decompress(compressed).unwrap();
                assert!(output == *expected, "{name}: wrong output (reverse order)");
            }
        }
    }

    #[test]
    fn decompressor_works_after_errors() {
        let streams = mixed_streams();
        let dynamic = &streams[0].1;
        let mut decompressor = Decompressor::new();

        // Fail at different points: bad block type, inside a dynamic header (tables
        // half rebuilt), and in the middle of the data.
        let failures: [&[u8]; 4] = [
            &[0x07],
            &dynamic[..3],
            &dynamic[..20],
            &dynamic[..dynamic.len() / 2],
        ];

        for failing in failures {
            assert!(decompressor.decompress(failing).is_err());

            for (name, compressed, expected) in &streams {
                let output = decompressor.decompress(compressed).unwrap();
                assert!(output == *expected, "{name}: wrong output after an error");
            }
        }
    }

    #[test]
    fn decompressor_matches_decompress_function() {
        let mut decompressor = Decompressor::default();

        for (name, compressed, _) in mixed_streams() {
            assert_eq!(
                decompressor.decompress(&compressed),
                decompress(&compressed),
                "{name}"
            );
        }

        for bad in [&[0x07][..], &[], &[0x4B, 0x04, 0x42, 0x00]] {
            assert_eq!(decompressor.decompress(bad), decompress(bad));
        }
    }

    #[test]
    fn decompressor_is_send_sync_and_debug() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Decompressor>();

        assert_eq!(format!("{:?}", Decompressor::new()), "Decompressor { .. }");
    }

    #[test]
    fn decompress_into_appends_and_returns_count() {
        let mut decompressor = Decompressor::new();
        let mut out = b"prefix:".to_vec();

        let hello = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
        let written = decompressor.decompress_into(&hello, &mut out).unwrap();

        assert_eq!(written, 23);
        assert_eq!(out, b"prefix:hello hello hello hello");
    }

    #[test]
    fn decompress_into_concatenates_streams() {
        let mut decompressor = Decompressor::new();
        let mut out = Vec::new();
        let streams = mixed_streams();

        let mut expected = Vec::new();
        for (_, compressed, raw) in &streams {
            let written = decompressor.decompress_into(compressed, &mut out).unwrap();
            assert_eq!(written, raw.len());
            expected.extend_from_slice(raw);
        }

        assert!(out == expected, "concatenated output differs");
    }

    #[test]
    fn decompress_into_cannot_refer_back_into_existing_output() {
        let mut decompressor = Decompressor::new();

        // Hand-encoded: a copy at distance 1 before this stream wrote anything. With
        // "abc" already in the buffer it would be in range of the buffer, but not of
        // the stream, so it must still be rejected.
        let mut out = b"abc".to_vec();
        assert_eq!(
            decompressor.decompress_into(&[0x03, 0x02, 0x00], &mut out),
            Err(Error::DistanceTooFarBack)
        );
        assert_eq!(out, b"abc");

        // Hand-encoded: literal 'a', then a copy at distance 2 (one byte too far).
        assert_eq!(
            decompressor.decompress_into(&[0x4B, 0x04, 0x42, 0x00], &mut out),
            Err(Error::DistanceTooFarBack)
        );
        assert_eq!(out, b"abc");

        // Hand-encoded: literal 'a', then a copy at distance 1, exactly the stream's start.
        assert_eq!(
            decompressor.decompress_into(&[0x4B, 0x04, 0x02, 0x00], &mut out),
            Ok(4)
        );
        assert_eq!(out, b"abcaaaa");
    }

    #[test]
    fn decompress_into_truncates_on_error() {
        let mut decompressor = Decompressor::new();
        let dynamic = include_bytes!("data/dynamic_text.deflate");

        let mut out = b"keep me".to_vec();
        // Cut halfway: plenty of output has been written by the time it fails.
        assert!(
            decompressor
                .decompress_into(&dynamic[..dynamic.len() / 2], &mut out)
                .is_err()
        );
        assert_eq!(out, b"keep me");

        // And the buffer is still fine to use.
        let written = decompressor.decompress_into(dynamic, &mut out).unwrap();
        assert_eq!(written, include_bytes!("data/dynamic_text.txt").len());
        assert!(out[7..] == include_bytes!("data/dynamic_text.txt")[..]);
    }

    #[test]
    fn decompress_into_reuses_the_buffer_without_reallocating() {
        let mut decompressor = Decompressor::new();
        let mut streams = mixed_streams();
        // Largest input first, so the buffer is big enough for everything after it.
        streams.sort_by_key(|(_, compressed, _)| std::cmp::Reverse(compressed.len()));

        let mut out = Vec::new();
        decompressor
            .decompress_into(&streams[0].1, &mut out)
            .unwrap();
        let buffer = out.as_ptr();

        for (name, compressed, expected) in &streams {
            out.clear();
            decompressor.decompress_into(compressed, &mut out).unwrap();
            assert!(out == *expected, "{name}: wrong output");
            assert_eq!(out.as_ptr(), buffer, "{name}: buffer was reallocated");
        }
    }
}
