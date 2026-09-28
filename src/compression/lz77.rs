pub(crate) const MIN_MATCH: usize = 3;
pub(crate) const MAX_MATCH: usize = 258;
pub(crate) const MAX_DISTANCE: usize = 32_768;
const WINDOW_MASK: usize = MAX_DISTANCE - 1;
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const EMPTY: u32 = u32::MAX - MAX_DISTANCE as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Token {
    Literal(u8),
    Match { length: u16, distance: u16 },
}

impl Token {
    /// How many bytes of input the token stands for.
    #[inline]
    pub(crate) fn byte_len(self) -> usize {
        match self {
            Token::Literal(_) => 1,
            Token::Match { length, .. } => length as usize,
        }
    }
}

#[derive(Clone, Copy)]
struct Config {
    max_chain: u16,
    nice_length: u16,
}

const GREEDY_CONFIGS: [Config; 3] = [
    Config {
        max_chain: 4,
        nice_length: 8,
    },
    Config {
        max_chain: 8,
        nice_length: 16,
    },
    Config {
        max_chain: 32,
        nice_length: 32,
    },
];

#[derive(Clone, Copy)]
struct LazyConfig {
    search: Config,
    good_length: u16,
    max_lazy: u16,
}

const fn lazy_config(good_length: u16, max_lazy: u16, nice_length: u16, max_chain: u16) -> LazyConfig {
    LazyConfig {
        search: Config {
            max_chain,
            nice_length,
        },
        good_length,
        max_lazy,
    }
}

const LAZY_CONFIGS: [LazyConfig; 6] = [
    lazy_config(4, 4, 16, 16),
    lazy_config(8, 16, 32, 32),
    lazy_config(8, 16, 128, 128),
    lazy_config(8, 32, 128, 256),
    lazy_config(32, 128, 258, 1024),
    lazy_config(32, 258, 258, 4096),
];

const TOO_FAR: usize = 4096;

/// Finds repeated data and turns the input into `Token`s.
pub(crate) struct MatchFinder {
    head: Vec<u32>,
    prev: Vec<u32>,
}

impl MatchFinder {
    pub(crate) fn new() -> Self {
        Self {
            head: Vec::new(),
            prev: Vec::new(),
        }
    }

    /// Clears `tokens` and fills it with the parse of `data`
    pub(crate) fn find_tokens(&mut self, data: &[u8], level: u8, tokens: &mut Vec<Token>) {
        tokens.clear();

        match level {
            0 => tokens.extend(data.iter().map(|&byte| Token::Literal(byte))),
            1..=3 => {
                self.reset();
                self.greedy(data, GREEDY_CONFIGS[level as usize - 1], tokens);
            }
            _ => {
                self.reset();
                self.lazy(data, LAZY_CONFIGS[level.min(9) as usize - 4], tokens);
            }
        }
    }

    fn reset(&mut self) {
        if self.head.is_empty() {
            self.head = vec![EMPTY; HASH_SIZE];
            self.prev = vec![EMPTY; MAX_DISTANCE];
        } else {
            self.head.fill(EMPTY);
        }
    }

    /// Greedy parsing: takes the longest match at each position, if there's one.
    fn greedy(&mut self, data: &[u8], config: Config, tokens: &mut Vec<Token>) {
        let hashable_end = data.len().saturating_sub(MIN_MATCH - 1);

        let mut pos = 0;
        while pos < hashable_end {
            let hash = hash3(data, pos);
            let (length, distance) = self.longest_match(data, pos, hash, 0, config);

            self.insert(pos, hash);

            if length >= MIN_MATCH {
                tokens.push(Token::Match {
                    length: length as u16,
                    distance: distance as u16,
                });

                let end = pos + length;
                for covered in pos + 1..end.min(hashable_end) {
                    self.insert(covered, hash3(data, covered));
                }
                pos = end;
            } else {
                tokens.push(Token::Literal(data[pos]));
                pos += 1;
            }
        }

        tokens.extend(data[pos..].iter().map(|&byte| Token::Literal(byte)));
    }

