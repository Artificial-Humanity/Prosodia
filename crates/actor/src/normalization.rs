use regex::Regex;
use once_cell::sync::Lazy;

#[derive(Clone, Debug, PartialEq)]
pub enum FeatureSpanKind {
    Stress(f64),
    PhonemeOverride(String),
    NumFlags(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeatureSpan {
    pub range: std::ops::Range<usize>,
    pub kind: FeatureSpanKind,
}

#[derive(Clone, Debug)]
pub struct PreprocessResult {
    pub text: String,
    pub feature_spans: Vec<FeatureSpan>,
}

static LINK_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[([^\]]+)\]\(([^)]*)\)").unwrap()
});

pub fn parse_feature(feature: &str) -> Option<FeatureSpanKind> {
    if let Ok(value) = feature.parse::<f64>() {
        return Some(FeatureSpanKind::Stress(value));
    }
    if feature.len() > 1 && feature.starts_with('/') && feature.ends_with('/') {
        let val = if feature == "/" { "" } else { &feature[1..feature.len() - 1] };
        return Some(FeatureSpanKind::PhonemeOverride(val.to_string()));
    } else if feature.len() > 1 && feature.starts_with('#') && feature.ends_with('#') {
        let val = &feature[1..feature.len() - 1];
        return Some(FeatureSpanKind::NumFlags(val.to_string()));
    }
    None
}

pub fn preprocess(text: &str) -> PreprocessResult {
    let trimmed = text.trim_start();
    let mut result = String::new();
    let mut feature_spans = Vec::new();
    let mut current_idx = 0;

    for cap in LINK_REGEX.captures_iter(trimmed) {
        let whole_match = cap.get(0).unwrap();
        let visible_match = cap.get(1).unwrap();
        let feature_match = cap.get(2).unwrap();

        let start_bytes = whole_match.start();
        if current_idx < start_bytes {
            result.push_str(&trimmed[current_idx..start_bytes]);
        }

        let start_char_idx = result.chars().count();
        result.push_str(visible_match.as_str());
        let end_char_idx = result.chars().count();

        if let Some(feature) = parse_feature(feature_match.as_str()) {
            feature_spans.push(FeatureSpan {
                range: start_char_idx..end_char_idx,
                kind: feature,
            });
        }

        current_idx = whole_match.end();
    }

    if current_idx < trimmed.len() {
        result.push_str(&trimmed[current_idx..]);
    }

    PreprocessResult {
        text: result,
        feature_spans,
    }
}

pub struct NumberToWords;

