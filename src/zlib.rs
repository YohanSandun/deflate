use crate::Error;
use crate::checksum::adler32;
use crate::compression::inflater::Inflater;

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

pub(crate) fn inflate(inflater: &mut Inflater, data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut inflated_data = Vec::new();
    inflate_into(inflater, data, &mut inflated_data)?;
    Ok(inflated_data)
}

pub(crate) fn inflate_into(
    inflater: &mut Inflater,
    data: &[u8],
    out: &mut Vec<u8>,
) -> Result<usize, Error> {
    if data.len() < 3 {
        return Err(Error::UnexpectedEndOfInput);
    }

    if !is_deflate(data) {
        return Err(Error::UnsupportedCompressionMethod);
    }

    if !is_window_size_valid(data) {
        return Err(Error::InvalidWindowSize);
    }

    if !is_fcheck_valid(data) {
        return Err(Error::InvalidFcheck);
    }

    if has_preset_dictionary(data) {
        return Err(Error::PresetDictionary);
    }

    let stream_start = out.len();
    let body = &data[2..];

    let inflated = inflater.inflate_into(body, out)?;

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
