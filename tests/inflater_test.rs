use deflate::compression::inflater::Inflater;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflate_stored_block() {
        let data = [
            0x01, 0x26, 0x00, 0xD9, 0xFF, 0x48, 0x65, 0x6C, 0x6C, 0x6F,
            0x2C, 0x20, 0x74, 0x68, 0x69, 0x73, 0x20, 0x69, 0x73, 0x20,
            0x61, 0x20, 0x73, 0x74, 0x6F, 0x72, 0x65, 0x64, 0x20, 0x44,
            0x45, 0x46, 0x4C, 0x41, 0x54, 0x45, 0x20, 0x62, 0x6C, 0x6F,
            0x63, 0x6B, 0x21,
        ];

        let mut inflater = Inflater::new(&data);

        let inflated = inflater.inflate();

        assert_eq!(
            inflated.unwrap(),
            b"Hello, this is a stored DEFLATE block!"
        );
    }

    // Streams below were produced by zlib with the Z_FIXED strategy (raw deflate),
    // unless noted as hand-encoded.

    #[test]
    fn inflate_fixed_block_with_back_references() {
        let data = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01];

        let inflated = Inflater::new(&data).inflate();

        assert_eq!(inflated.unwrap(), b"hello hello hello hello");
    }

    #[test]
    fn inflate_fixed_block_with_overlapping_copies() {
        // Distance 1 and 2 copies that are longer than their distance.
        let data = [
            0x4B, 0x4C, 0x1C, 0x05, 0x44, 0x83, 0xA4, 0x51, 0x38, 0x98,
            0x20, 0x00,
        ];

        let inflated = Inflater::new(&data).inflate().unwrap();

        let mut expected = vec![b'a'; 300];
        expected.extend(b"ab".repeat(200));
        assert_eq!(inflated, expected);
    }

    #[test]
    fn inflate_multiple_fixed_blocks() {
        // Three fixed blocks, BFINAL set only on the last; later blocks copy from earlier ones.
        let data = [
            0x4A, 0xCB, 0x2C, 0x2A, 0x2E, 0x51, 0x48, 0xCA, 0xC9, 0x4F,
            0xCE, 0xD6, 0x51, 0x48, 0x43, 0x70, 0xF4, 0x14, 0x00, 0x2A,
            0x4E, 0x4D, 0xCE, 0xCF, 0x4B, 0x81, 0x49, 0x21, 0xF3, 0xF4,
            0x14, 0x00, 0x2B, 0xC9, 0xC8, 0x2C, 0x82, 0x72, 0x14, 0x8A,
            0x52, 0xD3, 0x52, 0x8B, 0x8A, 0x15, 0x4A, 0xF2, 0x91, 0xB5,
            0x03, 0x00,
        ];

        let inflated = Inflater::new(&data).inflate();

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

        let inflated = Inflater::new(data).inflate().unwrap();

        assert_eq!(inflated.len(), expected.len());
        assert!(inflated == expected, "inflated output differs from expected text");
    }

    #[test]
    fn inflate_rejects_distance_with_empty_output() {
        // Hand-encoded: copy of length 3 at distance 1 before any output.
        let data = [0x03, 0x02, 0x00];

        let inflated = Inflater::new(&data).inflate();

        assert_eq!(inflated, Err("invalid distance: too far back".to_string()));
    }

    #[test]
    fn inflate_rejects_distance_one_past_output_start() {
        // Hand-encoded: literal 'a', then copy of length 3 at distance 2.
        let data = [0x4B, 0x04, 0x42, 0x00];

        let inflated = Inflater::new(&data).inflate();

        assert_eq!(inflated, Err("invalid distance: too far back".to_string()));
    }

    #[test]
    fn inflate_accepts_distance_equal_to_output_length() {
        // Hand-encoded: literal 'a', then copy of length 3 at distance 1.
        let data = [0x4B, 0x04, 0x02, 0x00];

        let inflated = Inflater::new(&data).inflate();

        assert_eq!(inflated.unwrap(), b"aaaa");
    }
}
