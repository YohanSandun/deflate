use super::{check_header, write_header};
use crate::io::bit_writer::BitWriter;
use crate::options::{CompressionOptions, Strategy};

fn header(options: CompressionOptions) -> [u8; 2] {
    let mut writer = BitWriter::new();
    write_header(&mut writer, options);
    writer.finish().try_into().unwrap()
}

#[test]
fn stored_is_always_fastest() {
    for level in 0..=9 {
        let options = CompressionOptions::new()
            .strategy(Strategy::Stored)
            .level(level);
        assert_eq!(header(options), [0x78, 0x01], "level {level}");
    }
}

#[test]
fn flevel_follows_zlib() {
    // The headers zlib writes: 78 01 (levels 0-1), 78 5E (2-5), 78 9C (6), 78 DA (7-9).
    for (level, want) in [
        (0, [0x78, 0x01]),
        (1, [0x78, 0x01]),
        (2, [0x78, 0x5E]),
        (5, [0x78, 0x5E]),
        (6, [0x78, 0x9C]),
        (7, [0x78, 0xDA]),
        (9, [0x78, 0xDA]),
    ] {
        for strategy in [Strategy::Fixed, Strategy::Dynamic] {
            let options = CompressionOptions::new().strategy(strategy).level(level);
            assert_eq!(header(options), want, "{strategy:?} level {level}");
        }
    }
}

#[test]
fn every_header_passes_check_header() {
    for strategy in [Strategy::Stored, Strategy::Fixed, Strategy::Dynamic] {
        for level in 0..=9 {
            let options = CompressionOptions::new().strategy(strategy).level(level);
            assert_eq!(check_header(&header(options)), Ok(()));
        }
    }
}
