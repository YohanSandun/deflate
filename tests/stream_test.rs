use std::io::{self, Read};

use rust_deflate::{DeflateDecoder, Error, ZlibDecoder, decompress};

#[cfg(test)]
mod tests {
    use super::*;

    // --- helpers ------------------------------------------------------------------

    // A reader that hands out at most `chunk` bytes per call, and reports
    // `Interrupted` before every `interrupt_every`-th call (0 = never), to check the
    // decoders cope with sources that trickle data.
    struct Trickle<'a> {
        data: &'a [u8],
        chunk: usize,
        interrupt_every: usize,
        calls: usize,
    }

    impl<'a> Trickle<'a> {
        fn new(data: &'a [u8], chunk: usize) -> Self {
            Self {
                data,
                chunk,
                interrupt_every: 0,
                calls: 0,
            }
        }

        fn interrupting(mut self, every: usize) -> Self {
            self.interrupt_every = every;
            self
        }
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.interrupt_every > 0 && self.calls % self.interrupt_every == 0 {
                return Err(io::ErrorKind::Interrupted.into());
            }

            let n = self.chunk.min(buf.len()).min(self.data.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            Ok(n)
        }
    }

    // Reads everything from `reader` using an output buffer of `buf_size` bytes.
    fn read_all(mut reader: impl Read, buf_size: usize) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; buf_size];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => return Ok(out),
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }

    fn inner_error(error: &io::Error) -> Option<Error> {
        error.get_ref()?.downcast_ref::<Error>().copied()
    }

    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    // Raw DEFLATE: a single zero byte, then `copies` matches of 258 bytes at
    // distance 1 (fixed Huffman), so the output is `1 + copies * 258` zeros.
    fn zero_run(copies: usize) -> Vec<u8> {
        let mut bits: Vec<u32> = vec![1, 1, 0]; // BFINAL = 1, BTYPE = fixed
        let mut code = |value: u32, length: u32| {
            for i in (0..length).rev() {
                bits.push((value >> i) & 1);
            }
        };
        code(0x30, 8); // literal 0
        for _ in 0..copies {
            code(0b1100_0101, 8); // length 258
            code(0, 5); // distance 1
        }
        code(0, 7); // end of block

        let mut bytes = vec![0u8; bits.len().div_ceil(8)];
        for (i, bit) in bits.iter().enumerate() {
            bytes[i / 8] |= (*bit as u8) << (i % 8);
        }
        bytes
    }

    fn raw_streams() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
        let fixtures: [(&str, &[u8]); 4] = [
            ("dynamic text", include_bytes!("data/dynamic_text.deflate")),
            ("fixed text", include_bytes!("data/fixed_text.deflate")),
            ("matches", include_bytes!("data/matches.deflate")),
            (
                "three dynamic blocks",
                include_bytes!("data/dynamic_multi.deflate"),
            ),
        ];
        let mut streams: Vec<_> = fixtures
            .into_iter()
            .map(|(name, c)| (name, c.to_vec(), decompress(c).unwrap()))
            .collect();

        streams.push((
            "stored then fixed",
            vec![
                0x00, 0x05, 0x00, 0xFA, 0xFF, b'1', b'2', b'3', b'4', b'5', 0xCB, 0x48, 0xCD, 0xC9,
                0xC9, 0x57, 0xC8, 0x40, 0x27, 0x01,
            ],
            b"12345hello hello hello hello".to_vec(),
        ));
        streams.push((
            "empty stored",
            vec![0x01, 0x00, 0x00, 0xFF, 0xFF],
            Vec::new(),
        ));
        streams
    }

    fn zlib_streams() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("level 9", include_bytes!("data/zlib_text_l9.zz")),
            ("small window", include_bytes!("data/zlib_text_l1_w9.zz")),
            ("stored", include_bytes!("data/zlib_text_stored.zz")),
            ("fixed", include_bytes!("data/zlib_text_fixed.zz")),
        ]
    }

    fn text() -> &'static [u8] {
        include_bytes!("data/dynamic_text.txt")
    }

    // --- correct output, however the input and output are split ---------------------

    #[test]
    fn deflate_decoder_matches_whole_buffer_decoding() {
        for (name, compressed, expected) in raw_streams() {
            for chunk in [1, 2, 7, 1000, 1 << 20] {
                for buf_size in [1, 13, 64 * 1024] {
                    // Both tiny is slow in debug builds and adds nothing: each is
                    // covered on its own.
                    if chunk <= 2 && buf_size == 1 {
                        continue;
                    }

                    let decoder = DeflateDecoder::new(Trickle::new(&compressed, chunk));
                    let output = read_all(decoder, buf_size).unwrap();
                    assert!(
                        output == expected,
                        "{name}: chunk {chunk}, buffer {buf_size}"
                    );
                }
            }
        }
    }

    #[test]
    fn zlib_decoder_matches_whole_buffer_decoding() {
        for (name, compressed) in zlib_streams() {
            for chunk in [1, 3, 4096, 1 << 20] {
                for buf_size in [1, 777, 64 * 1024] {
                    if chunk <= 3 && buf_size == 1 {
                        continue;
                    }

                    let decoder = ZlibDecoder::new(Trickle::new(compressed, chunk));
                    let output = read_all(decoder, buf_size).unwrap();
                    assert!(output == text(), "{name}: chunk {chunk}, buffer {buf_size}");
                }
            }
        }
    }

    #[test]
    fn decoders_retry_interrupted_reads() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let decoder = ZlibDecoder::new(Trickle::new(compressed, 100).interrupting(3));

        assert!(read_all(decoder, 4096).unwrap() == text());
    }

    #[test]
    fn works_with_std_io_helpers() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");

        let mut output = Vec::new();
        io::copy(&mut ZlibDecoder::new(&compressed[..]), &mut output).unwrap();
        assert!(output == text());

        let mut string = String::new();
        ZlibDecoder::new(&compressed[..])
            .read_to_string(&mut string)
            .unwrap();
        assert!(string.as_bytes() == text());
    }

    #[test]
    fn long_stream_decodes_in_pieces() {
        // About 20 MB of output from about 130 KB of input: a hundred times the
        // decoder's buffers, so the window has to be recycled many times.
        let copies = 80_000;
        let compressed = zero_run(copies);

        let mut decoder = DeflateDecoder::new(Trickle::new(&compressed, 4096));
        let mut buf = vec![0u8; 50_000];
        let mut total = 0usize;
        loop {
            let n = decoder.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            assert!(buf[..n].iter().all(|&b| b == 0));
            total += n;
        }

        assert_eq!(total, 1 + copies * 258);
    }

    #[test]
    fn long_zlib_stream_verifies_its_checksum() {
        let copies = 40_000;
        let raw = zero_run(copies);
        let output_len = 1 + copies * 258;

        let mut zlib = vec![0x78, 0x01];
        zlib.extend(&raw);
        zlib.extend(adler32(&vec![0u8; output_len]).to_be_bytes());

        let mut sink = io::sink();
        let copied =
            io::copy(&mut ZlibDecoder::new(Trickle::new(&zlib, 65536)), &mut sink).unwrap();
        assert_eq!(copied, output_len as u64);
    }

    #[test]
    fn empty_buffer_reads_nothing_and_consumes_nothing() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let mut decoder = ZlibDecoder::new(&compressed[..]);

        assert_eq!(decoder.read(&mut []).unwrap(), 0);
        assert!(read_all(decoder, 4096).unwrap() == text());
    }

    #[test]
    fn data_after_the_stream_is_ignored() {
        let mut data = include_bytes!("data/zlib_text_l9.zz").to_vec();
        data.extend_from_slice(b"trailing bytes");

        assert!(read_all(ZlibDecoder::new(&data[..]), 4096).unwrap() == text());
    }

    #[test]
    fn end_of_stream_keeps_returning_zero() {
        let mut decoder = DeflateDecoder::new(&[0x01, 0x00, 0x00, 0xFF, 0xFF][..]);
        let mut buf = [0u8; 16];

        for _ in 0..3 {
            assert_eq!(decoder.read(&mut buf).unwrap(), 0);
        }
    }

    // --- errors ---------------------------------------------------------------------

    #[test]
    fn truncated_stream_is_an_unexpected_eof() {
        let compressed = include_bytes!("data/dynamic_text.deflate");

        for cut in [0, 1, 20, compressed.len() / 2, compressed.len() - 1] {
            let error = read_all(DeflateDecoder::new(&compressed[..cut]), 4096).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
            assert_eq!(
                inner_error(&error),
                Some(Error::UnexpectedEndOfInput),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn corrupt_stream_is_invalid_data_and_stays_failed() {
        let mut decoder = DeflateDecoder::new(&[0x07][..]);
        let mut buf = [0u8; 16];

        for _ in 0..2 {
            let error = decoder.read(&mut buf).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(inner_error(&error), Some(Error::InvalidBlockType));
        }
    }

    #[test]
    fn zlib_checksum_mismatch_is_reported_at_the_end() {
        let mut data = include_bytes!("data/zlib_text_l9.zz").to_vec();
        let last = data.len() - 1;
        data[last] ^= 0x01;

        let error = read_all(ZlibDecoder::new(&data[..]), 4096).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(inner_error(&error), Some(Error::ChecksumMismatch));
    }

    #[test]
    fn zlib_header_errors_match_the_whole_buffer_api() {
        let cases: [(&[u8], Error); 4] = [
            (&[0x78, 0x9D, 0x03, 0x00], Error::InvalidFcheck),
            (
                &[0x77, 0x09, 0x03, 0x00],
                Error::UnsupportedCompressionMethod,
            ),
            (&[0x88, 0x1C, 0x03, 0x00], Error::InvalidWindowSize),
            (&[0x78], Error::UnexpectedEndOfInput),
        ];

        for (data, expected) in cases {
            let error = read_all(ZlibDecoder::new(data), 64).unwrap_err();
            assert_eq!(inner_error(&error), Some(expected));
        }
    }

    #[test]
    fn errors_from_the_source_are_passed_through() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "no access"))
            }
        }

        let error = read_all(ZlibDecoder::new(Failing), 64).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "no access");
    }

    #[test]
    fn error_converts_to_io_error() {
        let eof: io::Error = Error::UnexpectedEndOfInput.into();
        assert_eq!(eof.kind(), io::ErrorKind::UnexpectedEof);

        let invalid: io::Error = Error::ChecksumMismatch.into();
        assert_eq!(invalid.kind(), io::ErrorKind::InvalidData);
        assert_eq!(inner_error(&invalid), Some(Error::ChecksumMismatch));
    }

    // --- accessors ------------------------------------------------------------------

    #[test]
    fn wrapped_reader_is_accessible() {
        let compressed = include_bytes!("data/zlib_text_l9.zz");
        let mut decoder = ZlibDecoder::new(&compressed[..]);

        assert_eq!(decoder.get_ref().len(), compressed.len());
        let _ = decoder.get_mut();
        assert_eq!(format!("{decoder:?}"), "ZlibDecoder { .. }");

        let remaining = decoder.into_inner();
        assert_eq!(remaining.len(), compressed.len(), "nothing read yet");
    }
}
