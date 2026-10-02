//! The ASCII fold Sonora's front end applies first (`unidecode`), reproduced
//! with `deunicode` plus the code points where the two disagree.

/// Code points where `deunicode` and `unidecode` 1.4.0 disagree, with
/// unidecode's answer. Populated from the fixture test's output; every entry
/// is a disagreement that test found.
const OVERRIDES: &[(char, &str)] = &[
    ('\u{00BC}', " 1/4"), // deunicode "1/4"
    ('\u{00BD}', " 1/2"), // deunicode "1/2"
    ('\u{00BE}', " 3/4"), // deunicode "3/4"
    ('\u{014A}', "NG"), // deunicode "ng"
    ('\u{014B}', "ng"), // deunicode "NG"
    ('\u{2116}', "No. "), // deunicode "No" — verified against unidecode 1.4.0 locally (Task 8, № not in the fixture: no G7 probe carries it)
    ('\u{0241}', ""), // deunicode "'"
    ('\u{0242}', ""), // deunicode "'"
    ('\u{204A}', "&"), // deunicode "7"
    ('\u{204F}', ""), // deunicode ";"
    ('\u{2050}', ""), // deunicode "_"
    ('\u{2051}', ""), // deunicode "**"
    ('\u{2052}', "%"), // deunicode "./."
    ('\u{2054}', ""), // deunicode "_"
    ('\u{2055}', ""), // deunicode "*"
    ('\u{2056}', ""), // deunicode "_"
    ('\u{2058}', ""), // deunicode "_"
    ('\u{2059}', ""), // deunicode "*"
    ('\u{205A}', ""), // deunicode ":"
    ('\u{205B}', ""), // deunicode "_"
    ('\u{205C}', ""), // deunicode "+"
    ('\u{205D}', ""), // deunicode "_"
    ('\u{205E}', ""), // deunicode "_"
];

/// Folds `text` to ASCII as `unidecode` does: ASCII passes through, other
/// characters are transliterated, and characters with no transliteration
/// vanish.
pub fn ascii_fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else if let Some((_, rep)) = OVERRIDES.iter().find(|(c, _)| *c == ch) {
            out.push_str(rep);
        } else {
            out.push_str(deunicode::deunicode_char(ch).unwrap_or(""));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_unidecode_on_every_fixture_code_point() {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/device_g2p/fold.json")).unwrap(),
        )
        .unwrap();
        let chars = fixture["chars"].as_object().unwrap();
        assert_eq!(chars.len(), 576, "fixture must cover all four blocks");
        let mut disagreements = Vec::new();
        for (hex, want) in chars {
            let ch = char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap();
            let got = ascii_fold(&ch.to_string());
            if got != want.as_str().unwrap() {
                disagreements.push(format!("    ('\\u{{{hex}}}', {:?}), // deunicode {:?}", want.as_str().unwrap(), got));
            }
        }
        assert!(disagreements.is_empty(), "{} disagreements; add to OVERRIDES:\n{}", disagreements.len(), disagreements.join("\n"));
    }

    #[test]
    fn ascii_passes_through_and_unknowns_vanish() {
        assert_eq!(ascii_fold("Hello, world. 'x' \"y\" 12%"), "Hello, world. 'x' \"y\" 12%");
        // U+0378 is unassigned: unidecode returns "", and so must the fold.
        assert_eq!(ascii_fold("a\u{0378}b"), "ab");
    }
}
