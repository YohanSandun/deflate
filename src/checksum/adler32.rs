const MOD: u32 = 65521;
const NMAX: usize = 5552;

pub(crate) fn compute_adler32(data: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;

    for chunk in data.chunks(NMAX) {
        let mut blocks = chunk.chunks_exact(8);

        for block in &mut blocks {
            let b: [u32; 8] = std::array::from_fn(|i| u32::from(block[i]));

            s2 += 8 * s1
                + 8 * b[0]
                + 7 * b[1]
                + 6 * b[2]
                + 5 * b[3]
                + 4 * b[4]
                + 3 * b[5]
                + 2 * b[6]
                + b[7];
            s1 += b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + b[7];
        }

        for &byte in blocks.remainder() {
            s1 += u32::from(byte);
            s2 += s1;
        }

        s1 %= MOD;
        s2 %= MOD;
    }

    (s2 << 16) | s1
}

#[cfg(test)]
mod tests;
