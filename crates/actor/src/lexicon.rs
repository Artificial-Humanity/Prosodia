use std::collections::{HashMap, HashSet};
use once_cell::sync::Lazy;
use serde::Deserialize;
use regex::Regex;
use crate::tagger::InternalMToken;

pub const DIPHTHONGS: &str = "AIOQWYʤʧ";
pub const SUBTOKEN_JUNKS: &str = "',-._‘’/";
pub const PUNCTS: &str = ";:,.!?—…\"“”";
pub const NON_QUOTE_PUNCTS: &str = ";:,.!?—…"; // puncts without quote chars
pub const CONSONANTS: &str = "bdfhjklmnpstvwzðŋɡɹɾʃʒʤʧθ";
pub const VOWELS: &str = "AIOQWYaiuæɑɒɔəɛɜɪʊʌᵻ";
pub const US_TAUS: &str = "AIOWYiuæɑəɛɪɹʊʌ";
pub const STRESSES: &str = "ˌˈ";
pub const PRIMARY_STRESS: char = 'ˈ';
pub const SECONDARY_STRESS: char = 'ˌ';

const _US_VOCAB: &str = "AIOWYbdfhijklmnpstuvwzæðŋɑɔəɛɜɡɪɹɾʃʊʌʒʤʧˈˌθᵊᵻʔ";
const _GB_VOCAB: &str = "AIQWYabdfhijklmnpstuvwzðŋɑɒɔəɛɜɡɪɹʃʊʌʒʤʧˈˌːθᵊ";

const US_GOLD_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/us_gold.bin"));
const GB_GOLD_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gb_gold.bin"));
const US_SILVER_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/us_silver.bin"));
const GB_SILVER_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gb_silver.bin"));

#[derive(Clone, Debug)]
pub struct TokenContext {
    pub future_vowel: Option<bool>,
    pub future_to: bool,
}

