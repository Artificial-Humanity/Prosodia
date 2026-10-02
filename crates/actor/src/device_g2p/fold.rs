//! The ASCII fold Sonora's front end applies first (`unidecode`), reproduced
//! with `deunicode` plus rules for where the two disagree.
//!
//! The fixture (`tests/fixtures/device_g2p/fold.json`, from `generate.py`)
//! records unidecode 1.4.0 over Latin-1 through Latin Extended-B, General
//! Punctuation, Letterlike Symbols, Arrows, Miscellaneous Symbols and
//! Dingbats, and U+1F000-1FAFF (emoji and pictographs). deunicode speaks
//! emoji and symbols that unidecode drops (🎉 -> "tada", ✓ -> "OK"); the
//! fold must drop them too or the device front end reads words the training
//! front end never saw. Outside those ranges deunicode is unverified.

/// How a `RANGE_RULES` range folds.
#[derive(Clone, Copy)]
enum Rule {
    /// unidecode has nothing here: the character vanishes.
    Empty,
    /// A run of 26 enclosed capitals, A to Z in order, folded `(A)`.
    Parens,
    /// The same, folded `[A]`.
    Brackets,
}

/// Inclusive ranges where unidecode 1.4.0 follows a pattern deunicode does
/// not, checked after `OVERRIDES`. Each was read off the fixture.
const RANGE_RULES: &[(char, char, Rule)] = &[
    ('\u{2135}', '\u{2138}', Rule::Empty),    // Hebrew letter symbols
    ('\u{213C}', '\u{2144}', Rule::Empty),    // double-struck pi..turned sans-serif Y
    ('\u{214A}', '\u{214C}', Rule::Empty),    // property line..per sign
    ('\u{21F4}', '\u{21FF}', Rule::Empty),    // the arrows unidecode's table stops before
    ('\u{2600}', '\u{27BF}', Rule::Empty),    // Misc Symbols + Dingbats (non-empty ones in OVERRIDES)
    ('\u{1F000}', '\u{1F0FF}', Rule::Empty),  // Mahjong, Domino, Playing Cards
    ('\u{1F10B}', '\u{1F10F}', Rule::Empty),
    ('\u{1F12D}', '\u{1F12F}', Rule::Empty),
    ('\u{1F130}', '\u{1F149}', Rule::Brackets), // squared Latin capitals
    ('\u{1F14A}', '\u{1F14F}', Rule::Empty),
    ('\u{1F150}', '\u{1F169}', Rule::Parens),   // negative circled Latin capitals
    ('\u{1F16D}', '\u{1F16F}', Rule::Empty),
    ('\u{1F170}', '\u{1F189}', Rule::Brackets), // negative squared Latin capitals
    ('\u{1F18A}', '\u{1F18F}', Rule::Empty),
    ('\u{1F1AE}', '\u{1F1FF}', Rule::Empty),  // incl. regional indicators (flags)
    ('\u{1F200}', '\u{1FAFF}', Rule::Empty),  // emoji and pictographs (non-empty ones in OVERRIDES)
];

