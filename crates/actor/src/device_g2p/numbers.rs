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
// Case-sensitive: book text capitalizes the abbreviation ("No. 7"), and №
// folds to "No. " (see fold.rs's OVERRIDES). Case-insensitive used to also
// catch a bare lowercase "no." used as a word, not as the Numero sign —
// "said no. 7 cats" has no number being named, just a digit to speak.
static NUMERO: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bNo\.\s?(\d)").unwrap());
// Ordinals: a digit run immediately followed by st/nd/rd/th must consume
// the suffix with it, or NUMBER below spells only the digits and leaves
// the suffix stranded ("2nd" -> "two nd"). The leading `(\d\.)?` guards
// against matching the fractional half of a decimal ("3.5s", "0.2th"):
// the `regex` crate has no lookbehind, so the only way to see "preceded
// by <digit>." is to let the match start there and capture it. When that
// group is present the closure hands the whole match back unchanged, so
// the plain NUMBER pass reads the decimal whole instead ("3.5" -> "three
// point five"), leaving the stray suffix letter(s) as their own token.
static ORDINAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(\d\.)?\b(\d[\d,]*)(?:st|nd|rd|th)\b").unwrap());
// Decade/plural digits: a digit run immediately followed by "s" or "'s"
// must also consume the suffix with it, for the same reason ("1990s" ->
// "nineteen ninety s", "1920's" -> "nineteen twenty 's" otherwise). Runs
// after the fold, so a curly apostrophe is already ASCII. Same decimal
// guard as ORDINAL.
static DECADE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d\.)?\b(\d[\d,]*)'?s\b").unwrap());
static NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(AMOUNT).unwrap());

/// A bare four-digit 1100–2099 reads as a year. A thousands comma means the
/// writer meant a quantity ("1,999 men"), so a comma'd run never does.
fn is_year(whole: &str) -> bool {
    whole.len() == 4 && whole.bytes().all(|b| b.is_ascii_digit())
        && whole.parse::<i64>().is_ok_and(|v| (1100..=2099).contains(&v))
}

