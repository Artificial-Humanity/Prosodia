//! Phoneme and token chunking.
//!
//! StyleTTS2 has a fixed maximum input sequence length, so long passages must be
//! split into chunks that each stay under that limit before synthesis. This logic
//! previously lived in Swift (`ProsodiaActorPipeline.chunkPhonemes` /
//! `chunkTokens`); it is ported here so every platform shares one implementation.
//!
//! Counting note: the Swift originals measured length in `Character`s (grapheme
//! clusters). These ports count Unicode scalar values (`char`s) instead, which for
//! phoneme strings differs only when combining diacritics are present and is always
//! equal-or-more conservative — i.e. it never produces a chunk longer than Swift would.

use crate::g2p::TokenPhonemes;

/// Model sequence limit (512) minus the SOS/EOS sentinels reserved by the tokenizer.
pub const DEFAULT_CHUNK_LIMIT: u32 = 510;

/// Characters we prefer to break a phoneme chunk on, so splits land on natural
/// prosodic boundaries rather than mid-word.
const BREAK_CHARS: [char; 9] = [' ', '.', ',', ';', ':', '!', '?', '—', '…'];

/// One group of tokens whose combined phoneme+whitespace length fits within the limit.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TokenChunk {
    pub tokens: Vec<TokenPhonemes>,
}

/// Split a phoneme string into chunks of at most `limit` scalars each.
///
/// When a hard cut at `limit` would fall mid-word, the split is pulled back to the
/// most recent break character past the halfway point; if none is found within that
/// window the chunk is cut at `limit`. Each returned chunk is whitespace-trimmed and
/// empty chunks are dropped.
#[uniffi::export]
pub fn chunk_phonemes(phonemes: String, limit: u32) -> Vec<String> {
    let limit = limit as usize;
    let trimmed = phonemes.trim();
    let chars: Vec<char> = trimmed.chars().collect();

    if chars.len() <= limit {
        return if chars.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_string()]
        };
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let end_limit = (start + limit).min(chars.len());

        // Final chunk: take the remainder as-is.
        if end_limit == chars.len() {
            chunks.push(collect_trimmed(&chars[start..end_limit]));
            break;
        }

        // Walk backward from the limit looking for a break character, but don't
        // retreat past the halfway point of this chunk.
        let mut split_index = end_limit;
        let mut cursor = end_limit - 1;
        while cursor > start + limit / 2 {
            if BREAK_CHARS.contains(&chars[cursor]) {
                split_index = cursor + 1;
                break;
            }
            cursor -= 1;
        }

        chunks.push(collect_trimmed(&chars[start..split_index]));

        start = split_index;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }

    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

/// Group a token stream into chunks whose combined phoneme+whitespace length stays
/// within `limit`. A single token longer than `limit` becomes its own chunk (the
/// limit is a soft target, never a reason to drop content).
#[uniffi::export]
pub fn chunk_tokens(tokens: Vec<TokenPhonemes>, limit: u32) -> Vec<TokenChunk> {
    let limit = limit as usize;
    let mut chunks: Vec<TokenChunk> = Vec::new();
    let mut current: Vec<TokenPhonemes> = Vec::new();
    let mut current_len = 0usize;

    for token in tokens {
        let token_len = token.phonemes.chars().count() + token.whitespace.chars().count();
        if current_len + token_len > limit && !current.is_empty() {
            chunks.push(TokenChunk {
                tokens: std::mem::take(&mut current),
            });
            current_len = token_len;
            current.push(token);
        } else {
            current_len += token_len;
            current.push(token);
        }
    }

    if !current.is_empty() {
        chunks.push(TokenChunk { tokens: current });
    }

    chunks
}

/// Collect a char slice into a String and trim surrounding whitespace.
fn collect_trimmed(chars: &[char]) -> String {
    let s: String = chars.iter().collect();
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(phonemes: &str, whitespace: &str) -> TokenPhonemes {
        TokenPhonemes {
            phonemes: phonemes.to_string(),
            whitespace: whitespace.to_string(),
        }
    }

    #[test]
    fn short_phonemes_return_single_chunk() {
        assert_eq!(
            chunk_phonemes("hɛˈloʊ wɝˈld".to_string(), DEFAULT_CHUNK_LIMIT),
            vec!["hɛˈloʊ wɝˈld".to_string()]
        );
    }

    #[test]
    fn empty_phonemes_return_no_chunks() {
        assert!(chunk_phonemes(String::new(), DEFAULT_CHUNK_LIMIT).is_empty());
        assert!(chunk_phonemes("   \n  ".to_string(), DEFAULT_CHUNK_LIMIT).is_empty());
    }

    #[test]
    fn input_is_trimmed_before_chunking() {
        assert_eq!(
            chunk_phonemes("  hello  ".to_string(), DEFAULT_CHUNK_LIMIT),
            vec!["hello".to_string()]
        );
    }

    #[test]
    fn long_input_splits_on_break_characters() {
        // 8 words of 10 chars each joined by spaces, chunked at a small limit so the
        // splitter must break on spaces.
        let word = "abcdefghij"; // 10 scalars
        let phonemes = vec![word; 8].join(" "); // "ab...j ab...j ..." len = 8*10 + 7 = 87
        let chunks = chunk_phonemes(phonemes.clone(), 25);

        // No chunk exceeds the limit.
        for c in &chunks {
            assert!(c.chars().count() <= 25, "chunk too long: {c:?}");
        }
        // Splits land on whole words (no word is cut apart), and concatenating the
        // chunks' words reproduces the original word sequence.
        let rejoined: Vec<&str> = chunks.iter().flat_map(|c| c.split(' ')).collect();
        assert_eq!(rejoined, vec![word; 8]);
    }

    #[test]
    fn hard_split_when_no_break_character_available() {
        // A single run with no break characters must still be cut at the limit.
        let phonemes = "x".repeat(1200);
        let chunks = chunk_phonemes(phonemes, 510);
        assert_eq!(chunks.len(), 3); // 510 + 510 + 180
        assert_eq!(chunks[0].chars().count(), 510);
        assert_eq!(chunks[1].chars().count(), 510);
        assert_eq!(chunks[2].chars().count(), 180);
    }

    #[test]
    fn tokens_group_under_limit() {
        // Each token contributes phonemes+whitespace length. Limit chosen so two
        // tokens fit but a third tips over.
        let tokens = vec![
            tok("abc", " "),  // len 4
            tok("def", " "),  // len 4 -> running 8
            tok("ghi", " "),  // len 4 -> would be 12 > 10, new chunk
            tok("jkl", ""),   // len 3 -> running 7
        ];
        let chunks = chunk_tokens(tokens, 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].tokens, vec![tok("abc", " "), tok("def", " ")]);
        assert_eq!(chunks[1].tokens, vec![tok("ghi", " "), tok("jkl", "")]);
    }

    #[test]
    fn single_oversized_token_becomes_its_own_chunk() {
        let tokens = vec![
            tok("aaaaaaaaaaaa", " "), // len 13 > limit on its own
            tok("b", ""),
        ];
        let chunks = chunk_tokens(tokens, 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].tokens.len(), 1);
        assert_eq!(chunks[1].tokens, vec![tok("b", "")]);
    }

    #[test]
    fn empty_token_stream_yields_no_chunks() {
        assert!(chunk_tokens(Vec::new(), DEFAULT_CHUNK_LIMIT).is_empty());
    }
}