/// unidecode 1.4.0's answer for single code points, checked first. Every
/// entry is the fixture's value for its code point (a test enforces it), and
/// is there for one of two reasons, noted per entry: deunicode gives
/// something else ("deunicode ..."), or the code point sits in an `Empty`
/// `RANGE_RULES` range but unidecode does have a value for it ("range rule").
const OVERRIDES: &[(char, &str)] = &[
    ('\u{00BC}', " 1/4"), // deunicode "1/4"
    ('\u{00BD}', " 1/2"), // deunicode "1/2"
    ('\u{00BE}', " 3/4"), // deunicode "3/4"
    ('\u{014A}', "NG"), // deunicode "ng"
    ('\u{014B}', "ng"), // deunicode "NG"
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
    // Letterlike Symbols
    ('\u{2100}', " a/c "), // deunicode "a/c"
    ('\u{2101}', " a/s "), // deunicode "a/s"
    ('\u{2103}', "degC"), // deunicode "C"
    ('\u{2104}', ""), // deunicode "CL"
    ('\u{2105}', " c/o "), // deunicode "c/o"
    ('\u{2106}', " c/u "), // deunicode "c/u"
    ('\u{2107}', ""), // deunicode "E"
    ('\u{2108}', ""), // deunicode "g"
    ('\u{2109}', "degF"), // deunicode "F"
    ('\u{210F}', ""), // deunicode "h"
    ('\u{2114}', ""), // deunicode "#"
    ('\u{2116}', "No. "), // deunicode "No" — the NUMERO rule in numbers.rs relies on it
    ('\u{2117}', "(p)"), // deunicode "(P)"
    ('\u{2118}', ""), // deunicode "p"
    ('\u{211E}', ""), // deunicode "Rx"
    ('\u{211F}', "R/"), // deunicode "R"
    ('\u{2120}', "(sm)"), // deunicode "SM"
    ('\u{2122}', "(tm)"), // deunicode "tm"
    ('\u{2123}', "V/"), // deunicode "V"
    ('\u{2125}', ""), // deunicode "oz"
    ('\u{2126}', "ohm"), // deunicode "Ohm"
    ('\u{2127}', ""), // deunicode "Mho"
    ('\u{2129}', ""), // deunicode "i"
    ('\u{2139}', "i"), // deunicode "information source "
    ('\u{213A}', ""), // deunicode "Q"
    ('\u{214E}', "F"), // deunicode "f"
    ('\u{214F}', ""), // deunicode "Sh"
    // Misc Symbols, Dingbats, Enclosed Alphanumeric Supplement, Ornamental Dingbats
    ('\u{2654}', "white king"), // range rule ""
    ('\u{2655}', "white queen"), // range rule ""
    ('\u{2656}', "white rook"), // range rule ""
    ('\u{2657}', "white bishop"), // range rule ""
    ('\u{2658}', "white knight"), // range rule ""
    ('\u{2659}', "white pawn"), // range rule ""
    ('\u{265A}', "black king"), // range rule ""
    ('\u{265B}', "black queen"), // range rule ""
    ('\u{265C}', "black rook"), // range rule ""
    ('\u{265D}', "black bishop"), // range rule ""
    ('\u{265E}', "black knight"), // range rule ""
    ('\u{265F}', "black pawn"), // range rule ""
    ('\u{2660}', "spades"), // range rule ""
    ('\u{2661}', "hearts"), // range rule ""
    ('\u{2662}', "diamonds"), // range rule ""
    ('\u{2663}', "clubs"), // range rule ""
    ('\u{2664}', "spades"), // range rule ""
    ('\u{2665}', "hearts"), // range rule ""
    ('\u{2666}', "diamonds"), // range rule ""
    ('\u{2667}', "clubs"), // range rule ""
    ('\u{266F}', "#"), // range rule ""
    ('\u{2731}', "*"), // range rule ""
    ('\u{2758}', "|"), // range rule ""
    ('\u{275C}', "'"), // range rule ""
    ('\u{275D}', "\""), // range rule ""
    ('\u{275E}', "\""), // range rule ""
    ('\u{275F}', ","), // range rule ""
    ('\u{2760}', ",,"), // range rule ""
    ('\u{2762}', "!"), // range rule ""
    ('\u{1F12A}', "<S>"), // deunicode "[S]"
    ('\u{1F12B}', "(C)"), // deunicode "C"
    ('\u{1F12C}', "(R)"), // deunicode "R"
    ('\u{1F16A}', "(mc)"), // deunicode "MC"
    ('\u{1F16B}', "(md)"), // deunicode "MD"
    ('\u{1F16C}', "(mr)"), // deunicode "MR"
    ('\u{1F191}', "[CL]"), // deunicode "cl "
    ('\u{1F192}', "[COOL]"), // deunicode "cool "
    ('\u{1F193}', "[FREE]"), // deunicode "free "
    ('\u{1F194}', "[ID]"), // deunicode "id "
    ('\u{1F195}', "[NEW]"), // deunicode "new "
    ('\u{1F196}', "[NG]"), // deunicode "ng "
    ('\u{1F197}', "[OK]"), // deunicode "ok "
    ('\u{1F198}', "[SOS]"), // deunicode "sos "
    ('\u{1F199}', "[UP!]"), // deunicode "up "
    ('\u{1F19A}', "[VS]"), // deunicode "vs "
    ('\u{1F19B}', "[3D]"), // deunicode "3D"
    ('\u{1F19C}', "[2nd-Scr]"), // deunicode "2ndScr"
    ('\u{1F19D}', "[2K]"), // deunicode "2K"
    ('\u{1F19E}', "[4K]"), // deunicode "4K"
    ('\u{1F19F}', "[8K]"), // deunicode "8K"
    ('\u{1F1A0}', "[5.1]"), // deunicode "5.1"
    ('\u{1F1A1}', "[7.1]"), // deunicode "7.1"
    ('\u{1F1A2}', "[22.2]"), // deunicode "22.2"
    ('\u{1F1A3}', "[60P]"), // deunicode "60P"
    ('\u{1F1A4}', "[120P]"), // deunicode "120P"
    ('\u{1F1A5}', "[d]"), // deunicode "d"
    ('\u{1F1A6}', "[HC]"), // deunicode "HC"
    ('\u{1F1A7}', "[HDR]"), // deunicode "HDR"
    ('\u{1F1A8}', "[Hi-res]"), // deunicode "Hi-Res"
    ('\u{1F1A9}', "[Lossless]"), // deunicode "Lossless"
    ('\u{1F1AA}', "[SHV]"), // deunicode "SHV"
    ('\u{1F1AB}', "[UHD]"), // deunicode "UHD"
    ('\u{1F1AC}', "[VOD]"), // deunicode "VOD"
    ('\u{1F1AD}', "(m)"), // deunicode "*M*"
    ('\u{1F670}', "et"), // range rule ""
    ('\u{1F671}', "et"), // range rule ""
    ('\u{1F672}', "et"), // range rule ""
    ('\u{1F673}', "et"), // range rule ""
    ('\u{1F674}', "&"), // range rule ""
    ('\u{1F675}', "&"), // range rule ""
    ('\u{1F676}', "\""), // range rule ""
    ('\u{1F677}', "\""), // range rule ""
    ('\u{1F678}', ",,"), // range rule ""
    ('\u{1F679}', "!?"), // range rule ""
    ('\u{1F67A}', "!?"), // range rule ""
    ('\u{1F67B}', "!?"), // range rule ""
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
        } else if let Some(&(lo, _, rule)) = RANGE_RULES.iter().find(|(lo, hi, _)| (*lo..=*hi).contains(&ch)) {
            // Only Parens/Brackets ranges are 26 long, so the offset is 0..=25 there.
            let letter = || char::from_u32('A' as u32 + (ch as u32 - lo as u32)).unwrap_or('?');
            match rule {
                Rule::Empty => {}
                Rule::Parens => { out.push('('); out.push(letter()); out.push(')'); }
                Rule::Brackets => { out.push('['); out.push(letter()); out.push(']'); }
            }
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
        assert_eq!(chars.len(), 4032, "fixture must cover all eight ranges");
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
    fn every_override_is_the_fixture_value() {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/device_g2p/fold.json")).unwrap(),
        )
        .unwrap();
        for (ch, rep) in OVERRIDES {
            let hex = format!("{:04X}", *ch as u32);
            assert_eq!(fixture["chars"][&hex].as_str(), Some(*rep), "OVERRIDES U+{hex} is not the fixture's value");
        }
    }

    #[test]
    fn emoji_and_symbols_unidecode_drops_vanish() {
        // deunicode reads these as "tada", "grinning", "OK", "heavy check mark".
        assert_eq!(ascii_fold("Done\u{1F389}!"), "Done!");
        assert_eq!(ascii_fold("a\u{1F600}b\u{2713}c\u{2714}"), "abc");
        assert_eq!(ascii_fold("\u{1F1FA}\u{1F1F8}"), "", "regional indicators (a flag)");
        assert_eq!(ascii_fold("\u{2665}\u{2122}"), "hearts(tm)");
    }

    #[test]
    fn ascii_passes_through_and_unknowns_vanish() {
        assert_eq!(ascii_fold("Hello, world. 'x' \"y\" 12%"), "Hello, world. 'x' \"y\" 12%");
        // U+0378 is unassigned: unidecode returns "", and so must the fold.
        assert_eq!(ascii_fold("a\u{0378}b"), "ab");
    }
}