impl Default for TokenContext {
    fn default() -> Self {
        Self {
            future_vowel: None,
            future_to: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BinSilverMap {
    data: &'static [u8],
}

impl BinSilverMap {
    pub const fn new(data: &'static [u8]) -> Self {
        Self { data }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.data.len() < 12 {
            return Err("Binary too small for PSL1 header");
        }
        if &self.data[0..4] != b"PSL1" {
            return Err("Invalid PSL1 magic header");
        }
        let num_entries = u32::from_le_bytes(self.data[4..8].try_into().unwrap()) as usize;
        let pool_size = u32::from_le_bytes(self.data[8..12].try_into().unwrap()) as usize;
        let index_start = 12;
        let string_pool_start = index_start + num_entries * 12;
        let expected_len = string_pool_start + pool_size;
        if self.data.len() != expected_len {
            return Err("PSL1 size mismatch");
        }
        Ok(())
    }

    pub fn get(&self, query: &str) -> Option<&'static str> {
        let num_entries = u32::from_le_bytes(self.data[4..8].try_into().unwrap()) as usize;
        let index_start = 12;
        let string_pool_start = index_start + num_entries * 12;
        let string_pool = &self.data[string_pool_start..];

        let mut low = 0;
        let mut high = num_entries;
        while low < high {
            let mid = (low + high) / 2;
            let offset = index_start + mid * 12;
            let key_offset = u32::from_le_bytes(self.data[offset..offset+4].try_into().unwrap()) as usize;
            let key_len = u16::from_le_bytes(self.data[offset+4..offset+6].try_into().unwrap()) as usize;
            let key_bytes = &string_pool[key_offset..key_offset + key_len];
            let key_str = std::str::from_utf8(key_bytes).unwrap();

            match query.cmp(key_str) {
                std::cmp::Ordering::Equal => {
                    let val_offset = u32::from_le_bytes(self.data[offset+6..offset+10].try_into().unwrap()) as usize;
                    let val_len = u16::from_le_bytes(self.data[offset+10..offset+12].try_into().unwrap()) as usize;
                    let val_bytes = &string_pool[val_offset..val_offset + val_len];
                    return Some(std::str::from_utf8(val_bytes).unwrap());
                }
                std::cmp::Ordering::Less => {
                    high = mid;
                }
                std::cmp::Ordering::Greater => {
                    low = mid + 1;
                }
            }
        }
        None
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

#[derive(Clone, Copy)]
pub struct BinGoldMap {
    data: &'static [u8],
}

#[derive(Clone, Debug, PartialEq)]
pub enum BinLexiconValue {
    Single(&'static str),
    Variants(BinVariants),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BinVariants {
    data: &'static [u8],
    offset: usize,
    len: usize,
    string_pool: &'static [u8],
}

impl BinVariants {
    pub fn get(&self, query_tag: &str) -> Option<Option<&'static str>> {
        for i in 0..self.len {
            let item_offset = self.offset + i * 13;
            let tag_offset = u32::from_le_bytes(self.data[item_offset..item_offset+4].try_into().unwrap()) as usize;
            let tag_len = u16::from_le_bytes(self.data[item_offset+4..item_offset+6].try_into().unwrap()) as usize;
            let tag_bytes = &self.string_pool[tag_offset..tag_offset + tag_len];
            let tag_str = std::str::from_utf8(tag_bytes).unwrap();

            if tag_str == query_tag {
                let has_val = self.data[item_offset+6];
                if has_val == 0 {
                    return Some(None);
                } else {
                    let val_offset = u32::from_le_bytes(self.data[item_offset+7..item_offset+11].try_into().unwrap()) as usize;
                    let val_len = u16::from_le_bytes(self.data[item_offset+11..item_offset+13].try_into().unwrap()) as usize;
                    let val_bytes = &self.string_pool[val_offset..val_offset + val_len];
                    return Some(Some(std::str::from_utf8(val_bytes).unwrap()));
                }
            }
        }
        None
    }

    pub fn contains_key(&self, tag: &str) -> bool {
        self.get(tag).is_some()
    }
}

impl BinGoldMap {
    pub const fn new(data: &'static [u8]) -> Self {
        Self { data }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.data.len() < 16 {
            return Err("Binary too small for PGL1 header");
        }
        if &self.data[0..4] != b"PGL1" {
            return Err("Invalid PGL1 magic header");
        }
        let num_entries = u32::from_le_bytes(self.data[4..8].try_into().unwrap()) as usize;
        let pool_size = u32::from_le_bytes(self.data[8..12].try_into().unwrap()) as usize;
        let val_pool_size = u32::from_le_bytes(self.data[12..16].try_into().unwrap()) as usize;
        
        let index_start = 16;
        let string_pool_start = index_start + num_entries * 14;
        let values_pool_start = string_pool_start + pool_size;
        let expected_len = values_pool_start + val_pool_size;
        if self.data.len() != expected_len {
            return Err("PGL1 size mismatch");
        }
        Ok(())
    }

    pub fn get(&self, query: &str) -> Option<BinLexiconValue> {
        let num_entries = u32::from_le_bytes(self.data[4..8].try_into().unwrap()) as usize;
        let pool_size = u32::from_le_bytes(self.data[8..12].try_into().unwrap()) as usize;
        let val_pool_size = u32::from_le_bytes(self.data[12..16].try_into().unwrap()) as usize;

        let index_start = 16;
        let string_pool_start = index_start + num_entries * 14;
        let values_pool_start = string_pool_start + pool_size;

        let string_pool = &self.data[string_pool_start..string_pool_start + pool_size];
        let values_pool = &self.data[values_pool_start..values_pool_start + val_pool_size];

        let mut low = 0;
        let mut high = num_entries;
        while low < high {
            let mid = (low + high) / 2;
            let offset = index_start + mid * 14;
            let key_offset = u32::from_le_bytes(self.data[offset..offset+4].try_into().unwrap()) as usize;
            let key_len = u16::from_le_bytes(self.data[offset+4..offset+6].try_into().unwrap()) as usize;
            let key_bytes = &string_pool[key_offset..key_offset + key_len];
            let key_str = std::str::from_utf8(key_bytes).unwrap();

            match query.cmp(key_str) {
                std::cmp::Ordering::Equal => {
                    let val_type = self.data[offset+6];
                    let val_data_offset = u32::from_le_bytes(self.data[offset+8..offset+12].try_into().unwrap()) as usize;
                    let val_data_len = u16::from_le_bytes(self.data[offset+12..offset+14].try_into().unwrap()) as usize;

                    if val_type == 0 {
                        let val_bytes = &string_pool[val_data_offset..val_data_offset + val_data_len];
                        return Some(BinLexiconValue::Single(std::str::from_utf8(val_bytes).unwrap()));
                    } else {
                        return Some(BinLexiconValue::Variants(BinVariants {
                            data: values_pool,
                            offset: val_data_offset,
                            len: val_data_len as usize,
                            string_pool,
                        }));
                    }
                }
                std::cmp::Ordering::Less => {
                    high = mid;
                }
                std::cmp::Ordering::Greater => {
                    low = mid + 1;
                }
            }
        }
        None
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

pub struct Lexicon {
    pub british: bool,
    pub cap_stresses: (f64, f64),
    golds: BinGoldMap,
    silvers: BinSilverMap,
    currencies: HashMap<String, (&'static str, &'static str)>,
    ordinals: HashSet<&'static str>,
    add_symbols: HashMap<String, &'static str>,
    symbols: HashMap<String, &'static str>,
}

impl Lexicon {
    pub fn new(british: bool) -> Self {
        let golds = if british { BinGoldMap::new(GB_GOLD_BIN) } else { BinGoldMap::new(US_GOLD_BIN) };
        let silvers = if british { BinSilverMap::new(GB_SILVER_BIN) } else { BinSilverMap::new(US_SILVER_BIN) };

        golds.validate().expect("GB/US Gold Lexicon binary is corrupt or invalid");
        silvers.validate().expect("GB/US Silver Lexicon binary is corrupt or invalid");

        let mut currencies = HashMap::new();
        currencies.insert("$".to_string(), ("dollar", "cent"));
        currencies.insert("£".to_string(), ("pound", "pence"));
        currencies.insert("€".to_string(), ("euro", "cent"));

        let mut ordinals = HashSet::new();
        ordinals.insert("st");
        ordinals.insert("nd");
        ordinals.insert("rd");
        ordinals.insert("th");

        let mut add_symbols = HashMap::new();
        add_symbols.insert(".".to_string(), "dot");
        add_symbols.insert("/".to_string(), "slash");

        let mut symbols = HashMap::new();
        symbols.insert("%".to_string(), "percent");
        symbols.insert("&".to_string(), "and");
        symbols.insert("+".to_string(), "plus");
        symbols.insert("@".to_string(), "at");

        Self {
            british,
            cap_stresses: (0.5, 2.0),
            golds,
            silvers,
            currencies,
            ordinals,
            add_symbols,
            symbols,
        }
    }

    pub fn parent_tag(tag: Option<&str>) -> Option<String> {
        let tag = tag?;
        if tag.starts_with("VB") {
            return Some("VERB".to_string());
        }
        if tag.starts_with("NN") {
            return Some("NOUN".to_string());
        }
        if tag.starts_with("ADV") || tag.starts_with("RB") {
            return Some("ADV".to_string());
        }
        if tag.starts_with("ADJ") || tag.starts_with("JJ") {
            return Some("ADJ".to_string());
        }
        Some(tag.to_string())
    }

    pub fn stress_weight(phonemes: Option<&str>) -> i32 {
        match phonemes {
            None => 0,
            Some(ph) => ph.chars().fold(0, |acc, c| {
                acc + if DIPHTHONGS.contains(c) { 2 } else { 1 }
            }),
        }
    }

    pub fn gold_string(&self, key: &str) -> Option<String> {
        match self.golds.get(key) {
            Some(BinLexiconValue::Single(value)) => Some(value.to_string()),
            Some(BinLexiconValue::Variants(variants)) => {
                variants.get("DEFAULT").and_then(|opt| opt.map(|s| s.to_string()))
            }
            None => None,
        }
    }
}

pub fn apply_stress(phonemes: Option<String>, stress: Option<f64>) -> Option<String> {
    let phonemes = phonemes?;
    let stress = match stress {
        Some(s) => s,
        None => return Some(phonemes),
    };

    fn restress(phoneme_str: &str) -> String {
        let chars: Vec<char> = phoneme_str.chars().collect();
        let mut indexed: Vec<(f64, char)> = chars.iter().enumerate().map(|(i, &c)| (i as f64, c)).collect();
        for i in 0..chars.len() {
            if STRESSES.contains(chars[i]) {
                if let Some(vowel_offset) = chars[i..].iter().position(|&c| VOWELS.contains(c)) {
                    let vowel_index = i + vowel_offset;
                    indexed[i].0 = (vowel_index as f64) - 0.5;
                }
            }
        }
        indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        indexed.iter().map(|&(_, c)| c).collect()
    }

    if stress < -1.0 {
        return Some(phonemes.chars().filter(|&c| c != PRIMARY_STRESS && c != SECONDARY_STRESS).collect());
    }

    if stress == -1.0 || ((stress == 0.0 || stress == -0.5) && phonemes.contains(PRIMARY_STRESS)) {
        return Some(phonemes
            .replace(SECONDARY_STRESS, "")
            .replace(PRIMARY_STRESS, &SECONDARY_STRESS.to_string()));
    }

    if stress == 0.0 || stress == 0.5 || stress == 1.0 {
        if !phonemes.chars().any(|c| STRESSES.contains(c)) {
            if !phonemes.chars().any(|c| VOWELS.contains(c)) {
                return Some(phonemes);
            }
            return Some(restress(&format!("{}{}", SECONDARY_STRESS, phonemes)));
        }
        return Some(phonemes);
    }

    if stress >= 1.0 && !phonemes.contains(PRIMARY_STRESS) && phonemes.contains(SECONDARY_STRESS) {
        return Some(phonemes.replace(SECONDARY_STRESS, &PRIMARY_STRESS.to_string()));
    }

    if stress > 1.0 && !phonemes.chars().any(|c| STRESSES.contains(c)) {
        if !phonemes.chars().any(|c| VOWELS.contains(c)) {
            return Some(phonemes);
        }
        return Some(restress(&format!("{}{}", PRIMARY_STRESS, phonemes)));
    }

    Some(phonemes)
}

fn split_lowercase_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_alphabetic() {
            current.push(character);
        } else if !current.is_empty() {
            words.push(current);
            current = String::new();
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn is_all_uppercase(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_uppercase())
}

impl Lexicon {
    pub fn plural_suffix(&self, stem: Option<String>) -> Option<String> {
        let stem = stem?;
        let last = stem.chars().last()?;
        if "ptkfθ".contains(last) {
            return Some(stem + "s");
        }
        if "szʃʒʧʤ".contains(last) {
            let mid = if self.british { 'ɪ' } else { 'ᵻ' };
            return Some(format!("{}{}z", stem, mid));
        }
        Some(stem + "z")
    }

    pub fn ed_suffix(&self, stem: Option<String>) -> Option<String> {
        let stem = stem?;
        let last = stem.chars().last()?;
        if "pkfθʃsʧ".contains(last) {
            return Some(stem + "t");
        }
        if last == 'd' {
            let mid = if self.british { 'ɪ' } else { 'ᵻ' };
            return Some(format!("{}{}d", stem, mid));
        }
        if last != 't' {
            return Some(stem + "d");
        }
        if self.british || stem.chars().count() < 2 {
            return Some(stem + "ɪd");
        }
        let characters: Vec<char> = stem.chars().collect();
        if characters.len() >= 2 && US_TAUS.contains(characters[characters.len() - 2]) {
            return Some(characters[..characters.len() - 1].iter().collect::<String>() + "ɾᵻd");
        }
        Some(stem + "ᵻd")
    }

    pub fn ing_suffix(&self, stem: Option<String>) -> Option<String> {
        let stem = stem?;
        let last = stem.chars().last()?;
        if self.british {
            if last == 'ə' || last == 'ː' {
                return None;
            }
        } else if stem.chars().count() > 1 {
            let characters: Vec<char> = stem.chars().collect();
            if last == 't' && US_TAUS.contains(characters[characters.len() - 2]) {
                return Some(characters[..characters.len() - 1].iter().collect::<String>() + "ɾɪŋ");
            }
        }
        Some(stem + "ɪŋ")
    }

    pub fn get_nnp(&self, word: &str) -> (Option<String>, Option<i32>) {
        let mut pieces = Vec::new();
        for character in word.chars() {
            if character.is_alphabetic() {
                let char_upper = character.to_uppercase().to_string();
                match self.golds.get(&char_upper) {
                    Some(BinLexiconValue::Single(phonemes)) => {
                        pieces.push(phonemes.to_string());
                    }
                    Some(BinLexiconValue::Variants(variants)) => {
                        if let Some(Some(phonemes)) = variants.get("DEFAULT") {
                            pieces.push(phonemes.to_string());
                        }
                    }
                    None => {}
                }
            }
        }
        if pieces.is_empty() {
            return (None, None);
        }
        let phonemes = pieces.concat();
        if phonemes.is_empty() {
            return (None, None);
        }
        let stressed = apply_stress(Some(phonemes), Some(0.0));
        let stressed = match stressed {
            Some(s) => s,
            None => return (None, None),
        };
        let parts: Vec<&str> = stressed.splitn(2, SECONDARY_STRESS).collect();
        let joined = parts.join(&PRIMARY_STRESS.to_string());
        (Some(joined), Some(3))
    }

    fn get_special_case(&self, word: &str, tag: Option<&str>, stress: Option<f64>, ctx: TokenContext) -> (Option<String>, Option<i32>) {
        if tag == Some("ADD") {
            if let Some(symbol_word) = self.add_symbols.get(word) {
                return self.lookup(symbol_word, None, Some(-0.5), Some(ctx));
            }
        }
        if let Some(symbol_word) = self.symbols.get(word) {
            return self.lookup(symbol_word, None, None, Some(ctx));
        }
        if word.trim_matches('.').contains('.')
           && word.replace('.', "").chars().all(|c| c.is_alphabetic())
           && word.split('.').map(|s| s.chars().count()).max().unwrap_or(0) < 3 
        {
            return self.get_nnp(word);
        }
        if word == "a" || word == "A" {
            return (Some(if tag == Some("DT") { "ɐ".to_string() } else { format!("{}A", PRIMARY_STRESS) }), Some(4));
        }
        if ["am", "Am", "AM"].contains(&word) {
            if tag.unwrap_or("").starts_with("NN") {
                return self.get_nnp(word);
            }
            if ctx.future_vowel.is_none() || word != "am" || (stress.is_some() && stress.unwrap() > 0.0) {
                return (self.gold_string("am"), Some(4));
            }
            return (Some("ɐm".to_string()), Some(4));
        }
        if ["an", "An", "AN"].contains(&word) {
            if word == "AN" && tag.unwrap_or("").starts_with("NN") {
                return self.get_nnp(word);
            }
            return (Some("ɐn".to_string()), Some(4));
        }
        if word == "I" && tag == Some("PRP") {
            return (Some(format!("{}I", SECONDARY_STRESS)), Some(4));
        }
        if ["by", "By", "BY"].contains(&word) && Self::parent_tag(tag).as_deref() == Some("ADV") {
            return (Some(format!("b{}I", PRIMARY_STRESS)), Some(4));
        }
        if ["to", "To"].contains(&word) || (word == "TO" && (tag == Some("TO") || tag == Some("IN"))) {
            let phonemes = match ctx.future_vowel {
                None => self.gold_string("to").unwrap_or_else(|| "tu".to_string()),
                Some(false) => "tə".to_string(),
                Some(true) => "tʊ".to_string(),
            };
            return (Some(phonemes), Some(4));
        }
        if ["in", "In"].contains(&word) || (word == "IN" && tag != Some("NNP")) {
            let stress_marker = if ctx.future_vowel.is_none() || tag != Some("IN") { PRIMARY_STRESS.to_string() } else { String::new() };
            return (Some(format!("{}ɪn", stress_marker)), Some(4));
        }
        if ["the", "The"].contains(&word) || (word == "THE" && tag == Some("DT")) {
            return (Some(if ctx.future_vowel == Some(true) { "ði".to_string() } else { "ðə".to_string() }), Some(4));
        }
        
        static VS_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^vs\.?$").unwrap());
        if tag == Some("IN") && VS_REGEX.is_match(word) {
            return self.lookup("versus", None, None, Some(ctx));
        }
        if ["used", "Used", "USED"].contains(&word) {
            if (tag == Some("VBD") || tag == Some("JJ") || tag == Some("VB")) && ctx.future_to {
                if let Some(BinLexiconValue::Variants(variants)) = self.golds.get("used") {
                    if let Some(Some(p)) = variants.get("VBD") {
                        return (Some(p.to_string()), Some(4));
                    }
                }
            }
            if let Some(BinLexiconValue::Variants(variants)) = self.golds.get("used") {
                if let Some(Some(p)) = variants.get("DEFAULT") {
                    return (Some(p.to_string()), Some(4));
                }
            }
        }
        (None, None)
    }

    fn is_lexicon_ordinal_character(c: char) -> bool {
        c == '\'' || c.is_ascii_alphabetic()
    }

    pub fn is_known(&self, word: &str, _tag: Option<&str>) -> bool {
        if self.golds.contains_key(word) || self.symbols.contains_key(word) || self.silvers.contains_key(word) {
            return true;
        }
        if !word.chars().all(Self::is_lexicon_ordinal_character) {
            return false;
        }
        if word.chars().count() == 1 {
            return true;
        }
        if is_all_uppercase(word) && self.golds.contains_key(&word.to_lowercase()) {
            return true;
        }
        let mut chars = word.chars();
        chars.next();
        let suffix: String = chars.collect();
        suffix == suffix.to_uppercase()
    }

    pub fn lookup(&self, word: &str, tag: Option<&str>, stress: Option<f64>, ctx: Option<TokenContext>) -> (Option<String>, Option<i32>) {
        let mut lookup_word = word.to_string();
        let mut proper_name_guess = false;

        if is_all_uppercase(word) && !self.golds.contains_key(word) {
            lookup_word = word.to_lowercase();
            proper_name_guess = tag == Some("NNP");
        }

        let mut phonemes = None;
        let mut rating = 4;

        match self.golds.get(&lookup_word) {
            Some(BinLexiconValue::Single(value)) => {
                phonemes = Some(value.to_string());
            }
            Some(BinLexiconValue::Variants(variants)) => {
                let mut selected_tag = tag.map(|t| t.to_string());
                if let Some(ref c) = ctx {
                    if c.future_vowel.is_none() && variants.contains_key("None") {
                        selected_tag = Some("None".to_string());
                    } else if let Some(ref current_tag) = selected_tag {
                        if !variants.contains_key(current_tag) {
                            selected_tag = Self::parent_tag(Some(current_tag));
                        }
                    }
                }
                
                let lookup_tag = selected_tag.as_deref().unwrap_or("DEFAULT");
                phonemes = variants.get(lookup_tag)
                    .and_then(|opt| opt)
                    .or_else(|| variants.get("DEFAULT").and_then(|opt| opt))
                    .map(|s| s.to_string());
            }
            None => {}
        }

        if phonemes.is_none() && !proper_name_guess {
            phonemes = self.silvers.get(&lookup_word).map(|s| s.to_string());
            if phonemes.is_some() {
                rating = 3;
            }
        }

        if phonemes.is_none() || (proper_name_guess && !phonemes.as_ref().unwrap().contains(PRIMARY_STRESS)) {
            let (proper_name_phonemes, proper_name_rating) = self.get_nnp(&lookup_word);
            if proper_name_phonemes.is_some() {
                return (proper_name_phonemes, proper_name_rating);
            }
        }

        (apply_stress(phonemes, stress), Some(rating))
    }

    fn stem_s(&self, word: &str, tag: Option<&str>, stress: Option<f64>, ctx: Option<TokenContext>) -> (Option<String>, Option<i32>) {
        if word.chars().count() < 3 || !word.ends_with('s') {
            return (None, None);
        }

        let stem: Option<String>;
        if !word.ends_with("ss") && self.is_known(&word[..word.len() - 1], tag) {
            stem = Some(word[..word.len() - 1].to_string());
        } else if (word.ends_with("'s") || (word.chars().count() > 4 && word.ends_with("es") && !word.ends_with("ies")))
                  && self.is_known(&word[..word.len() - 2], tag) {
            stem = Some(word[..word.len() - 2].to_string());
        } else if word.chars().count() > 4 && word.ends_with("ies")
                  && self.is_known(&(word[..word.len() - 3].to_string() + "y"), tag) {
            stem = Some(word[..word.len() - 3].to_string() + "y");
        } else {
            stem = None;
        }

        let stem = match stem {
            Some(s) => s,
            None => return (None, None),
        };

        let (stem_phonemes, rating) = self.lookup(&stem, tag, stress, ctx);
        (self.plural_suffix(stem_phonemes), rating)
    }

    fn stem_ed(&self, word: &str, tag: Option<&str>, stress: Option<f64>, ctx: Option<TokenContext>) -> (Option<String>, Option<i32>) {
        if word.chars().count() < 4 || !word.ends_with('d') {
            return (None, None);
        }

        let stem: Option<String>;
        if !word.ends_with("dd") && self.is_known(&word[..word.len() - 1], tag) {
            stem = Some(word[..word.len() - 1].to_string());
        } else if word.chars().count() > 4 && word.ends_with("ed") && !word.ends_with("eed")
                  && self.is_known(&word[..word.len() - 2], tag) {
            stem = Some(word[..word.len() - 2].to_string());
        } else {
            stem = None;
        }

        let stem = match stem {
            Some(s) => s,
            None => return (None, None),
        };

        let (stem_phonemes, rating) = self.lookup(&stem, tag, stress, ctx);
        (self.ed_suffix(stem_phonemes), rating)
    }

    fn stem_ing(&self, word: &str, tag: Option<&str>, stress: Option<f64>, ctx: Option<TokenContext>) -> (Option<String>, Option<i32>) {
        if word.chars().count() < 5 || !word.ends_with("ing") {
            return (None, None);
        }

        let base = word[..word.len() - 3].to_string();
        let stem: Option<String>;

        static ING_REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?i)([bcdgklmnprstvxz])\1ing$|cking$").unwrap()
        });

        if word.chars().count() > 5 && self.is_known(&base, tag) {
            stem = Some(base);
        } else if self.is_known(&(base.clone() + "e"), tag) {
            stem = Some(base + "e");
        } else if word.chars().count() > 5 && ING_REGEX.is_match(word)
                  && self.is_known(&word[..word.len() - 4], tag) {
            stem = Some(word[..word.len() - 4].to_string());
        } else {
            stem = None;
        }

        let stem = match stem {
            Some(s) => s,
            None => return (None, None),
        };

        let (stem_phonemes, rating) = self.lookup(&stem, tag, stress, ctx);
        (self.ing_suffix(stem_phonemes), rating)
    }

    pub fn get_word(&self, word: &str, tag: Option<&str>, stress: Option<f64>, ctx: TokenContext) -> (Option<String>, Option<i32>) {
        let (special_case_phonemes, special_case_rating) = self.get_special_case(word, tag, stress, ctx.clone());
        if special_case_phonemes.is_some() {
            return (special_case_phonemes, special_case_rating);
        }

        let mut lookup_word = word.to_string();
        let lowercased = word.to_lowercase();
        let deapostrophized = word.replace('\'', "");

        if word.chars().count() > 1
           && !deapostrophized.is_empty()
           && deapostrophized.chars().all(|c| c.is_alphabetic())
           && word != lowercased
           && (tag != Some("NNP") || word.chars().count() > 7)
           && !self.golds.contains_key(word)
           && !self.silvers.contains_key(word)
           && (is_all_uppercase(word) || {
               let mut chars = word.chars();
               chars.next();
               let rest: String = chars.collect();
               rest == rest.to_lowercase()
           })
           && (self.golds.contains_key(&lowercased)
               || self.silvers.contains_key(&lowercased)
               || self.stem_s(&lowercased, tag, stress, Some(ctx.clone())).0.is_some()
               || self.stem_ed(&lowercased, tag, stress, Some(ctx.clone())).0.is_some()
               || self.stem_ing(&lowercased, tag, stress.or(Some(0.5)), Some(ctx.clone())).0.is_some())
        {
            lookup_word = lowercased;
        }

        if self.is_known(&lookup_word, tag) {
            return self.lookup(&lookup_word, tag, stress, Some(ctx));
        }

        if lookup_word.ends_with("s'") && self.is_known(&(lookup_word[..lookup_word.len() - 2].to_string() + "'s"), tag) {
            return self.lookup(&(lookup_word[..lookup_word.len() - 2].to_string() + "'s"), tag, stress, Some(ctx));
        }

        if lookup_word.ends_with('\'') && self.is_known(&lookup_word[..lookup_word.len() - 1], tag) {
            return self.lookup(&lookup_word[..lookup_word.len() - 1], tag, stress, Some(ctx));
        }

        let s_result = self.stem_s(&lookup_word, tag, stress, Some(ctx.clone()));
        if s_result.0.is_some() {
            return s_result;
        }

        let ed_result = self.stem_ed(&lookup_word, tag, stress, Some(ctx.clone()));
        if ed_result.0.is_some() {
            return ed_result;
        }

        let ing_stress = stress.unwrap_or(0.5);
        let ing_result = self.stem_ing(&lookup_word, tag, Some(ing_stress), Some(ctx));
        if ing_result.0.is_some() {
            return ing_result;
        }

        (None, None)
    }

    pub fn is_currency(word: &str) -> bool {
        if !word.contains('.') {
            return true;
        }
        if word.chars().filter(|&c| c == '.').count() > 1 {
            return false;
        }
        let parts: Vec<&str> = word.split('.').collect();
        let cents = parts.get(1).unwrap_or(&"");
        cents.chars().count() < 3 || cents.chars().all(|c| c == '0')
    }

    fn append_word_to_result(&self, result: &mut Vec<(String, i32)>, w: &str, first: bool, escape: bool, num_flags: &str) {
        let source = if escape {
            w.to_string()
        } else {
            crate::normalization::NumberToWords::cardinal_str(w).unwrap_or_else(|| w.to_string())
        };
        let splits = split_lowercase_words(&source);
        for (index, split) in splits.iter().enumerate() {
            if split != "and" || num_flags.contains('&') {
                if first && index == 0 && splits.len() > 1 && split == "one" && num_flags.contains('a') {
                    result.push(("ə".to_string(), 4));
                } else {
                    let stress = if split == "point" { Some(-2.0) } else { None };
                    if let (Some(ph), Some(r)) = self.lookup(split, None, stress, None) {
                        result.push((ph, r));
                    }
                }
            } else if split == "and" && num_flags.contains('n') && !result.is_empty() {
                let last = result.pop().unwrap();
                result.push((last.0 + "ən", last.1));
            }
        }
    }

    pub fn get_number(&self, word: &str, currency: Option<&str>, is_head: bool, num_flags: &str) -> (Option<String>, Option<i32>) {
        static SUFFIX_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"[a-zA-Z']+$").unwrap());
        let suffix = SUFFIX_REGEX.find(word).map(|m| m.as_str());
        let suffix_lower = suffix.map(|s| s.to_lowercase());
        let numeric_word = if let Some(s) = suffix {
            &word[..word.len() - s.len()]
        } else {
            word
        };

        let mut working_word = numeric_word.to_string();
        let mut result: Vec<(String, i32)> = Vec::new();

        if working_word.starts_with('-') {
            if let (Some(minus_ph), Some(minus_r)) = self.lookup("minus", None, None, None) {
                result.push((minus_ph, minus_r));
            }
            working_word.remove(0);
        }

        let is_digit_string = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_numeric());

        if is_digit_string(&working_word) && suffix_lower.is_some() && self.ordinals.contains(suffix_lower.as_ref().unwrap().as_str()) {
            self.append_word_to_result(&mut result, &crate::normalization::NumberToWords::ordinal_str(&working_word).unwrap_or(working_word.clone()), true, true, num_flags);
        } else if result.is_empty() && working_word.chars().count() == 4 && currency.is_none() && is_digit_string(&working_word) {
            self.append_word_to_result(&mut result, &crate::normalization::NumberToWords::year_str(&working_word).unwrap_or(working_word.clone()), true, true, num_flags);
        } else if !is_head && !working_word.contains('.') {
            let compact = working_word.replace(',', "");
            if compact.starts_with('0') || compact.chars().count() > 3 {
                for c in compact.chars() {
                    self.append_word_to_result(&mut result, &c.to_string(), false, false, num_flags);
                }
            } else if compact.chars().count() == 3 && !compact.ends_with("00") {
                self.append_word_to_result(&mut result, &compact[..1], true, false, num_flags);
                let tens_and_ones = &compact[1..];
                if tens_and_ones.starts_with('0') {
                    if let (Some(o_ph), Some(o_r)) = self.lookup("O", None, Some(-2.0), None) {
                        result.push((o_ph, o_r));
                    }
                    self.append_word_to_result(&mut result, &tens_and_ones[1..], false, false, num_flags);
                } else {
                    self.append_word_to_result(&mut result, tens_and_ones, false, false, num_flags);
                }
            } else {
                self.append_word_to_result(&mut result, &compact, true, false, num_flags);
            }
        } else if working_word.chars().filter(|&c| c == '.').count() > 1 || !is_head {
            let mut first = true;
            for chunk in working_word.replace(',', "").split('.').collect::<Vec<&str>>() {
                if chunk.is_empty() {
                    continue;
                }
                if chunk.starts_with('0') || (chunk.chars().count() != 2 && chunk[1..].chars().any(|c| c != '0')) {
                    for c in chunk.chars() {
                        self.append_word_to_result(&mut result, &c.to_string(), false, false, num_flags);
                    }
                } else {
                    self.append_word_to_result(&mut result, chunk, first, false, num_flags);
                }
                first = false;
            }
        } else if let Some(curr) = currency {
            if let Some(&(unit_singular, unit_cents)) = self.currencies.get(curr) {
                if Self::is_currency(&working_word) {
                    let mut pairs: Vec<String> = working_word.replace(',', "").split('.').map(|s| s.to_string()).collect();
                    while pairs.len() < 2 {
                        pairs.push(String::new());
                    }
                    let mut quantities = Vec::new();
                    let val0 = pairs[0].parse::<i64>().unwrap_or(0);
                    let val1 = pairs[1].parse::<i64>().unwrap_or(0);
                    quantities.push((val0, unit_singular.to_string()));
                    if val1 != 0 || val0 == 0 {
                        quantities.push((val1, unit_cents.to_string()));
                    }
                    if val1 == 0 && val0 != 0 {
                        quantities.truncate(1);
                    }
                    if val0 == 0 && val1 != 0 {
                        quantities.remove(0);
                    }

                    for (index, pair) in quantities.iter().enumerate() {
                        if index > 0 {
                            if let (Some(and_ph), Some(and_r)) = self.lookup("and", None, None, None) {
                                result.push((and_ph, and_r));
                            }
                        }
                        self.append_word_to_result(&mut result, &pair.0.to_string(), index == 0, false, num_flags);
                        if pair.0.abs() != 1 && pair.1 != "pence" {
                            let pluralized = self.stem_s(&(pair.1.clone() + "s"), None, None, None);
                            if let (Some(ph), Some(r)) = pluralized {
                                result.push((ph, r));
                            }
                        } else {
                            if let (Some(ph), Some(r)) = self.lookup(&pair.1, None, None, None) {
                                result.push((ph, r));
                            }
                        }
                    }
                }
            }
        } else {
            let spoken: String;
            if is_digit_string(&working_word) {
                spoken = crate::normalization::NumberToWords::cardinal_str(&working_word).unwrap_or(working_word.clone());
            } else if !working_word.contains('.') {
                let compact = working_word.replace(',', "");
                spoken = suffix_lower.as_ref()
                    .filter(|s| self.ordinals.contains(s.as_str()))
                    .and_then(|_| crate::normalization::NumberToWords::ordinal_str(&compact))
                    .or_else(|| crate::normalization::NumberToWords::cardinal_str(&compact))
                    .unwrap_or(compact);
            } else {
                let compact = working_word.replace(',', "");
                if compact.starts_with('.') {
                    let decimal_digits: String = compact[1..].chars()
                        .map(|c| crate::normalization::NumberToWords::cardinal_str(&c.to_string()).unwrap_or_else(|| c.to_string()))
                        .collect::<Vec<String>>()
                        .join(" ");
                    spoken = format!("point {}", decimal_digits);
                } else {
                    let parts: Vec<&str> = compact.split('.').collect();
                    let left = crate::normalization::NumberToWords::cardinal_str(parts[0]).unwrap_or_else(|| parts[0].to_string());
                    let right = if parts.len() > 1 {
                        parts[1].chars()
                            .map(|c| crate::normalization::NumberToWords::cardinal_str(&c.to_string()).unwrap_or_else(|| c.to_string()))
                            .collect::<Vec<String>>()
                            .join(" ")
                    } else {
                        String::new()
                    };
                    spoken = if right.is_empty() { left } else { format!("{} point {}", left, right) };
                }
            }
            self.append_word_to_result(&mut result, &spoken, true, true, num_flags);
        }

        if result.is_empty() {
            return (None, None);
        }
        let phonemes = result.iter().map(|x| x.0.clone()).collect::<Vec<String>>().join(" ");
        let rating = result.iter().map(|x| x.1).min();

        if let Some(ref s_low) = suffix_lower {
            if s_low == "s" || s_low == "'s" {
                return (self.plural_suffix(Some(phonemes)), rating);
            }
            if s_low == "ed" || s_low == "'d" {
                return (self.ed_suffix(Some(phonemes)), rating);
            }
            if s_low == "ing" {
                return (self.ing_suffix(Some(phonemes)), rating);
            }
        }

        (Some(phonemes), rating)
    }

    pub fn append_currency(&self, phonemes: &str, currency: Option<&str>) -> String {
        let currency = match currency {
            Some(c) => c,
            None => return phonemes.to_string(),
        };
        let unit = match self.currencies.get(currency) {
            Some(&(u, _)) => u,
            None => return phonemes.to_string(),
        };
        match self.stem_s(&(unit.to_string() + "s"), None, None, None).0 {
            Some(pluralized) => format!("{} {}", phonemes, pluralized),
            None => phonemes.to_string(),
        }
    }

    fn numeric_if_needed(c: char) -> String {
        if c.is_numeric() {
            if let Some(val) = c.to_digit(10) {
                return val.to_string();
            }
        }
        c.to_string()
    }

    pub fn is_number(word: &str, is_head: bool) -> bool {
        if !word.chars().any(|c| c.is_numeric()) {
            return false;
        }
        let suffixes = ["ing", "'d", "ed", "'s", "st", "nd", "rd", "th", "s"];
        let word_lower = word.to_lowercase();
        let mut candidate = word.to_string();
        for suffix in suffixes {
            if word_lower.ends_with(suffix) {
                candidate.truncate(candidate.chars().count() - suffix.chars().count());
                break;
            }
        }
        candidate.chars().enumerate().all(|(index, character)| {
            character.is_numeric() || character == ',' || character == '.' || (is_head && index == 0 && character == '-')
        })
    }

    pub fn lookup_token(&self, token: &InternalMToken, ctx: TokenContext) -> (Option<String>, Option<i32>) {
        let raw_alias = token.underscore.alias.as_deref().unwrap_or(&token.text);
        let replaced = raw_alias.replace('‘', "'").replace('’', "'");
        let word: String = replaced.chars().map(Self::numeric_if_needed).collect();

        let stress = if word == word.to_lowercase() {
            None
        } else {
            Some(if is_all_uppercase(&word) { self.cap_stresses.1 } else { self.cap_stresses.0 })
        };

        let (word_phonemes, word_rating) = self.get_word(&word, Some(&token.tag), stress, ctx);
        if let Some(wp) = word_phonemes {
            let appended = self.append_currency(&wp, token.underscore.currency.as_deref());
            let stressed = apply_stress(Some(appended), token.underscore.stress);
            return (stressed, word_rating);
        }

        if Self::is_number(&word, token.underscore.is_head) {
            let (number_phonemes, number_rating) = self.get_number(
                &word,
                token.underscore.currency.as_deref(),
                token.underscore.is_head,
                &token.underscore.num_flags
            );
            return (apply_stress(number_phonemes, token.underscore.stress), number_rating);
        }

        (None, None)
    }
}
