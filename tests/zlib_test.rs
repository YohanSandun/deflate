use rust_deflate::{Decompressor, Error, decompress_zlib};

#[cfg(test)]
mod tests {
    use super::*;

    // "hello hello hello hello", compressed by zlib (header 78 9C, then DEFLATE,
    // then the big-endian Adler-32 68 03 08 B1).
    const HELLO: [u8; 16] = [
        0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08,
        0xB1,
    ];

    fn text() -> &'static [u8] {
        include_bytes!("data/dynamic_text.txt")
    }

    // --- valid streams ------------------------------------------------------

    #[test]
    fn decompresses_short_stream() {
        assert_eq!(decompress_zlib(&HELLO).unwrap(), b"hello hello hello hello");
    }

    #[test]
    fn decompresses_empty_stream() {
        // zlib.compress(b"") -- the Adler-32 of nothing is 1.
        let data = [0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];

        assert_eq!(decompress_zlib(&data).unwrap(), b"");
    }

    #[test]
    fn decompresses_level_9() {
        let data = include_bytes!("data/zlib_text_l9.zz");

        assert!(decompress_zlib(data).unwrap() == text());
    }

    #[test]
    fn decompresses_small_window() {
        // Level 1 with a 512-byte window: header 18 19 (CINFO = 1).
        let data = include_bytes!("data/zlib_text_l1_w9.zz");

        assert!(decompress_zlib(data).unwrap() == text());
    }

    #[test]
    fn decompresses_stored_blocks() {
        // Level 0: stored blocks, so the checksum sits right after raw bytes.
        let data = include_bytes!("data/zlib_text_stored.zz");

        assert!(decompress_zlib(data).unwrap() == text());
    }

    #[test]
    fn decompresses_fixed_huffman() {
        let data = include_bytes!("data/zlib_text_fixed.zz");

        assert!(decompress_zlib(data).unwrap() == text());
    }

    #[test]
    fn ignores_bytes_after_checksum() {
        let mut data = HELLO.to_vec();
        data.extend_from_slice(b"trailing garbage");

        assert_eq!(decompress_zlib(&data).unwrap(), b"hello hello hello hello");
    }

    // --- header errors ------------------------------------------------------

    #[test]
    fn rejects_empty_and_one_byte_input() {
        assert_eq!(decompress_zlib(&[]), Err(Error::UnexpectedEndOfInput));
        assert_eq!(decompress_zlib(&[0x78]), Err(Error::UnexpectedEndOfInput));
    }

    #[test]
    fn rejects_bad_header_check() {
        let mut data = HELLO;
        data[1] = 0x9D; // (0x78 * 256 + 0x9D) % 31 != 0

        assert_eq!(decompress_zlib(&data), Err(Error::InvalidFcheck));
    }

    #[test]
    fn rejects_compression_method_other_than_deflate() {
        // CMF 0x77 (method 7) with a matching check byte.
        let mut data = HELLO;
        data[0] = 0x77;
        data[1] = 0x09;

        assert_eq!(
            decompress_zlib(&data),
            Err(Error::UnsupportedCompressionMethod)
        );
    }

    #[test]
    fn rejects_window_size_above_32k() {
        // CMF 0x88: method 8, but CINFO 8 (a 64 KB window), which RFC 1950 forbids.
        // Check byte 1C makes the header check pass.
        let mut data = HELLO;
        data[0] = 0x88;
        data[1] = 0x1C;

        assert_eq!(decompress_zlib(&data), Err(Error::InvalidWindowSize));
    }

    #[test]
    fn rejects_preset_dictionary() {
        // zlib.compressobj(zdict=b"hello"): FLG has FDICT set.
        let data = [
            0x78, 0xBB, 0x06, 0x2C, 0x02, 0x15, 0xCB, 0x00, 0x11, 0x0A, 0x60, 0x12, 0x00, 0x19,
            0x91, 0x04, 0x49,
        ];

        assert_eq!(decompress_zlib(&data), Err(Error::PresetDictionary));
    }

    // --- body and trailer errors ---------------------------------------------

    #[test]
    fn rejects_wrong_checksum() {
        let mut data = HELLO;
        data[15] ^= 0x01;

        assert_eq!(decompress_zlib(&data), Err(Error::ChecksumMismatch));
    }

    #[test]
    fn rejects_corrupted_data_with_valid_header() {
        // Flip a bit in the DEFLATE data: either the DEFLATE decoding or the checksum
        // must catch it.
        let mut data = include_bytes!("data/zlib_text_l9.zz").to_vec();
        data[500] ^= 0x10;

        assert!(decompress_zlib(&data).is_err());
    }

    #[test]
    fn rejects_truncated_checksum() {
        for cut in 1..=4 {
            let data = &HELLO[..HELLO.len() - cut];

            assert_eq!(
                decompress_zlib(data),
                Err(Error::UnexpectedEndOfInput),
                "{cut} bytes cut"
            );
        }
    }

    #[test]
    fn rejects_truncated_deflate_data() {
        let data = include_bytes!("data/zlib_text_l9.zz");

        assert!(decompress_zlib(&data[..data.len() / 2]).is_err());
    }

    // --- decompress_zlib_into ---------------------------------------------------

    #[test]
    fn into_appends_and_checksums_only_new_bytes() {
        let mut decompressor = Decompressor::new();
        let mut out = b"prefix:".to_vec();

        let written = decompressor.decompress_zlib_into(&HELLO, &mut out).unwrap();

        assert_eq!(written, 23);
        assert_eq!(out, b"prefix:hello hello hello hello");
    }

    #[test]
    fn into_truncates_on_checksum_error() {
        let mut decompressor = Decompressor::new();
        let mut out = b"keep me".to_vec();

        let mut data = HELLO;
        data[15] ^= 0x01;

        assert_eq!(
            decompressor.decompress_zlib_into(&data, &mut out),
            Err(Error::ChecksumMismatch)
        );
        assert_eq!(
            out, b"keep me",
            "output must not keep a stream that failed its checksum"
        );
    }

    #[test]
    fn into_truncates_on_missing_checksum() {
        let mut decompressor = Decompressor::new();
        let mut out = b"keep me".to_vec();

        assert_eq!(
            decompressor.decompress_zlib_into(&HELLO[..HELLO.len() - 2], &mut out),
            Err(Error::UnexpectedEndOfInput)
        );
        assert_eq!(out, b"keep me");
    }

    #[test]
    fn into_leaves_output_alone_on_header_error() {
        let mut decompressor = Decompressor::new();
        let mut out = b"keep me".to_vec();

        assert!(
            decompressor
                .decompress_zlib_into(&[0x78, 0x9D], &mut out)
                .is_err()
        );
        assert_eq!(out, b"keep me");
    }

    // --- reuse ------------------------------------------------------------------

    #[test]
    fn decompressor_mixes_zlib_and_raw_streams() {
        let mut decompressor = Decompressor::new();
        let raw_hello = &HELLO[2..12];
        let level9 = include_bytes!("data/zlib_text_l9.zz");

        for _ in 0..2 {
            assert_eq!(
                decompressor.decompress_zlib(&HELLO).unwrap(),
                b"hello hello hello hello"
            );
            assert_eq!(
                decompressor.decompress(raw_hello).unwrap(),
                b"hello hello hello hello"
            );
            assert!(decompressor.decompress_zlib(level9).unwrap() == text());

            let mut bad = HELLO;
            bad[15] ^= 0x01;
            assert!(decompressor.decompress_zlib(&bad).is_err());
        }
    }

    #[test]
    fn zlib_and_raw_agree_on_the_same_deflate_data() {
        // A zlib stream is just header + raw DEFLATE + checksum.
        let data = include_bytes!("data/zlib_text_fixed.zz");
        let raw = &data[2..data.len() - 4];

        assert_eq!(decompress_zlib(data), rust_deflate::decompress(raw));
    }
}
