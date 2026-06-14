use std::collections::HashMap;
use std::convert::TryInto;

#[derive(Debug, thiserror::Error)]
pub enum BpeError {
    #[error("Invalid file size for .pvocab format")]
    InvalidSize,
    #[error("Invalid magic header: expected 'PVOC', got {0:?}")]
    InvalidMagic(Vec<u8>),
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u16),
    #[error("Malformed vocabulary section")]
    MalformedVocab,
    #[error("Malformed token string in vocabulary")]
    MalformedToken,
    #[error("Invalid UTF-8 in vocabulary: {0}")]
    InvalidUtf8Vocab(#[from] std::str::Utf8Error),
    #[error("Malformed merges section")]
    MalformedMerges,
    #[error("Malformed merge rule part 1")]
    MalformedMergePart1,
    #[error("Malformed merge rule part 2 length")]
    MalformedMergePart2Length,
    #[error("Malformed merge rule part 2")]
    MalformedMergePart2,
    #[error("Invalid regex pattern: {0}")]
    InvalidRegex(#[from] regex::Error),
}

pub struct BpeTokenizer {
    pub vocab: HashMap<String, i32>,
    pub inverse_vocab: HashMap<i32, String>,
    pub merges: HashMap<(String, String), usize>,
    pub byte_level: bool,
    pub unknown_token_id: Option<i32>,
    byte_to_unicode: HashMap<u8, char>,
    unicode_to_byte: HashMap<char, u8>,
    regex: regex::Regex,
}

impl BpeTokenizer {
    pub fn new(data: &[u8], byte_level: bool, unknown_token_id: Option<i32>) -> Result<Self, BpeError> {
        if data.len() < 14 {
            return Err(BpeError::InvalidSize);
        }

        let magic = &data[0..4];
        if magic != b"PVOC" {
            return Err(BpeError::InvalidMagic(magic.to_vec()));
        }

        let version = u16::from_le_bytes(data[4..6].try_into().unwrap());
        if version != 1 {
            return Err(BpeError::UnsupportedVersion(version));
        }

        let vocab_count = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
        let merges_count = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;

        let mut offset = 14;
        let mut vocab = HashMap::with_capacity(vocab_count);
        let mut inverse_vocab = HashMap::with_capacity(vocab_count);

        // Parse Vocab Section
        for _ in 0..vocab_count {
            if offset + 6 > data.len() {
                return Err(BpeError::MalformedVocab);
            }
            let token_id = i32::from_le_bytes(data[offset..offset+4].try_into().unwrap());
            offset += 4;
            let len = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;

            if offset + len > data.len() {
                return Err(BpeError::MalformedToken);
            }
            let token_bytes = &data[offset..offset+len];
            offset += len;

            let token_str = std::str::from_utf8(token_bytes)?.to_string();
            vocab.insert(token_str.clone(), token_id);
            inverse_vocab.insert(token_id, token_str);
        }

        // Parse Merges Section
        let mut merges = HashMap::with_capacity(merges_count);
        for _ in 0..merges_count {
            if offset + 8 > data.len() {
                return Err(BpeError::MalformedMerges);
            }
            let rank = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;

            let len1 = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;
            if offset + len1 > data.len() {
                return Err(BpeError::MalformedMergePart1);
            }
            let first_bytes = &data[offset..offset+len1];
            offset += len1;
            let first_str = std::str::from_utf8(first_bytes)?.to_string();

            if offset + 2 > data.len() {
                return Err(BpeError::MalformedMergePart2Length);
            }
            let len2 = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;
            if offset + len2 > data.len() {
                return Err(BpeError::MalformedMergePart2);
            }
            let second_bytes = &data[offset..offset+len2];
            offset += len2;
            let second_str = std::str::from_utf8(second_bytes)?.to_string();

            merges.insert((first_str, second_str), rank);
        }

        let (byte_to_unicode, unicode_to_byte) = Self::create_byte_to_unicode_maps();

        // Standard GPT-2 / Qwen BPE pre-tokenization regex
        let regex = regex::Regex::new(r"'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+$|\s+")?;

        Ok(Self {
            vocab,
            inverse_vocab,
            merges,
            byte_level,
            unknown_token_id,
            byte_to_unicode,
            unicode_to_byte,
            regex,
        })
    }

    pub fn encode(&self, text: &str) -> Vec<i32> {
        if text.is_empty() {
            return Vec::new();
        }

        let words = self.pre_tokenize(text);
        let mut token_ids = Vec::new();

        for word in words {
            if self.byte_level {
                let mut mapped_word = String::new();
                for &byte in word.as_bytes() {
                    if let Some(&c) = self.byte_to_unicode.get(&byte) {
                        mapped_word.push(c);
                    }
                }

                let subwords = self.bpe(&mapped_word);
                for subword in subwords {
                    if let Some(&id) = self.vocab.get(&subword) {
                        token_ids.push(id);
                    } else if let Some(unk_id) = self.unknown_token_id {
                        token_ids.push(unk_id);
                    }
                }
            } else {
                let subwords = self.bpe(&word);
                for subword in subwords {
                    if let Some(&id) = self.vocab.get(&subword) {
                        token_ids.push(id);
                    } else if let Some(unk_id) = self.unknown_token_id {
                        token_ids.push(unk_id);
                    }
                }
            }
        }

        token_ids
    }

    pub fn decode(&self, tokens: &[i32]) -> String {
        let mut joined_string = String::new();
        for &token in tokens {
            if let Some(token_str) = self.inverse_vocab.get(&token) {
                joined_string.push_str(token_str);
            }
        }

        if self.byte_level {
            let mut bytes = Vec::new();
            for c in joined_string.chars() {
                if let Some(&byte) = self.unicode_to_byte.get(&c) {
                    bytes.push(byte);
                } else {
                    bytes.extend_from_slice(c.to_string().as_bytes());
                }
            }
            String::from_utf8(bytes).unwrap_or(joined_string)
        } else {
            joined_string
        }
    }

    fn pre_tokenize(&self, text: &str) -> Vec<String> {
        self.regex
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect()
    }

    fn bpe(&self, word: &str) -> Vec<String> {
        let mut parts: Vec<String> = word.chars().map(|c| c.to_string()).collect();
        if parts.is_empty() {
            return Vec::new();
        }

        loop {
            let mut best_pair = None;
            let mut min_rank = usize::MAX;

            for i in 0..(parts.len() - 1) {
                let pair = (parts[i].clone(), parts[i + 1].clone());
                if let Some(&rank) = self.merges.get(&pair) {
                    if rank < min_rank {
                        min_rank = rank;
                        best_pair = Some(pair);
                    }
                }
            }

            let pair_to_merge = match best_pair {
                Some(p) => p,
                None => break,
            };

            let mut new_parts = Vec::new();
            let mut i = 0;
            while i < parts.len() {
                if i < parts.len() - 1 && parts[i] == pair_to_merge.0 && parts[i + 1] == pair_to_merge.1 {
                    new_parts.push(format!("{}{}", pair_to_merge.0, pair_to_merge.1));
                    i += 2;
                } else {
                    new_parts.push(parts[i].clone());
                    i += 1;
                }
            }
            parts = new_parts;
        }

        parts
    }

    fn create_byte_to_unicode_maps() -> (HashMap<u8, char>, HashMap<char, u8>) {
        let mut bs = Vec::new();
        for b in 33..=126 {
            bs.push(b as u8);
        }
        for b in 161..=172 {
            bs.push(b as u8);
        }
        for b in 174..=255 {
            bs.push(b as u8);
        }

        let mut cs: Vec<u32> = bs.iter().map(|&x| x as u32).collect();
        let mut n = 0;
        for b in 0..=255 {
            if !bs.contains(&(b as u8)) {
                bs.push(b as u8);
                cs.push(256 + n);
                n += 1;
            }
        }

        let mut byte_to_unicode = HashMap::new();
        let mut unicode_to_byte = HashMap::new();

        for (&b, &c_val) in bs.iter().zip(cs.iter()) {
            if let Some(c) = char::from_u32(c_val) {
                byte_to_unicode.insert(b, c);
                unicode_to_byte.insert(c, b);
            }
        }

        (byte_to_unicode, unicode_to_byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpe_tokenizer_basic() {
        // Construct a mock .pvocab file in memory
        let mut data = Vec::new();
        data.extend_from_slice(b"PVOC"); // magic
        data.extend_from_slice(&1u16.to_le_bytes()); // version
        data.extend_from_slice(&4u32.to_le_bytes()); // vocab count
        data.extend_from_slice(&2u32.to_le_bytes()); // merges count

        // Vocab item 0: "h" -> 0
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"h");

        // Vocab item 1: "e" -> 1
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"e");

        // Vocab item 2: "l" -> 2
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"l");

        // Vocab item 3: "he" -> 3
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(b"he");

        // Merge item 0: "h" + "e" -> rank 0
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"h");
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"e");

        // Merge item 1: "l" + "l" -> rank 1
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"l");
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(b"l");

        let tokenizer = BpeTokenizer::new(&data, false, None).unwrap();
        assert_eq!(tokenizer.vocab.get("he"), Some(&3));
        assert_eq!(tokenizer.inverse_vocab.get(&3), Some(&"he".to_string()));
        assert_eq!(tokenizer.merges.get(&("h".to_string(), "e".to_string())), Some(&0));

        // Test BPE merging: "he" -> merges "h" and "e" into "he"
        let encoded = tokenizer.encode("he");
        assert_eq!(encoded, vec![3]);

        let decoded = tokenizer.decode(&encoded);
        assert_eq!(decoded, "he");
    }
}
