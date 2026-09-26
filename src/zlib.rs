use crate::Error;
use crate::checksum::adler32;
use crate::compression::deflater::{self, Deflater};
use crate::compression::inflater::{Inflater, OutputSize};
use crate::io::bit_writer::BitWriter;

fn is_fcheck_valid(data: &[u8]) -> bool {
    (((data[0] as u16) << 8) | data[1] as u16) % 31 == 0
}

fn is_deflate(data: &[u8]) -> bool {
    data[0] & 0xF == 8
}

fn is_window_size_valid(data: &[u8]) -> bool {
    data[0] >> 4 <= 7
}

fn has_preset_dictionary(data: &[u8]) -> bool {
    data[1] & 0x20 == 0x20
}

fn read_adler32_checksum(data: &[u8]) -> Result<u32, Error> {
    let Some(bytes) = data.first_chunk::<4>() else {
        return Err(Error::UnexpectedEndOfInput);
    };

    Ok(u32::from_be_bytes(*bytes))
}

pub(crate) fn check_header(header: &[u8; 2]) -> Result<(), Error> {
    if !is_deflate(header) {
        return Err(Error::UnsupportedCompressionMethod);
    }

    if !is_window_size_valid(header) {
        return Err(Error::InvalidWindowSize);
    }

    if !is_fcheck_valid(header) {
        return Err(Error::InvalidFcheck);
    }

    if has_preset_dictionary(header) {
        return Err(Error::PresetDictionary);
    }

    Ok(())
}

pub(crate) fn inflate_into(
    inflater: &mut Inflater,
    data: &[u8],
    out: &mut Vec<u8>,
    size: OutputSize,
) -> Result<usize, Error> {
    let Some(header) = data.first_chunk::<2>() else {
        return Err(Error::UnexpectedEndOfInput);
    };
    check_header(header)?;

    let stream_start = out.len();
    let body = &data[2..];

    let inflated = inflater.inflate_into(body, out, size)?;

    let checksum = match read_adler32_checksum(&body[inflated.input_used..]) {
        Ok(checksum) => checksum,
        Err(error) => {
            out.truncate(stream_start);
            return Err(error);
        }
    };

    if checksum != adler32::compute_adler32(&out[stream_start..]) {
        out.truncate(stream_start);
        return Err(Error::ChecksumMismatch);
    }

    Ok(inflated.output_added)
}

pub(crate) fn deflate(deflater: &mut Deflater, data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::with_capacity(2 + deflater::output_bound(data.len()) + 4);

    write_header(&mut writer);
    deflater.deflate_into(data, &mut writer);

    writer.align_to_byte();
    writer.write_bytes(&adler32::compute_adler32(data).to_be_bytes());

    writer.finish()
}

fn write_header(writer: &mut BitWriter) {
    writer.write_bits(0x78, 8); // CMF = 8, CINFO = 7
    writer.write_bits(1, 8); // 01 for now
}