/// `whole[.frac]` in words; years per `is_year` when `as_year`.
fn amount(whole: &str, frac: Option<&str>, as_year: bool) -> String {
    let digits = whole.replace(',', "");
    let year = as_year && frac.is_none() && is_year(whole);
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

/// `whole` in ordinal words (`21` -> `twenty-first`).
fn ordinal_words(whole: &str) -> String {
    let digits = whole.replace(',', "");
    NumberToWords::ordinal_str(&digits).unwrap_or_else(|| digits.clone())
}

/// Pluralizes the last whitespace-delimited word only, so a hyphenated
/// head (`twenty-one`) stays intact and only its own tail is touched: a
/// final `y` becomes `ies` (`ninety` -> `nineties`), a final sibilant
/// (`x`, `s`, `sh`, `ch`) takes `es` (`six` -> `sixes`), and everything
/// else takes `s` (`hundred` -> `hundreds`).
fn pluralize_last(words: &str) -> String {
    match words.rsplit_once(' ') {
        Some((head, last)) => format!("{head} {}", pluralize_word(last)),
        None => pluralize_word(words),
    }
}

fn pluralize_word(word: &str) -> String {
    if let Some(stem) = word.strip_suffix('y') {
        format!("{stem}ies")
    } else if word.ends_with(['x', 's']) || word.ends_with("sh") || word.ends_with("ch") {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

/// `whole` as a decade/plural: year words per `is_year` ("1990s"),
/// cardinal otherwise ("1,900s"), with the last word pluralized.
fn decade_words(whole: &str) -> String {
    let digits = whole.replace(',', "");
    let year = is_year(whole);
    let words = if year { NumberToWords::year_str(&digits) } else { NumberToWords::cardinal_str(&digits) }
        .unwrap_or_else(|| digits.clone());
    pluralize_last(&words)
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

/// After the fold: fractions, `No. N`, ordinals and decade plurals (both
/// must consume their letter suffix before the plain number pass would
/// otherwise spell just the digits and strand the suffix), then every
/// remaining number.
pub fn expand_digits(folded: &str) -> String {
    let t = FRACTION.replace_all(folded, |c: &Captures| match (&c[1], &c[2]) {
        ("1", "2") => " one half ".to_string(),
        ("1", "4") => " one quarter ".to_string(),
        ("3", "4") => " three quarters ".to_string(),
        (a, b) => format!(" {} over {} ", amount(a, None, false), amount(b, None, false)),
    });
    let t = NUMERO.replace_all(&t, " number $1");
    let t = ORDINAL.replace_all(&t, |c: &Captures| match c.get(1) {
        Some(_) => c[0].to_string(), // the fractional half of a decimal: leave it for NUMBER
        None => format!(" {} ", ordinal_words(&c[2])),
    });
    let t = DECADE.replace_all(&t, |c: &Captures| match c.get(1) {
        Some(_) => c[0].to_string(), // the fractional half of a decimal: leave it for NUMBER
        None => format!(" {} ", decade_words(&c[2])),
    });
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

    #[test]
    fn ordinals_are_spelled_as_ordinals_not_a_floating_suffix() {
        assert_eq!(spoken("the 2nd edition"), "the second edition");
        assert_eq!(spoken("October 21st,"), "October twenty-first ,");
        assert_eq!(spoken("3rd and 4th"), "third and fourth");
    }

    #[test]
    fn decade_plurals_keep_the_final_word_pluralized() {
        assert_eq!(spoken("the 1990s were"), "the nineteen nineties were");
        assert_eq!(spoken("the 20s"), "the twenties");
        assert_eq!(spoken("1800s"), "eighteen hundreds");
    }

    #[test]
    fn numero_only_fires_on_the_capitalized_abbreviation() {
        assert_eq!(spoken("No. 7"), "number seven");
        assert_eq!(spoken("№3"), "number three");
        assert_eq!(spoken("said no. 7 cats"), "said no. seven cats");
    }

    #[test]
    fn decade_and_ordinal_never_eat_a_decimal_fraction() {
        assert_eq!(spoken("it took 3.5s"), "it took three point five s");
        assert!(spoken("0.2th").contains("zero point two"), "{}", spoken("0.2th"));
        // Unaffected by the decimal guard:
        assert_eq!(spoken("the 1990s were"), "the nineteen nineties were");
        assert_eq!(spoken("the 2nd edition"), "the second edition");
    }

    #[test]
    fn a_thousands_comma_never_reads_as_a_year() {
        assert_eq!(spoken("1,250 pages"), "one thousand two hundred fifty pages");
        assert_eq!(spoken("1,999 men"), "one thousand nine hundred ninety-nine men");
        assert_eq!(spoken("the 1,900s"), "the one thousand nine hundreds");
        // Without the comma the year rule still applies:
        assert_eq!(spoken("In 1999 men"), "In nineteen ninety-nine men");
        assert_eq!(spoken("the 1990s"), "the nineteen nineties");
    }

    #[test]
    fn decade_plurals_with_an_apostrophe_are_plurals_too() {
        assert_eq!(spoken("the 1920's"), "the nineteen twenties");
        assert_eq!(spoken("6's and 7's"), "sixes and sevens");
        // The curly apostrophe folds to ASCII first.
        assert_eq!(spoken("the 1920\u{2019}s"), "the nineteen twenties");
        // The decimal guard still holds with the apostrophe.
        assert!(spoken("it took 3.5's").starts_with("it took three point five"), "{}", spoken("it took 3.5's"));
    }

    #[test]
    fn decade_plural_sibilants_take_es_not_a_bare_s() {
        assert_eq!(spoken("4s and 6s"), "fours and sixes");
    }
}
