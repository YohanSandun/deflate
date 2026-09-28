use std::io::Read;

use rust_deflate::{
    CompressionLevel, CompressionOptions, Compressor, DeflateDecoder, Strategy,
    StreamDecompressor, ZlibDecoder, compress, compress_with, compress_zlib,
    compress_zlib_with, decompress, decompress_zlib,
};

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_STORED_BLOCK: usize = 65_535;

    // Inputs covering the edge cases: empty, tiny, every byte value, both sides of
    // the stored-block size, long runs, text, and incompressible data.
    fn inputs() -> Vec<(String, Vec<u8>)> {
        let mut inputs: Vec<(String, Vec<u8>)> = vec![
            ("empty".into(), Vec::new()),
            ("one byte".into(), vec![0x42]),
            ("all byte values".into(), (0..=255u8).collect()),
            ("1 MB of one byte".into(), vec![0u8; 1 << 20]),
            (
                "text".into(),
                include_bytes!("data/dynamic_text.txt").to_vec(),
            ),
            (
                "matches".into(),
                include_bytes!("data/matches.raw").to_vec(),
            ),
        ];

        for len in [
            MAX_STORED_BLOCK - 1,
            MAX_STORED_BLOCK,
            MAX_STORED_BLOCK + 1,
            2 * MAX_STORED_BLOCK,
            2 * MAX_STORED_BLOCK + 1,
        ] {
            inputs.push((
                format!("{len} bytes"),
                (0..len).map(|i| (i * 7 % 256) as u8).collect(),
            ));
        }

        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let random: Vec<u8> = (0..300_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        inputs.push(("300 KB random".into(), random));
        inputs
    }

    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    // --- round trips (hold for every stage) ----------------------------------------

    #[test]
    fn raw_output_round_trips() {
        for (name, data) in inputs() {
            let compressed = compress(&data);
            assert!(decompress(&compressed).unwrap() == data, "{name}");
        }
    }

    #[test]
    fn zlib_output_round_trips() {
        for (name, data) in inputs() {
            let compressed = compress_zlib(&data);
            assert!(decompress_zlib(&compressed).unwrap() == data, "{name}");
        }
    }

    #[test]
    fn output_decodes_with_the_streaming_decoders() {
        for (name, data) in inputs() {
            let mut out = Vec::new();
            DeflateDecoder::new(&compress(&data)[..])
                .read_to_end(&mut out)
                .unwrap();
            assert!(out == data, "{name}: DeflateDecoder");

            let mut out = Vec::new();
            ZlibDecoder::new(&compress_zlib(&data)[..])
                .read_to_end(&mut out)
                .unwrap();
            assert!(out == data, "{name}: ZlibDecoder");

            let mut stream = StreamDecompressor::zlib();
            let mut out = Vec::new();
            for chunk in compress_zlib(&data).chunks(1000) {
                stream.push(chunk, &mut out).unwrap();
            }
            stream.finish(&mut out).unwrap();
            assert!(out == data, "{name}: StreamDecompressor");
        }
    }

    #[test]
    fn compression_is_deterministic() {
        for (name, data) in inputs() {
            assert_eq!(compress(&data), compress(&data), "{name}");
            assert_eq!(compress_zlib(&data), compress_zlib(&data), "{name}");
        }
    }

    // --- zlib framing (holds for every stage) --------------------------------------

    #[test]
    fn zlib_header_is_valid() {
        for (name, data) in inputs() {
            let compressed = compress_zlib(&data);
            let (cmf, flg) = (compressed[0], compressed[1]);

            assert_eq!(cmf & 0x0F, 8, "{name}: compression method must be DEFLATE");
            assert!(cmf >> 4 <= 7, "{name}: window size at most 32 KB");
            assert_eq!(
                (u16::from(cmf) * 256 + u16::from(flg)) % 31,
                0,
                "{name}: header check"
            );
            assert_eq!(flg & 0x20, 0, "{name}: no preset dictionary");
        }
    }

    #[test]
    fn zlib_trailer_is_the_big_endian_adler32() {
        for (name, data) in inputs() {
            let compressed = compress_zlib(&data);
            let trailer = &compressed[compressed.len() - 4..];

            assert_eq!(trailer, adler32(&data).to_be_bytes(), "{name}");
        }
    }

    #[test]
    fn zlib_body_is_the_raw_deflate_stream() {
        // zlib = 2-byte header + raw DEFLATE + 4-byte checksum.
        for (name, data) in inputs() {
            let zlib = compress_zlib(&data);
            assert!(zlib[2..zlib.len() - 4] == compress(&data)[..], "{name}");
        }
    }

    #[test]
    fn zlib_size_adds_six_bytes() {
        for (name, data) in inputs() {
            assert_eq!(
                compress_zlib(&data).len(),
                compress(&data).len() + 6,
                "{name}"
            );
        }
    }

    // --- compression levels ---------------------------------------------------------

    const PRESETS: [CompressionLevel; 4] = [
        CompressionLevel::NONE,
        CompressionLevel::FAST,
        CompressionLevel::MEDIUM,
        CompressionLevel::BEST,
    ];

    #[test]
    fn presets_are_zlibs_levels() {
        assert_eq!(CompressionLevel::NONE.get(), 0);
        assert_eq!(CompressionLevel::FAST.get(), 1);
        assert_eq!(CompressionLevel::MEDIUM.get(), 6);
        assert_eq!(CompressionLevel::BEST.get(), 9);
        assert_eq!(CompressionLevel::default(), CompressionLevel::MEDIUM);
    }

    #[test]
    fn levels_above_9_are_9() {
        assert_eq!(CompressionLevel::new(10), CompressionLevel::BEST);
        assert_eq!(CompressionLevel::new(255), CompressionLevel::BEST);
        assert_eq!(CompressionLevel::from(4), CompressionLevel::new(4));
    }

    #[test]
    fn default_is_medium() {
        let data = include_bytes!("data/dynamic_text.txt");
        let medium = compress_with(data, CompressionLevel::MEDIUM);

        assert_eq!(compress(data), medium);
        assert_eq!(compress_with(data, CompressionOptions::default()), medium);
        assert_eq!(
            compress_zlib(data),
            compress_zlib_with(data, CompressionLevel::MEDIUM)
        );
    }

    #[test]
    fn every_level_round_trips() {
        for (name, data) in inputs() {
            for level in 0..=9 {
                let level = CompressionLevel::new(level);
                let raw = compress_with(&data, level);
                assert!(decompress(&raw).unwrap() == data, "{name}, {level:?}");
                let zlib = compress_zlib_with(&data, level);
                assert!(decompress_zlib(&zlib).unwrap() == data, "{name}, {level:?}");
            }
        }
    }

    #[test]
    fn higher_levels_compress_better() {
        for data in [
            &include_bytes!("data/dynamic_text.txt")[..],
            include_bytes!("data/matches.raw"),
        ] {
            let sizes = PRESETS.map(|level| compress_with(data, level).len());
            assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
            assert!(sizes[1] < sizes[0] / 2, "{sizes:?}");
        }
    }

    #[test]
    fn zlib_header_records_the_level() {
        // FLEVEL as zlib writes it: 78 01 (levels 0-1), 78 9C (6), 78 DA (9).
        for (level, header) in PRESETS.into_iter().zip([
            [0x78, 0x01],
            [0x78, 0x01],
            [0x78, 0x9C],
            [0x78, 0xDA],
        ]) {
            assert_eq!(compress_zlib_with(b"abc", level)[..2], header, "{level:?}");
        }
    }

    // --- no compression -------------------------------------------------------------

    #[test]
    fn no_compression_is_input_plus_five_bytes_per_block() {
        for (name, data) in inputs() {
            let blocks = data.len().div_ceil(MAX_STORED_BLOCK).max(1);
            let raw = compress_with(&data, CompressionLevel::NONE);
            assert_eq!(raw.len(), data.len() + 5 * blocks, "{name}");
        }
    }

    #[test]
    fn no_compression_is_stored_whatever_the_strategy() {
        let data = include_bytes!("data/dynamic_text.txt");
        let stored = compress_with(data, CompressionOptions::new().strategy(Strategy::Stored));
        for strategy in [Strategy::Fixed, Strategy::Dynamic] {
            let options = CompressionOptions::new()
                .strategy(strategy)
                .level(CompressionLevel::NONE);
            assert_eq!(compress_with(data, options), stored, "{strategy:?}");
        }
    }

    #[test]
    fn no_compression_matches_zlib_level_0_for_small_inputs() {
        // zlib.compress(data, 0) from Python: header 78 01 (FLEVEL 0), one stored block,
        // then the Adler-32. (For inputs over 64 KB zlib splits blocks differently, at
        // 65,531 bytes, so only single-block inputs can match byte for byte.)
        let none = CompressionLevel::NONE;
        assert_eq!(
            compress_zlib_with(b"", none),
            [
                0x78, 0x01, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01
            ]
        );
        assert_eq!(
            compress_zlib_with(b"hello", none),
            [
                0x78, 0x01, 0x01, 0x05, 0x00, 0xFA, 0xFF, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x06, 0x2C,
                0x02, 0x15
            ]
        );
        assert_eq!(
            compress_with(b"hello", none),
            [0x01, 0x05, 0x00, 0xFA, 0xFF, 0x68, 0x65, 0x6C, 0x6C, 0x6F]
        );
    }

    // --- Compressor -----------------------------------------------------------------

    #[test]
    fn compressor_matches_the_functions() {
        let mut compressor = Compressor::new();
        for (name, data) in inputs() {
            for level in PRESETS {
                assert!(
                    compressor.compress_with(&data, level) == compress_with(&data, level),
                    "{name}, {level:?}"
                );
                assert!(
                    compressor.compress_zlib_with(&data, level)
                        == compress_zlib_with(&data, level),
                    "{name}, {level:?}"
                );
            }
            assert!(compressor.compress(&data) == compress(&data), "{name}");
            assert!(compressor.compress_zlib(&data) == compress_zlib(&data), "{name}");
        }
    }

    #[test]
    fn compress_into_appends_and_returns_the_length() {
        let data = include_bytes!("data/dynamic_text.txt");
        let mut compressor = Compressor::new();

        let mut out = b"existing".to_vec();
        let written = compressor.compress_into(data, &mut out);
        assert_eq!(&out[..8], b"existing");
        assert_eq!(written, out.len() - 8);
        assert_eq!(out[8..], compress(data));

        let mut out = b"existing".to_vec();
        let written = compressor.compress_zlib_into(data, &mut out);
        assert_eq!(&out[..8], b"existing");
        assert_eq!(written, out.len() - 8);
        assert_eq!(out[8..], compress_zlib(data));
    }

    #[test]
    fn compress_into_with_takes_a_level_or_options() {
        let data = include_bytes!("data/matches.raw");
        let mut compressor = Compressor::new();

        for level in PRESETS {
            let mut out = Vec::new();
            compressor.compress_into_with(data, &mut out, level);
            assert!(out == compress_with(&data[..], level), "{level:?}");

            out.clear();
            let options = CompressionOptions::new().strategy(Strategy::Fixed).level(level);
            compressor.compress_zlib_into_with(data, &mut out, options);
            assert!(out == compress_zlib_with(&data[..], options), "{level:?}");
        }
    }

    #[test]
    fn empty_input_appends_a_valid_stream() {
        let mut compressor = Compressor::new();
        let mut out = vec![1, 2, 3];
        let written = compressor.compress_into(b"", &mut out);
        assert!(written > 0);
        assert_eq!(decompress(&out[3..]).unwrap(), b"");
    }
}
