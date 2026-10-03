//! The ASCII fold Sonora's front end applies first (`unidecode`), reproduced
//! with `deunicode` plus rules for where the two disagree.
//!
//! The fold matches unidecode 1.4.0 on every code point from U+0000 to
//! U+10FFFF; the test checks it against `tests/fixtures/device_g2p/fold.json`
//! (from `generate.py`), which holds a hash per block of 4096 code points.
//! deunicode speaks symbols, emoji and whole scripts that unidecode drops
//! (🎉 -> "tada", ⌚ -> "watch"); the fold must drop them too, or the device
//! front end reads words the training front end never saw.

mod unidecode_empty;

use unidecode_empty::UNIDECODE_EMPTY;

/// Ranges of 26 enclosed capitals, A to Z in order, that unidecode folds with
/// the letter in brackets (`open`, `close`) where deunicode differs.
const LETTER_RANGES: &[(char, char, char, char)] = &[
    ('\u{1F130}', '\u{1F149}', '[', ']'), // squared Latin capitals
    ('\u{1F150}', '\u{1F169}', '(', ')'), // negative circled Latin capitals
    ('\u{1F170}', '\u{1F189}', '[', ']'), // negative squared Latin capitals
];

/// unidecode 1.4.0's answer where it is not empty and deunicode gives
/// something else (noted per entry), sorted by code point. A test checks the
/// list is sorted and that none of it is dead.
const OVERRIDES: &[(char, &str)] = &[
    ('\u{00BC}', " 1/4"), // deunicode "1/4"
    ('\u{00BD}', " 1/2"), // deunicode "1/2"
    ('\u{00BE}', " 3/4"), // deunicode "3/4"
    ('\u{014A}', "NG"), // deunicode "ng"
    ('\u{014B}', "ng"), // deunicode "NG"
    ('\u{0404}', "Ie"), // deunicode "E"
    ('\u{0589}', ":"), // deunicode "."
    ('\u{05C3}', "."), // deunicode ":"
    ('\u{05C6}', "n"), // deunicode "^"
    ('\u{05D0}', "A"), // deunicode "'"
    ('\u{05D7}', "H"), // deunicode "kh"
    ('\u{05D8}', "T"), // deunicode "t"
    ('\u{05DA}', "KH"), // deunicode "k"
    ('\u{05DB}', "KH"), // deunicode "k"
    ('\u{05E5}', "TS"), // deunicode "ts"
    ('\u{05E6}', "TS"), // deunicode "ts"
    ('\u{05E7}', "k"), // deunicode "q"
    ('\u{05E9}', "SH"), // deunicode "sh"
    ('\u{05F1}', "OY"), // deunicode "oy"
    ('\u{05F2}', "EY"), // deunicode "i"
    ('\u{1D00}', "A"), // deunicode "a"
    ('\u{1D01}', "AE"), // deunicode "ae"
    ('\u{1D03}', "B"), // deunicode "b"
    ('\u{1D04}', "C"), // deunicode "c"
    ('\u{1D05}', "D"), // deunicode "d"
    ('\u{1D06}', "D"), // deunicode "d"
    ('\u{1D07}', "E"), // deunicode "e"
    ('\u{1D0A}', "J"), // deunicode "j"
    ('\u{1D0B}', "K"), // deunicode "k"
    ('\u{1D0C}', "L"), // deunicode "l"
    ('\u{1D0D}', "M"), // deunicode "m"
    ('\u{1D0E}', "N"), // deunicode "n"
    ('\u{1D0F}', "O"), // deunicode "o"
    ('\u{1D11}', "O"), // deunicode "o"
    ('\u{1D13}', "O"), // deunicode "o"
    ('\u{1D14}', "Oe"), // deunicode "oe"
    ('\u{1D15}', "Ou"), // deunicode "ou"
    ('\u{1D18}', "P"), // deunicode "p"
    ('\u{1D19}', "R"), // deunicode "r"
    ('\u{1D1A}', "R"), // deunicode "r"
    ('\u{1D1B}', "T"), // deunicode "t"
    ('\u{1D1C}', "U"), // deunicode "u"
    ('\u{1D1F}', "m"), // deunicode "w"
    ('\u{1D20}', "V"), // deunicode "v"
    ('\u{1D21}', "W"), // deunicode "w"
    ('\u{1D22}', "Z"), // deunicode "z"
    ('\u{1D2D}', "AE"), // deunicode "Ae"
    ('\u{1D5A}', "m"), // deunicode "w"
    ('\u{1D5D}', "b"), // deunicode "v"
    ('\u{1D66}', "b"), // deunicode "v"
    ('\u{1E9B}', "S"), // deunicode "s"
    ('\u{1E9E}', "SS"), // deunicode "Ss"
    ('\u{204A}', "&"), // deunicode "7"
    ('\u{2052}', "%"), // deunicode "./."
    ('\u{20B4}', "UAH"), // deunicode "HRN"
    ('\u{20B5}', "C|"), // deunicode "C"
    ('\u{20B6}', "L"), // deunicode "lt"
    ('\u{20BA}', "L"), // deunicode "TL"
    ('\u{20BC}', "m"), // deunicode "Manat"
    ('\u{20BD}', "R"), // deunicode "Rb"
    ('\u{20BE}', "l"), // deunicode "Lari"
    ('\u{2100}', " a/c "), // deunicode "a/c"
    ('\u{2101}', " a/s "), // deunicode "a/s"
    ('\u{2103}', "degC"), // deunicode "C"
    ('\u{2105}', " c/o "), // deunicode "c/o"
    ('\u{2106}', " c/u "), // deunicode "c/u"
    ('\u{2109}', "degF"), // deunicode "F"
    ('\u{2116}', "No. "), // deunicode "No" — the NUMERO rule in numbers.rs relies on it
    ('\u{2117}', "(p)"), // deunicode "(P)"
    ('\u{211F}', "R/"), // deunicode "R"
    ('\u{2120}', "(sm)"), // deunicode "SM"
    ('\u{2122}', "(tm)"), // deunicode "tm"
    ('\u{2123}', "V/"), // deunicode "V"
    ('\u{2126}', "ohm"), // deunicode "Ohm"
    ('\u{2139}', "i"), // deunicode "information source "
    ('\u{214E}', "F"), // deunicode "f"
    ('\u{2150}', " 1/7 "), // deunicode "1/7"
    ('\u{2151}', " 1/9 "), // deunicode "1/9"
    ('\u{2152}', " 1/10 "), // deunicode "1/10"
    ('\u{2189}', " 0/3 "), // deunicode "0/3"
    ('\u{232A}', "> "), // deunicode ">"
    ('\u{25D6}', "*"), // deunicode "("
    ('\u{25D7}', "*"), // deunicode ")"
    ('\u{25E2}', "*"), // deunicode "/"
    ('\u{25E3}', "*"), // deunicode "\\"
    ('\u{25E4}', "*"), // deunicode "/"
    ('\u{25E5}', "*"), // deunicode "\\"
    ('\u{25F4}', "#"), // deunicode "O"
    ('\u{25F5}', "#"), // deunicode "O"
    ('\u{25F6}', "#"), // deunicode "O"
    ('\u{25F7}', "#"), // deunicode "O"
    ('\u{2654}', "white king"), // deunicode "white king "
    ('\u{2655}', "white queen"), // deunicode "Q"
    ('\u{2656}', "white rook"), // deunicode "R"
    ('\u{2657}', "white bishop"), // deunicode "B"
    ('\u{2658}', "white knight"), // deunicode "N"
    ('\u{2659}', "white pawn"), // deunicode "P"
    ('\u{265A}', "black king"), // deunicode "k"
    ('\u{265B}', "black queen"), // deunicode "q"
    ('\u{265C}', "black rook"), // deunicode "r"
    ('\u{265D}', "black bishop"), // deunicode "b"
    ('\u{265E}', "black knight"), // deunicode "black knight "
    ('\u{265F}', "black pawn"), // deunicode "chess pawn "
    ('\u{2660}', "spades"), // deunicode "spades "
    ('\u{2661}', "hearts"), // deunicode "white heart "
    ('\u{2662}', "diamonds"), // deunicode "white diamond "
    ('\u{2663}', "clubs"), // deunicode "clubs "
    ('\u{2664}', "spades"), // deunicode "white spade "
    ('\u{2665}', "hearts"), // deunicode "hearts "
    ('\u{2666}', "diamonds"), // deunicode "diamonds "
    ('\u{2667}', "clubs"), // deunicode "white club "
    ('\u{275F}', ","), // deunicode "'"
    ('\u{2760}', ",,"), // deunicode "\""
    ('\u{27E9}', "> "), // deunicode ">"
    ('\u{2802}', "1"), // deunicode ","
    ('\u{2806}', "2"), // deunicode ";"
    ('\u{2812}', "3"), // deunicode ":"
    ('\u{2816}', "6"), // deunicode "!"
    ('\u{2826}', "8"), // deunicode "?"
    ('\u{2832}', "4"), // deunicode "."
    ('\u{2840}', "[d7]"), // deunicode "d7"
    ('\u{2841}', "[d17]"), // deunicode "A"
    ('\u{2842}', "[d27]"), // deunicode "d27"
    ('\u{2843}', "[d127]"), // deunicode "B"
    ('\u{2844}', "[d37]"), // deunicode "d37"
    ('\u{2845}', "[d137]"), // deunicode "K"
    ('\u{2846}', "[d237]"), // deunicode "d237"
    ('\u{2847}', "[d1237]"), // deunicode "L"
    ('\u{2848}', "[d47]"), // deunicode "d47"
    ('\u{2849}', "[d147]"), // deunicode "C"
    ('\u{284A}', "[d247]"), // deunicode "I"
    ('\u{284B}', "[d1247]"), // deunicode "F"
    ('\u{284C}', "[d347]"), // deunicode "d347"
    ('\u{284D}', "[d1347]"), // deunicode "M"
    ('\u{284E}', "[d2347]"), // deunicode "S"
    ('\u{284F}', "[d12347]"), // deunicode "P"
    ('\u{2850}', "[d57]"), // deunicode "d57"
    ('\u{2851}', "[d157]"), // deunicode "E"
    ('\u{2852}', "[d257]"), // deunicode "d257"
    ('\u{2853}', "[d1257]"), // deunicode "H"
    ('\u{2854}', "[d357]"), // deunicode "d357"
    ('\u{2855}', "[d1357]"), // deunicode "O"
    ('\u{2856}', "[d2357]"), // deunicode "d2357"
    ('\u{2857}', "[d12357]"), // deunicode "R"
    ('\u{2858}', "[d457]"), // deunicode "d457"
    ('\u{2859}', "[d1457]"), // deunicode "D"
    ('\u{285A}', "[d2457]"), // deunicode "J"
    ('\u{285B}', "[d12457]"), // deunicode "G"
    ('\u{285C}', "[d3457]"), // deunicode "d3457"
    ('\u{285D}', "[d13457]"), // deunicode "N"
    ('\u{285E}', "[d23457]"), // deunicode "T"
    ('\u{285F}', "[d123457]"), // deunicode "Q"
    ('\u{2860}', "[d67]"), // deunicode "d67"
    ('\u{2861}', "[d167]"), // deunicode "d167"
    ('\u{2862}', "[d267]"), // deunicode "d267"
    ('\u{2863}', "[d1267]"), // deunicode "d1267"
    ('\u{2864}', "[d367]"), // deunicode "d367"
    ('\u{2865}', "[d1367]"), // deunicode "U"
    ('\u{2866}', "[d2367]"), // deunicode "d2367"
    ('\u{2867}', "[d12367]"), // deunicode "V"
    ('\u{2868}', "[d467]"), // deunicode "d467"
    ('\u{2869}', "[d1467]"), // deunicode "d1467"
    ('\u{286A}', "[d2467]"), // deunicode "d2467"
    ('\u{286B}', "[d12467]"), // deunicode "d12467"
    ('\u{286C}', "[d3467]"), // deunicode "d3467"
    ('\u{286D}', "[d13467]"), // deunicode "X"
    ('\u{286E}', "[d23467]"), // deunicode "d23467"
    ('\u{286F}', "[d123467]"), // deunicode "d123467"
    ('\u{2870}', "[d567]"), // deunicode "d567"
    ('\u{2871}', "[d1567]"), // deunicode "d1567"
    ('\u{2872}', "[d2567]"), // deunicode "d2567"
    ('\u{2873}', "[d12567]"), // deunicode "d12567"
    ('\u{2874}', "[d3567]"), // deunicode "d3567"
    ('\u{2875}', "[d13567]"), // deunicode "Z"
    ('\u{2876}', "[d23567]"), // deunicode "d23567"
    ('\u{2877}', "[d123567]"), // deunicode "d123567"
    ('\u{2878}', "[d4567]"), // deunicode "d4567"
    ('\u{2879}', "[d14567]"), // deunicode "d14567"
    ('\u{287A}', "[d24567]"), // deunicode "W"
    ('\u{287B}', "[d124567]"), // deunicode "d124567"
    ('\u{287C}', "[d34567]"), // deunicode "d34567"
    ('\u{287D}', "[d134567]"), // deunicode "Y"
    ('\u{287E}', "[d234567]"), // deunicode "d234567"
    ('\u{287F}', "[d1234567]"), // deunicode "d1234567"
    ('\u{2880}', "[d8]"), // deunicode "d8"
    ('\u{2881}', "[d18]"), // deunicode "d18"
    ('\u{2882}', "[d28]"), // deunicode "d28"
    ('\u{2883}', "[d128]"), // deunicode "d128"
    ('\u{2884}', "[d38]"), // deunicode "d38"
    ('\u{2885}', "[d138]"), // deunicode "d138"
    ('\u{2886}', "[d238]"), // deunicode "d238"
    ('\u{2887}', "[d1238]"), // deunicode "d1238"
    ('\u{2888}', "[d48]"), // deunicode "d48"
    ('\u{2889}', "[d148]"), // deunicode "d148"
    ('\u{288A}', "[d248]"), // deunicode "d248"
    ('\u{288B}', "[d1248]"), // deunicode "d1248"
    ('\u{288C}', "[d348]"), // deunicode "d348"
    ('\u{288D}', "[d1348]"), // deunicode "d1348"
    ('\u{288E}', "[d2348]"), // deunicode "d2348"
    ('\u{288F}', "[d12348]"), // deunicode "d12348"
    ('\u{2890}', "[d58]"), // deunicode "d58"
    ('\u{2891}', "[d158]"), // deunicode "d158"
    ('\u{2892}', "[d258]"), // deunicode "d258"
    ('\u{2893}', "[d1258]"), // deunicode "d1258"
    ('\u{2894}', "[d358]"), // deunicode "d358"
    ('\u{2895}', "[d1358]"), // deunicode "d1358"
    ('\u{2896}', "[d2358]"), // deunicode "d2358"
    ('\u{2897}', "[d12358]"), // deunicode "d12358"
    ('\u{2898}', "[d458]"), // deunicode "d458"
    ('\u{2899}', "[d1458]"), // deunicode "d1458"
    ('\u{289A}', "[d2458]"), // deunicode "d2458"
    ('\u{289B}', "[d12458]"), // deunicode "d12458"
    ('\u{289C}', "[d3458]"), // deunicode "d3458"
    ('\u{289D}', "[d13458]"), // deunicode "d13458"
    ('\u{289E}', "[d23458]"), // deunicode "d23458"
    ('\u{289F}', "[d123458]"), // deunicode "d123458"
    ('\u{28A0}', "[d68]"), // deunicode "d68"
    ('\u{28A1}', "[d168]"), // deunicode "d168"
    ('\u{28A2}', "[d268]"), // deunicode "d268"
    ('\u{28A3}', "[d1268]"), // deunicode "d1268"
    ('\u{28A4}', "[d368]"), // deunicode "d368"
    ('\u{28A5}', "[d1368]"), // deunicode "d1368"
    ('\u{28A6}', "[d2368]"), // deunicode "d2368"
    ('\u{28A7}', "[d12368]"), // deunicode "d12368"
    ('\u{28A8}', "[d468]"), // deunicode "d468"
    ('\u{28A9}', "[d1468]"), // deunicode "d1468"
    ('\u{28AA}', "[d2468]"), // deunicode "d2468"
    ('\u{28AB}', "[d12468]"), // deunicode "d12468"
    ('\u{28AC}', "[d3468]"), // deunicode "d3468"
    ('\u{28AD}', "[d13468]"), // deunicode "d13468"
    ('\u{28AE}', "[d23468]"), // deunicode "d23468"
    ('\u{28AF}', "[d123468]"), // deunicode "d123468"
    ('\u{28B0}', "[d568]"), // deunicode "d568"
    ('\u{28B1}', "[d1568]"), // deunicode "d1568"
    ('\u{28B2}', "[d2568]"), // deunicode "d2568"
    ('\u{28B3}', "[d12568]"), // deunicode "d12568"
    ('\u{28B4}', "[d3568]"), // deunicode "d3568"
    ('\u{28B5}', "[d13568]"), // deunicode "d13568"
    ('\u{28B6}', "[d23568]"), // deunicode "d23568"
    ('\u{28B7}', "[d123568]"), // deunicode "d123568"
    ('\u{28B8}', "[d4568]"), // deunicode "d4568"
    ('\u{28B9}', "[d14568]"), // deunicode "d14568"
    ('\u{28BA}', "[d24568]"), // deunicode "d24568"
    ('\u{28BB}', "[d124568]"), // deunicode "d124568"
    ('\u{28BC}', "[d34568]"), // deunicode "d34568"
    ('\u{28BD}', "[d134568]"), // deunicode "d134568"
    ('\u{28BE}', "[d234568]"), // deunicode "d234568"
    ('\u{28BF}', "[d1234568]"), // deunicode "d1234568"
    ('\u{28C0}', "[d78]"), // deunicode "d78"
    ('\u{28C1}', "[d178]"), // deunicode "d178"
    ('\u{28C2}', "[d278]"), // deunicode "d278"
    ('\u{28C3}', "[d1278]"), // deunicode "d1278"
    ('\u{28C4}', "[d378]"), // deunicode "d378"
    ('\u{28C5}', "[d1378]"), // deunicode "d1378"
    ('\u{28C6}', "[d2378]"), // deunicode "d2378"
    ('\u{28C7}', "[d12378]"), // deunicode "d12378"
    ('\u{28C8}', "[d478]"), // deunicode "d478"
    ('\u{28C9}', "[d1478]"), // deunicode "d1478"
    ('\u{28CA}', "[d2478]"), // deunicode "d2478"
    ('\u{28CB}', "[d12478]"), // deunicode "d12478"
    ('\u{28CC}', "[d3478]"), // deunicode "d3478"
    ('\u{28CD}', "[d13478]"), // deunicode "d13478"
    ('\u{28CE}', "[d23478]"), // deunicode "d23478"
    ('\u{28CF}', "[d123478]"), // deunicode "d123478"
    ('\u{28D0}', "[d578]"), // deunicode "d578"
    ('\u{28D1}', "[d1578]"), // deunicode "d1578"
    ('\u{28D2}', "[d2578]"), // deunicode "d2578"
    ('\u{28D3}', "[d12578]"), // deunicode "d12578"
    ('\u{28D4}', "[d3578]"), // deunicode "d3578"
    ('\u{28D5}', "[d13578]"), // deunicode "d13578"
    ('\u{28D6}', "[d23578]"), // deunicode "d23578"
    ('\u{28D7}', "[d123578]"), // deunicode "d123578"
    ('\u{28D8}', "[d4578]"), // deunicode "d4578"
    ('\u{28D9}', "[d14578]"), // deunicode "d14578"
    ('\u{28DA}', "[d24578]"), // deunicode "d24578"
    ('\u{28DB}', "[d124578]"), // deunicode "d124578"
    ('\u{28DC}', "[d34578]"), // deunicode "d34578"
    ('\u{28DD}', "[d134578]"), // deunicode "d134578"
    ('\u{28DE}', "[d234578]"), // deunicode "d234578"
    ('\u{28DF}', "[d1234578]"), // deunicode "d1234578"
    ('\u{28E0}', "[d678]"), // deunicode "d678"
    ('\u{28E1}', "[d1678]"), // deunicode "d1678"
    ('\u{28E2}', "[d2678]"), // deunicode "d2678"
    ('\u{28E3}', "[d12678]"), // deunicode "d12678"
    ('\u{28E4}', "[d3678]"), // deunicode "d3678"
    ('\u{28E5}', "[d13678]"), // deunicode "d13678"
    ('\u{28E6}', "[d23678]"), // deunicode "d23678"
    ('\u{28E7}', "[d123678]"), // deunicode "d123678"
    ('\u{28E8}', "[d4678]"), // deunicode "d4678"
    ('\u{28E9}', "[d14678]"), // deunicode "d14678"
    ('\u{28EA}', "[d24678]"), // deunicode "d24678"
    ('\u{28EB}', "[d124678]"), // deunicode "d124678"
    ('\u{28EC}', "[d34678]"), // deunicode "d34678"
    ('\u{28ED}', "[d134678]"), // deunicode "d134678"
    ('\u{28EE}', "[d234678]"), // deunicode "d234678"
    ('\u{28EF}', "[d1234678]"), // deunicode "d1234678"
    ('\u{28F0}', "[d5678]"), // deunicode "d5678"
    ('\u{28F1}', "[d15678]"), // deunicode "d15678"
    ('\u{28F2}', "[d25678]"), // deunicode "d25678"
    ('\u{28F3}', "[d125678]"), // deunicode "d125678"
    ('\u{28F4}', "[d35678]"), // deunicode "d35678"
    ('\u{28F5}', "[d135678]"), // deunicode "d135678"
    ('\u{28F6}', "[d235678]"), // deunicode "d235678"
    ('\u{28F7}', "[d1235678]"), // deunicode "d1235678"
    ('\u{28F8}', "[d45678]"), // deunicode "d45678"
    ('\u{28F9}', "[d145678]"), // deunicode "d145678"
    ('\u{28FA}', "[d245678]"), // deunicode "d245678"
    ('\u{28FB}', "[d1245678]"), // deunicode "d1245678"
    ('\u{28FC}', "[d345678]"), // deunicode "d345678"
    ('\u{28FD}', "[d1345678]"), // deunicode "d1345678"
    ('\u{28FE}', "[d2345678]"), // deunicode "d2345678"
    ('\u{28FF}', "[d12345678]"), // deunicode "d12345678"
    ('\u{2984}', "} "), // deunicode "}"
    ('\u{2A74}', "::="), // deunicode "="
    ('\u{2E00}', "r"), // deunicode "+"
    ('\u{2E01}', "r."), // deunicode "+"
    ('\u{2E06}', "T"), // deunicode "+"
    ('\u{2E07}', "T."), // deunicode "+"
    ('\u{2E09}', "s"), // deunicode "+"
    ('\u{2E0B}', "[]"), // deunicode "*"
    ('\u{2E0F}', "__"), // deunicode "_"
    ('\u{2E12}', ">"), // deunicode ","
    ('\u{2E13}', "%"), // deunicode "/"
    ('\u{2E20}', "|-"), // deunicode "["
    ('\u{2E21}', "-|"), // deunicode "]"
    ('\u{2E26}', "<="), // deunicode "("
    ('\u{2E27}', "=>"), // deunicode ")"
    ('\u{2E2C}', "::"), // deunicode "_"
    ('\u{2E30}', "o"), // deunicode "*"
    ('\u{2E31}', "."), // deunicode "-"
    ('\u{2E33}', "."), // deunicode "-"
    ('\u{2E3A}', "----"), // deunicode "--"
    ('\u{2E3B}', "------"), // deunicode "---"
    ('\u{2E3C}', "x"), // deunicode "."
    ('\u{2E3D}', "|"), // deunicode "_"
    ('\u{2E43}', "`--"), // deunicode "-"
    ('\u{3057}', "shi"), // deunicode "si"
    ('\u{3061}', "chi"), // deunicode "ti"
    ('\u{3063}', "tsu"), // deunicode "tu"
    ('\u{3064}', "tsu"), // deunicode "tu"
    ('\u{30B7}', "shi"), // deunicode "si"
    ('\u{30C1}', "chi"), // deunicode "ti"
    ('\u{30C3}', "tsu"), // deunicode "tu"
    ('\u{30C4}', "tsu"), // deunicode "tu"
    ('\u{30FB}', "*"), // deunicode "-"
    ('\u{30FC}', "-"), // deunicode ""
    ('\u{3319}', "gram ton"), // deunicode "gram ton "
    ('\u{3371}', "hPa"), // deunicode "HPA"
    ('\u{3378}', "dm^2"), // deunicode "dm2"
    ('\u{3379}', "dm^3"), // deunicode "dm3"
    ('\u{338C}', "uF"), // deunicode "microFarad"
    ('\u{338D}', "ug"), // deunicode "microgram"
    ('\u{3395}', "ul"), // deunicode "microliter"
    ('\u{33A3}', "mm^3"), // deunicode "mm^4"
    ('\u{33B2}', "us"), // deunicode "microsecond"
    ('\u{33B6}', "uV"), // deunicode "microvolt"
    ('\u{33BC}', "uW"), // deunicode "microwatt"
    ('\u{4E48}', "Yao "), // deunicode "Me "
    ('\u{4EC0}', "Shi "), // deunicode "Shen "
    ('\u{4EF7}', "Jie "), // deunicode "Jia "
    ('\u{65C5}', "Lu "), // deunicode "Lv "
    ('\u{672F}', "Zhu "), // deunicode "Shu "
    ('\u{FB1D}', "i"), // deunicode "yi"
    ('\u{FB1F}', "AY"), // deunicode "ay"
    ('\u{FB21}', "A"), // deunicode "'"
    ('\u{FB24}', "KH"), // deunicode "k"
    ('\u{FB27}', "r"), // deunicode "m"
    ('\u{FB2A}', "SH"), // deunicode "sh"
    ('\u{FB2B}', "S"), // deunicode "s"
    ('\u{FB2C}', "SH"), // deunicode "sh"
    ('\u{FB2D}', "S"), // deunicode "s"
    ('\u{FB30}', "A"), // deunicode "'"
    ('\u{FB3A}', "KH"), // deunicode "k"
    ('\u{FB3B}', "KH"), // deunicode "k"
    ('\u{FB3E}', "m"), // deunicode "l"
    ('\u{FB41}', "s"), // deunicode "n"
    ('\u{FB46}', "TS"), // deunicode "ts"
    ('\u{FB47}', "k"), // deunicode "ts"
    ('\u{FB49}', "SH"), // deunicode "sh"
    ('\u{FB4B}', "o"), // deunicode "vo"
    ('\u{FB4C}', "v"), // deunicode "b"
    ('\u{FB4D}', "KH"), // deunicode "k"
    ('\u{FB4E}', "f"), // deunicode "p"
    ('\u{FB4F}', "EL"), // deunicode "l"
    ('\u{1D6A8}', "Alpha"), // deunicode "A"
    ('\u{1D6A9}', "Beta"), // deunicode "B"
    ('\u{1D6AA}', "Gamma"), // deunicode "G"
    ('\u{1D6AB}', "Delta"), // deunicode "D"
    ('\u{1D6AC}', "Epsilon"), // deunicode "E"
    ('\u{1D6AD}', "Zeta"), // deunicode "Z"
    ('\u{1D6AE}', "Eta"), // deunicode "E"
    ('\u{1D6AF}', "Theta"), // deunicode "Th"
    ('\u{1D6B0}', "Iota"), // deunicode "I"
    ('\u{1D6B1}', "Kappa"), // deunicode "K"
    ('\u{1D6B2}', "Lamda"), // deunicode "L"
    ('\u{1D6B3}', "Mu"), // deunicode "M"
    ('\u{1D6B4}', "Nu"), // deunicode "N"
    ('\u{1D6B5}', "Xi"), // deunicode "X"
    ('\u{1D6B6}', "Omicron"), // deunicode "O"
    ('\u{1D6B7}', "Pi"), // deunicode "P"
    ('\u{1D6B8}', "Rho"), // deunicode "R"
    ('\u{1D6B9}', "Theta"), // deunicode "Th"
    ('\u{1D6BA}', "Sigma"), // deunicode "S"
    ('\u{1D6BB}', "Tau"), // deunicode "T"
    ('\u{1D6BC}', "Upsilon"), // deunicode "Y"
    ('\u{1D6BD}', "Phi"), // deunicode "Ph"
    ('\u{1D6BE}', "Chi"), // deunicode "Ch"
    ('\u{1D6BF}', "Psi"), // deunicode "Ps"
    ('\u{1D6C0}', "Omega"), // deunicode "O"
    ('\u{1D6C1}', "nabla"), // deunicode "D"
    ('\u{1D6C2}', "alpha"), // deunicode "a"
    ('\u{1D6C3}', "beta"), // deunicode "b"
    ('\u{1D6C4}', "gamma"), // deunicode "g"
    ('\u{1D6C5}', "delta"), // deunicode "d"
    ('\u{1D6C6}', "epsilon"), // deunicode "e"
    ('\u{1D6C7}', "zeta"), // deunicode "z"
    ('\u{1D6C8}', "eta"), // deunicode "e"
    ('\u{1D6C9}', "theta"), // deunicode "th"
    ('\u{1D6CA}', "iota"), // deunicode "i"
    ('\u{1D6CB}', "kappa"), // deunicode "k"
    ('\u{1D6CC}', "lamda"), // deunicode "l"
    ('\u{1D6CD}', "mu"), // deunicode "m"
    ('\u{1D6CE}', "nu"), // deunicode "n"
    ('\u{1D6CF}', "xi"), // deunicode "x"
    ('\u{1D6D0}', "omicron"), // deunicode "o"
    ('\u{1D6D1}', "pi"), // deunicode "p"
    ('\u{1D6D2}', "rho"), // deunicode "r"
    ('\u{1D6D3}', "sigma"), // deunicode "s"
    ('\u{1D6D4}', "sigma"), // deunicode "s"
    ('\u{1D6D5}', "tau"), // deunicode "t"
    ('\u{1D6D6}', "upsilon"), // deunicode "y"
    ('\u{1D6D7}', "phi"), // deunicode "ph"
    ('\u{1D6D8}', "chi"), // deunicode "ch"
    ('\u{1D6D9}', "psi"), // deunicode "ps"
    ('\u{1D6DA}', "omega"), // deunicode "o"
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
    ('\u{1F670}', "et"), // deunicode "&"
    ('\u{1F671}', "et"), // deunicode "&"
    ('\u{1F672}', "et"), // deunicode "&"
    ('\u{1F673}', "et"), // deunicode "&"
    ('\u{1F678}', ",,"), // deunicode "\""
];