impl NumberToWords {
    const UNITS: &'static [&'static str] = &[
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"
    ];

    const TENS: &'static [(&'static str, usize)] = &[
        ("twenty", 20),
        ("thirty", 30),
        ("forty", 40),
        ("fifty", 50),
        ("sixty", 60),
        ("seventy", 70),
        ("eighty", 80),
        ("ninety", 90),
    ];

    const ORDINAL_UNITS: &'static [&'static str] = &[
        "zeroth", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteen", "seventeenth", "eighteenth", "nineteenth"
    ];

    const ORDINAL_TENS: &'static [(&'static str, usize)] = &[
        ("twentieth", 20),
        ("thirtieth", 30),
        ("fortieth", 40),
        ("fiftieth", 50),
        ("sixtieth", 60),
        ("seventieth", 70),
        ("eightieth", 80),
        ("ninetieth", 90),
    ];

    const SCALES: &'static [(u64, &'static str)] = &[
        (1_000_000_000_000_000_000, "quintillion"),
        (1_000_000_000_000_000, "quadrillion"),
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];

    fn get_unit(val: usize) -> Option<&'static str> {
        if val < Self::UNITS.len() {
            Some(Self::UNITS[val])
        } else {
            None
        }
    }

    fn get_ten(val: usize) -> Option<&'static str> {
        for &(word, num) in Self::TENS {
            if num == val {
                return Some(word);
            }
        }
        None
    }

    fn get_ordinal_unit(val: usize) -> Option<&'static str> {
        if val < Self::ORDINAL_UNITS.len() {
            Some(Self::ORDINAL_UNITS[val])
        } else {
            None
        }
    }

    fn get_ordinal_ten(val: usize) -> Option<&'static str> {
        for &(word, num) in Self::ORDINAL_TENS {
            if num == val {
                return Some(word);
            }
        }
        None
    }

    pub fn cardinal_magnitude(value: u64) -> String {
        if let Some(unit) = Self::get_unit(value as usize) {
            return unit.to_string();
        }
        if value < 100 {
            let tens_value = (value / 10) * 10;
            let remainder = value % 10;
            let tens_word = Self::get_ten(tens_value as usize).unwrap_or("");
            return if remainder == 0 {
                tens_word.to_string()
            } else {
                format!("{}-{}", tens_word, Self::get_unit(remainder as usize).unwrap_or(""))
            };
        }
        if value < 1_000 {
            let hundreds = value / 100;
            let remainder = value % 100;
            let base = format!("{} hundred", Self::get_unit(hundreds as usize).unwrap_or(""));
            return if remainder == 0 {
                base
            } else {
                format!("{} {}", base, Self::cardinal_magnitude(remainder))
            };
        }
        for &(scale_value, scale_name) in Self::SCALES {
            if value >= scale_value {
                let major = value / scale_value;
                let remainder = value % scale_value;
                let base = format!("{} {}", Self::cardinal_magnitude(major), scale_name);
                return if remainder == 0 {
                    base
                } else {
                    format!("{} {}", base, Self::cardinal_magnitude(remainder))
                };
            }
        }
        value.to_string()
    }

    pub fn ordinal_magnitude(value: u64) -> String {
        if let Some(direct) = Self::get_ordinal_unit(value as usize) {
            return direct.to_string();
        }
        if value < 100 {
            let tens_value = (value / 10) * 10;
            let remainder = value % 10;
            let tens_word = Self::get_ten(tens_value as usize).unwrap_or("");
            return if remainder == 0 {
                Self::get_ordinal_ten(tens_value as usize)
                    .map(|w| w.to_string())
                    .unwrap_or_else(|| format!("{}th", tens_word))
            } else {
                format!("{}-{}", tens_word, Self::get_ordinal_unit(remainder as usize).unwrap_or(""))
            };
        }
        if value < 1_000 {
            let hundreds = value / 100;
            let remainder = value % 100;
            let base = format!("{} hundred", Self::get_unit(hundreds as usize).unwrap_or(""));
            return if remainder == 0 {
                format!("{}th", base)
            } else {
                format!("{} {}", base, Self::ordinal_magnitude(remainder))
            };
        }
        for &(scale_value, scale_name) in Self::SCALES {
            if value >= scale_value {
                let major = value / scale_value;
                let remainder = value % scale_value;
                let base = format!("{} {}", Self::cardinal_magnitude(major), scale_name);
                return if remainder == 0 {
                    format!("{}th", base)
                } else {
                    format!("{} {}", base, Self::ordinal_magnitude(remainder))
                };
            }
        }
        format!("{}th", value)
    }

    pub fn year_magnitude(value: u64) -> String {
        if !(1000..=9999).contains(&value) {
            return Self::cardinal_magnitude(value);
        }
        let high = value / 100;
        let low = value % 100;

        if (2000..=2009).contains(&value) {
            return if low == 0 {
                "two thousand".to_string()
            } else {
                format!("two thousand {}", Self::cardinal_magnitude(low))
            };
        }

        if low == 0 {
            return format!("{} hundred", Self::cardinal_magnitude(high));
        }

        if low < 10 {
            return format!("{} oh-{}", Self::cardinal_magnitude(high), Self::cardinal_magnitude(low));
        }

        format!("{} {}", Self::cardinal_magnitude(high), Self::cardinal_magnitude(low))
    }

    pub fn cardinal(value: i64) -> String {
        if value < 0 {
            return format!("minus {}", Self::cardinal_magnitude(value.unsigned_abs()));
        }
        Self::cardinal_magnitude(value as u64)
    }

    pub fn ordinal(value: i64) -> String {
        if value < 0 {
            return format!("minus {}", Self::ordinal_magnitude(value.unsigned_abs()));
        }
        Self::ordinal_magnitude(value as u64)
    }

    pub fn year(value: i64) -> String {
        if value < 0 {
            return format!("minus {}", Self::year_magnitude(value.unsigned_abs()));
        }
        Self::year_magnitude(value as u64)
    }

    pub fn cardinal_str(text: &str) -> Option<String> {
        text.parse::<i64>().ok().map(Self::cardinal)
    }

    pub fn ordinal_str(text: &str) -> Option<String> {
        text.parse::<i64>().ok().map(Self::ordinal)
    }

    pub fn year_str(text: &str) -> Option<String> {
        text.parse::<i64>().ok().map(Self::year)
    }
}
