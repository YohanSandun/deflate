use rust_deflate::{
    Decompressor, Error, OutputOptions, decompress, decompress_with, decompress_zlib,
    decompress_zlib_with,
};

#[cfg(test)]
mod tests {
    use super::*;

    // "hello hello hello hello" (23 bytes): raw DEFLATE, and the same wrapped in zlib.
    const HELLO_RAW: [u8; 10] = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];
    const HELLO_ZLIB: [u8; 16] = [
        0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08,
        0xB1,
    ];

    fn text() -> &'static [u8] {
        include_bytes!("data/dynamic_text.txt")
    }

    // A decompression bomb: one fixed Huffman block holding a single zero byte
    // followed by `copies` matches of length 258 at distance 1, so every 13 bits of
    // input become 258 bytes of output. Built bit by bit rather than stored as a file.
    fn bomb(copies: usize) -> Vec<u8> {
        // BFINAL = 1, BTYPE = 01 (fixed), least significant bit first.
        let mut bits: Vec<u32> = vec![1, 1, 0];
        let mut code = |value: u32, length: u32| {
            // Huffman codes are sent most significant bit first.
            for i in (0..length).rev() {
                bits.push((value >> i) & 1);
            }
        };

        code(0x30, 8); // literal 0
        for _ in 0..copies {
            code(0b1100_0101, 8); // length symbol 285: 258 bytes, no extra bits
            code(0, 5); // distance symbol 0: distance 1
        }
        code(0, 7); // end of block

        let mut bytes = vec![0u8; bits.len().div_ceil(8)];
        for (i, bit) in bits.iter().enumerate() {
            bytes[i / 8] |= (*bit as u8) << (i % 8);
        }
        bytes
    }

    // --- max_output -----------------------------------------------------------

    #[test]
    fn limit_equal_to_output_size_succeeds() {
        let mut decompressor = Decompressor::new();
        let mut out = Vec::new();

        let options = OutputOptions::new().max_output(23);

        assert_eq!(
            decompressor.decompress_into_with(&HELLO_RAW, &mut out, options),
            Ok(23)
        );
        out.clear();
        assert_eq!(
            decompressor.decompress_zlib_into_with(&HELLO_ZLIB, &mut out, options),
            Ok(23)
        );
    }

    #[test]
    fn limit_one_byte_short_fails_and_leaves_output_alone() {
        let mut decompressor = Decompressor::new();
        let mut out = b"prefix".to_vec();

        let options = OutputOptions::new().max_output(22);

        assert_eq!(
            decompressor.decompress_into_with(&HELLO_RAW, &mut out, options),
            Err(Error::OutputLimitExceeded)
        );
        assert_eq!(out, b"prefix");
        assert_eq!(
            decompressor.decompress_zlib_into_with(&HELLO_ZLIB, &mut out, options),
            Err(Error::OutputLimitExceeded)
        );
        assert_eq!(out, b"prefix");
    }

    #[test]
    fn limit_counts_only_bytes_this_call_appends() {
        let mut decompressor = Decompressor::new();
        let mut out = vec![0u8; 1000];

        let written = decompressor.decompress_into_with(
            &HELLO_RAW,
            &mut out,
            OutputOptions::new().max_output(23),
        );

        assert_eq!(written, Ok(23));
        assert_eq!(out.len(), 1023);
    }

    #[test]
    fn limit_applies_to_every_block_type() {
        let len = text().len();
        let streams: [(&str, &[u8]); 4] = [
            ("dynamic", include_bytes!("data/zlib_text_l9.zz")),
            ("small window", include_bytes!("data/zlib_text_l1_w9.zz")),
            ("stored", include_bytes!("data/zlib_text_stored.zz")),
            ("fixed", include_bytes!("data/zlib_text_fixed.zz")),
        ];
        let mut decompressor = Decompressor::new();
        let mut out = Vec::new();

        for (name, compressed) in streams {
            for limit in [len, len + 1] {
                out.clear();
                let result = decompressor.decompress_zlib_into_with(
                    compressed,
                    &mut out,
                    OutputOptions::new().max_output(limit),
                );
                assert_eq!(result, Ok(len), "{name}, limit {limit}");
                assert!(out == text(), "{name}: wrong output");
            }

            // Short by one byte, and far short (it must stop early, not at the end).
            for limit in [len - 1, 1000, 0] {
                out.clear();
                let result = decompressor.decompress_zlib_into_with(
                    compressed,
                    &mut out,
                    OutputOptions::new().max_output(limit),
                );
                assert_eq!(
                    result,
                    Err(Error::OutputLimitExceeded),
                    "{name}, limit {limit}"
                );
                assert!(
                    out.is_empty(),
                    "{name}, limit {limit}: output not rolled back"
                );
            }
        }
    }

    #[test]
    fn zero_limit_accepts_empty_streams() {
        let mut decompressor = Decompressor::new();
        let mut out = Vec::new();

        // zlib.compress(b"")
        let empty = [0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];

        assert_eq!(
            decompressor.decompress_zlib_into_with(&empty, &mut out, OutputOptions::exact(0)),
            Ok(0)
        );
    }

    #[test]
    fn limit_stops_a_decompression_bomb_without_allocating_its_output() {
        // About 10 MB of output from about 65 KB of input.
        let compressed = bomb(40_000);
        let mut decompressor = Decompressor::new();

        // Check the bomb really expands that far when nothing stops it.
        let mut out = Vec::new();
        let full = decompressor.decompress_into(&compressed, &mut out);
        assert_eq!(full, Ok(1 + 40_000 * 258));

        let limit = 256 * 1024;
        let mut out = Vec::new();
        let limited = decompressor.decompress_into_with(
            &compressed,
            &mut out,
            OutputOptions::new().max_output(limit),
        );

        assert_eq!(limited, Err(Error::OutputLimitExceeded));
        assert!(out.is_empty());
        // The buffer stayed near the limit, not the 10 MB the input asks for.
        assert!(
            out.capacity() < 2 * limit + 64 * 1024,
            "capacity {} for a {limit}-byte limit",
            out.capacity()
        );
    }

    #[test]
    fn limit_stops_a_zlib_bomb_before_checking_the_checksum() {
        let mut compressed = vec![0x78, 0x01];
        compressed.extend(bomb(40_000));
        compressed.extend([0, 0, 0, 0]); // wrong checksum; the limit has to trigger first

        let mut out = Vec::new();
        let result = Decompressor::new().decompress_zlib_into_with(
            &compressed,
            &mut out,
            OutputOptions::new().max_output(1 << 20),
        );

        assert_eq!(result, Err(Error::OutputLimitExceeded));
    }

    // --- size_hint ------------------------------------------------------------

    #[test]
    fn exact_size_allocates_once_without_the_4x_guess() {
        // Stored blocks barely compress, so the usual guess (4x the input) would
        // allocate about four times the output. An exact hint allocates just the output.
        let compressed = include_bytes!("data/zlib_text_stored.zz");
        let len = text().len();

        let mut out = Vec::new();
        Decompressor::new()
            .decompress_zlib_into_with(compressed, &mut out, OutputOptions::exact(len))
            .unwrap();

        assert!(out == text());
        assert!(out.capacity() >= len);
        assert!(
            out.capacity() < len + 1024,
            "capacity {} for {len} bytes",
            out.capacity()
        );
    }

    #[test]
    fn exact_size_never_reallocates_the_buffer() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let len = text().len();

        // Reserve through the hint, then check the decode never moved the buffer by
        // reserving the same hint for a second, identical decode.
        let mut out = Vec::new();
        let mut decompressor = Decompressor::new();
        decompressor
            .decompress_zlib_into_with(compressed, &mut out, OutputOptions::exact(len))
            .unwrap();
        let buffer = out.as_ptr();
        let capacity = out.capacity();

        out.clear();
        decompressor
            .decompress_zlib_into_with(compressed, &mut out, OutputOptions::exact(len))
            .unwrap();

        assert_eq!(out.as_ptr(), buffer);
        assert_eq!(out.capacity(), capacity);
    }

    #[test]
    fn too_small_hint_still_decompresses() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let mut out = Vec::new();

        let written = Decompressor::new().decompress_zlib_into_with(
            compressed,
            &mut out,
            OutputOptions::new().size_hint(10),
        );

        assert_eq!(written, Ok(text().len()));
        assert!(out == text());
    }

    #[test]
    fn hint_is_capped_by_the_limit() {
        // A huge hint (as if read from a hostile header) doesn't get allocated when a
        // limit is also set.
        let mut out = Vec::new();
        let options = OutputOptions::new().size_hint(1 << 40).max_output(100);

        let written = Decompressor::new().decompress_into_with(&HELLO_RAW, &mut out, options);

        assert_eq!(written, Ok(23));
        assert!(out.capacity() < 4096, "capacity {}", out.capacity());
    }

    #[test]
    fn options_have_useful_defaults_and_debug() {
        assert_eq!(OutputOptions::new(), OutputOptions::default());
        assert_eq!(
            OutputOptions::exact(5),
            OutputOptions::new().max_output(5).size_hint(5)
        );
        assert!(format!("{:?}", OutputOptions::exact(5)).contains("5"));
    }

    // --- one-shot _with functions and methods -----------------------------------

    #[test]
    fn one_shot_functions_apply_the_limit() {
        let limited = OutputOptions::new().max_output(22);

        assert_eq!(
            decompress_with(&HELLO_RAW, limited),
            Err(Error::OutputLimitExceeded)
        );
        assert_eq!(
            decompress_zlib_with(&HELLO_ZLIB, limited),
            Err(Error::OutputLimitExceeded)
        );

        let exact = OutputOptions::exact(23);
        assert_eq!(
            decompress_with(&HELLO_RAW, exact).unwrap(),
            b"hello hello hello hello"
        );
        assert_eq!(
            decompress_zlib_with(&HELLO_ZLIB, exact).unwrap(),
            b"hello hello hello hello"
        );
    }

    #[test]
    fn one_shot_methods_apply_the_limit_and_stay_usable() {
        let mut decompressor = Decompressor::new();
        let limited = OutputOptions::new().max_output(22);

        for _ in 0..2 {
            assert_eq!(
                decompressor.decompress_with(&HELLO_RAW, limited),
                Err(Error::OutputLimitExceeded)
            );
            assert_eq!(
                decompressor.decompress_zlib_with(&HELLO_ZLIB, limited),
                Err(Error::OutputLimitExceeded)
            );

            // A failed call doesn't affect the next one.
            assert_eq!(
                decompressor.decompress(&HELLO_RAW).unwrap(),
                b"hello hello hello hello"
            );
            assert_eq!(
                decompressor.decompress_zlib(&HELLO_ZLIB).unwrap(),
                b"hello hello hello hello"
            );
        }
    }

    #[test]
    fn one_shot_with_default_options_matches_plain_versions() {
        let streams: [&[u8]; 3] = [
            include_bytes!("data/zlib_text_l9.zz"),
            include_bytes!("data/zlib_text_stored.zz"),
            &[0x78, 0x9D],
        ];
        let mut decompressor = Decompressor::new();

        for zlib in streams {
            assert_eq!(
                decompress_zlib_with(zlib, OutputOptions::new()),
                decompress_zlib(zlib)
            );
            assert_eq!(
                decompressor.decompress_zlib_with(zlib, OutputOptions::new()),
                decompress_zlib(zlib)
            );
        }
        for raw in [&HELLO_RAW[..], &[0x07], &[]] {
            assert_eq!(decompress_with(raw, OutputOptions::new()), decompress(raw));
            assert_eq!(
                decompressor.decompress_with(raw, OutputOptions::new()),
                decompress(raw)
            );
        }
    }

    #[test]
    fn one_shot_limit_stops_a_bomb() {
        let compressed = bomb(40_000);

        assert_eq!(
            decompress_with(&compressed, OutputOptions::new().max_output(1 << 20)),
            Err(Error::OutputLimitExceeded)
        );
        assert_eq!(
            decompress(&compressed).map(|v| v.len()),
            Ok(1 + 40_000 * 258)
        );
    }

    #[test]
    fn one_shot_exact_size_returns_a_tightly_sized_vec() {
        let compressed = include_bytes!("data/zlib_text_stored.zz");
        let len = text().len();

        let data = decompress_zlib_with(compressed, OutputOptions::exact(len)).unwrap();

        assert!(data == text());
        assert!(
            data.capacity() < len + 1024,
            "capacity {} for {len} bytes",
            data.capacity()
        );
    }
}
