//! Numbers, money and percentages spelled out for the device G2P. Sonora's
//! tokenizer deletes digits (its corpus dropped digit-bearing clips); a book
//! reader must speak them. Two passes, because the ASCII fold destroys
//! currency symbols (`£5` -> `PS5`) and manufactures digits (`²` -> `2`).

use crate::normalization::NumberToWords;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};

const AMOUNT: &str = r"(\d[\d,]*)(\.\d+)?";
static CURRENCY: Lazy<Regex> = Lazy::new(|| Regex::new(&format!(r"([$£€¥])\s?{AMOUNT}")).unwrap());
static PERCENT: Lazy<Regex> = Lazy::new(|| Regex::new(&format!(r"{AMOUNT}\s?%")).unwrap());
static FRACTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)\s*/\s*(\d+)").unwrap());
static NUMERO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\bno\.\s?(\d)").unwrap());
static NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(AMOUNT).unwrap());

/// `whole[.frac]` in words; years for bare four-digit 1100–2099.
fn amount(whole: &str, frac: Option<&str>, as_year: bool) -> String {
    let digits = whole.replace(',', "");
    let year = as_year && frac.is_none() && digits.len() == 4
        && digits.parse::<i64>().is_ok_and(|v| (1100..=2099).contains(&v));
    let mut words = if year { NumberToWords::year_str(&digits) } else { NumberToWords::cardinal_str(&digits) }
        .unwrap_or_else(|| digits.clone());
    if let Some(frac) = frac {
        words.push_str(" point");
        for d in frac.trim_start_matches('.').chars().filter_map(|c| c.to_digit(10)) {
            words.push(' ');
            words.push_str(&NumberToWords::cardinal(d as i64));
        }
    }
    words
}

/// `AMOUNT`'s `[\d,]*` is greedy: when a thousands comma is the last thing
/// in the match, nothing stopped it from also eating a sentence/list comma
/// that happened to follow the digits directly (`"€20, ¥3"` -> whole
/// capture `"20,"`). A genuine thousands separator is always followed by
/// more digits *inside* the match, so it can never be the trailing byte —
/// a trailing comma is therefore always punctuation that strayed into the
/// capture, never a grouping comma. Split it back out so it lands after the
/// spelled-out words instead of vanishing with them.
fn split_trailing_comma(whole: &str) -> (&str, &str) {
    match whole.strip_suffix(',') {
        Some(w) => (w, ","),
        None => (whole, ""),
    }
}

/// Before the fold: currency amounts and percentages.
pub fn expand_symbols(text: &str) -> String {
    let t = CURRENCY.replace_all(text, |c: &Captures| {
        let (whole, trailing) = split_trailing_comma(&c[2]);
        let n = amount(whole, c.get(3).map(|m| m.as_str()), false);
        let one = whole == "1" && c.get(3).is_none();
        let unit = match &c[1] {
            "$" => if one { "dollar" } else { "dollars" },
            "£" => if one { "pound" } else { "pounds" },
            "€" => if one { "euro" } else { "euros" },
            _ => "yen",
        };
        format!(" {n} {unit} {trailing}")
    });
    PERCENT
        .replace_all(&t, |c: &Captures| {
            let (whole, trailing) = split_trailing_comma(&c[1]);
            format!(" {} percent {trailing}", amount(whole, c.get(2).map(|m| m.as_str()), false))
        })
        .into_owned()
}

/// After the fold: fractions, `No. N`, then every remaining number.
pub fn expand_digits(folded: &str) -> String {
    let t = FRACTION.replace_all(folded, |c: &Captures| match (&c[1], &c[2]) {
        ("1", "2") => " one half ".to_string(),
        ("1", "4") => " one quarter ".to_string(),
        ("3", "4") => " three quarters ".to_string(),
        (a, b) => format!(" {} over {} ", amount(a, None, false), amount(b, None, false)),
    });
    let t = NUMERO.replace_all(&t, " number $1");
    NUMBER
        .replace_all(&t, |c: &Captures| {
            let (whole, trailing) = split_trailing_comma(&c[1]);
            format!(" {} {trailing}", amount(whole, c.get(2).map(|m| m.as_str()), true))
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_g2p::fold::ascii_fold;

    fn spoken(text: &str) -> String {
        let s = expand_digits(&ascii_fold(&expand_symbols(text)));
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn currency_and_percent_are_spelled_before_the_fold_destroys_them() {
        assert_eq!(spoken("Chapter 12 cost $5."), "Chapter twelve cost five dollars .");
        assert_eq!(spoken("£5 and €20, ¥3"), "five pounds and twenty euros , three yen");
        assert_eq!(spoken("3% of 40"), "three percent of forty");
        assert_eq!(spoken("$1"), "one dollar");
    }

    #[test]
    fn digits_the_fold_makes_are_spelled_after_it() {
        assert_eq!(spoken("x²"), "x two");
        assert_eq!(spoken("½ cup"), "one half cup");
        assert_eq!(spoken("１００ pages"), "one hundred pages");
        assert_eq!(spoken("№3"), "number three");
        assert_eq!(spoken("In 1984, 2.5 kg"), "In nineteen eighty-four , two point five kg");
    }

    #[test]
    fn text_without_numbers_is_unchanged() {
        assert_eq!(spoken("No numbers here."), "No numbers here.");
    }
}