/// Folds `text` to ASCII as `unidecode` does: ASCII passes through, other
/// characters are transliterated, and characters with no transliteration
/// vanish.
pub fn ascii_fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            out.push_str(&fold_non_ascii(ch));
        }
    }
    out
}

fn fold_non_ascii(ch: char) -> std::borrow::Cow<'static, str> {
    let cp = ch as u32;
    if folds_to_nothing(cp) {
        return "".into();
    }
    if let Ok(i) = OVERRIDES.binary_search_by_key(&ch, |&(c, _)| c) {
        return OVERRIDES[i].1.into();
    }
    if let Some(&(lo, _, open, close)) = LETTER_RANGES.iter().find(|(lo, hi, _, _)| (*lo..=*hi).contains(&ch)) {
        // Each range is 26 long, so the offset is 0..=25.
        let letter = char::from_u32('A' as u32 + (cp - lo as u32)).unwrap_or('?');
        return format!("{open}{letter}{close}").into();
    }
    deunicode::deunicode_char(ch).unwrap_or("").into()
}

/// Whether `cp` lies in a `UNIDECODE_EMPTY` range.
fn folds_to_nothing(cp: u32) -> bool {
    UNIDECODE_EMPTY
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                std::cmp::Ordering::Less
            } else if lo > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv1a64(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3))
    }

    #[test]
    fn matches_unidecode_on_every_code_point() {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/device_g2p/fold.json")).unwrap(),
        )
        .unwrap();
        let hex = |key: &str| u32::from_str_radix(fixture[key].as_str().unwrap(), 16).unwrap();
        let (first, last) = (hex("first"), hex("last"));
        let block = fixture["block"].as_u64().unwrap() as u32;
        let want = fixture["fnv1a64"].as_object().unwrap();
        assert_eq!((first, last, block), (0, 0x10FFFF, 0x1000), "the fixture must cover U+0000..U+10FFFF");
        assert_eq!(want.len(), 272, "one hash per block");

        let mut lines: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
        for ch in (first..=last).filter_map(char::from_u32) {
            let line = format!("{:04X}={}\n", ch as u32, ascii_fold(&ch.to_string()));
            lines.entry(ch as u32 / block * block).or_default().push_str(&line);
        }
        let mut failing = Vec::new();
        for (start, text) in &lines {
            let key = format!("{start:04X}");
            let got = format!("{:016x}", fnv1a64(text.as_bytes()));
            if want.get(&key).and_then(|v| v.as_str()) != Some(got.as_str()) {
                let path = std::env::temp_dir().join(format!("prosodia-fold-{key}.txt"));
                std::fs::write(&path, text).unwrap();
                failing.push(format!("{key} {:04X}: {}", start + block - 1, path.display()));
            }
        }
        assert_eq!(lines.len(), want.len(), "blocks checked");
        assert!(
            failing.is_empty(),
            "the fold disagrees with unidecode in {} block(s); the fold's lines are written to the files \
             below. Diff each against `../Sonora/github/.venv/bin/python \
             crates/actor/tests/fixtures/device_g2p/generate.py --dump LO HI`:\n{}",
            failing.len(),
            failing.join("\n")
        );
    }

    #[test]
    fn tables_are_sorted_and_no_override_is_dead() {
        assert!(UNIDECODE_EMPTY.windows(2).all(|w| w[0].1 < w[1].0), "UNIDECODE_EMPTY must be sorted and disjoint");
        assert!(UNIDECODE_EMPTY.iter().all(|(lo, hi)| lo <= hi));
        assert!(OVERRIDES.windows(2).all(|w| w[0].0 < w[1].0), "OVERRIDES must be sorted by code point, without repeats");
        for &(ch, rep) in OVERRIDES {
            assert!(!rep.is_empty(), "U+{:04X}: empty values belong in UNIDECODE_EMPTY", ch as u32);
            assert!(!folds_to_nothing(ch as u32), "U+{:04X}: shadowed by UNIDECODE_EMPTY", ch as u32);
            assert_ne!(deunicode::deunicode_char(ch).unwrap_or(""), rep, "U+{:04X}: deunicode already agrees", ch as u32);
        }
        for &(lo, hi, _, _) in LETTER_RANGES {
            assert_eq!(hi as u32 - lo as u32, 25, "U+{:04X}: a letter range is A to Z", lo as u32);
            let range = lo..=hi;
            assert!((lo as u32..=hi as u32).all(|cp| !folds_to_nothing(cp)), "U+{:04X}: shadowed by UNIDECODE_EMPTY", lo as u32);
            assert!(OVERRIDES.iter().all(|(c, _)| !range.contains(c)), "U+{:04X}: overlaps OVERRIDES", lo as u32);
            assert!(
                range.clone().any(|c| deunicode::deunicode_char(c).unwrap_or("") != fold_non_ascii(c)),
                "U+{:04X}: deunicode already agrees on the whole range",
                lo as u32
            );
        }
    }

    #[test]
    fn symbols_outside_the_old_fixture_ranges_vanish() {
        // Issue #23. deunicode reads these as "watch", "alarm clock", arrows,
        // squares, a circle and a part alternation mark.
        for s in ["\u{231A}", "\u{23F0}", "\u{2934}", "\u{2935}", "\u{2B1B}", "\u{2B1C}", "\u{2B55}", "\u{303D}"] {
            assert_eq!(ascii_fold(&format!("a{s}b")), "ab", "U+{:04X}", s.chars().next().unwrap() as u32);
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