    /// Lazy parsing: a match is held back for one position, and dropped for a
    /// literal if the next position has a longer one.
    fn lazy(&mut self, data: &[u8], config: LazyConfig, tokens: &mut Vec<Token>) {
        let hashable_end = data.len().saturating_sub(MIN_MATCH - 1);
        let reduced = Config {
            max_chain: config.search.max_chain >> 2,
            ..config.search
        };

        let mut pending = false;
        let mut prev_length = 0;
        let mut prev_distance = 0;

        let mut pos = 0;
        while pos < hashable_end {
            let hash = hash3(data, pos);
            let (mut length, distance) = if prev_length < config.max_lazy as usize {
                let search = if prev_length >= config.good_length as usize {
                    reduced
                } else {
                    config.search
                };
                self.longest_match(data, pos, hash, prev_length, search)
            } else {
                (0, 0)
            };

            self.insert(pos, hash);

            if length == MIN_MATCH && distance > TOO_FAR {
                length = 0;
            }

            if prev_length >= MIN_MATCH && length <= prev_length {
                tokens.push(Token::Match {
                    length: prev_length as u16,
                    distance: prev_distance as u16,
                });

                let end = pos - 1 + prev_length;
                for covered in pos + 1..end.min(hashable_end) {
                    self.insert(covered, hash3(data, covered));
                }
                pos = end;
                pending = false;
                prev_length = 0;
            } else {
                if pending {
                    tokens.push(Token::Literal(data[pos - 1]));
                }
                pending = true;
                prev_length = length;
                prev_distance = distance;
                pos += 1;
            }
        }

        if pending {
            if prev_length >= MIN_MATCH {
                tokens.push(Token::Match {
                    length: prev_length as u16,
                    distance: prev_distance as u16,
                });
                pos = pos - 1 + prev_length;
            } else {
                pos -= 1;
            }
        }
        tokens.extend(data[pos..].iter().map(|&byte| Token::Literal(byte)));
    }

    #[inline]
    fn longest_match(
        &self,
        data: &[u8],
        pos: usize,
        hash: usize,
        prev_length: usize,
        config: Config,
    ) -> (usize, usize) {
        let max_length = MAX_MATCH.min(data.len() - pos);
        let nice_length = (config.nice_length as usize).min(max_length);

        let mut best_length = prev_length.max(MIN_MATCH - 1);
        let mut best_distance = 0;
        if best_length >= max_length {
            return (0, 0);
        }

        let mut candidate = self.head[hash];
        for _ in 0..config.max_chain {
            let distance = (pos as u32).wrapping_sub(candidate) as usize;
            if distance.wrapping_sub(1) >= MAX_DISTANCE {
                break;
            }
            let start = pos - distance;

            if data[start + best_length] == data[pos + best_length] {
                let length = match_length(data, start, pos, max_length);
                if length > best_length {
                    best_length = length;
                    best_distance = distance;

                    if length >= nice_length {
                        break;
                    }
                }
            }

            candidate = self.prev[start & WINDOW_MASK];
        }

        (best_length, best_distance)
    }

    #[inline]
    fn insert(&mut self, pos: usize, hash: usize) {
        self.prev[pos & WINDOW_MASK] = self.head[hash];
        self.head[hash] = pos as u32;
    }
}

#[inline]
fn hash3(data: &[u8], pos: usize) -> usize {
    let bytes = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], 0]);
    (bytes.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

#[inline]
fn match_length(data: &[u8], earlier: usize, pos: usize, max: usize) -> usize {
    let mut length = 0;

    while length + 8 <= max {
        let diff = load_u64(data, earlier + length) ^ load_u64(data, pos + length);
        if diff != 0 {
            return length + (diff.trailing_zeros() / 8) as usize;
        }
        length += 8;
    }

    while length < max && data[earlier + length] == data[pos + length] {
        length += 1;
    }
    length
}

#[inline]
fn load_u64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(*data[at..].first_chunk::<8>().unwrap())
}

#[cfg(test)]
mod tests;
