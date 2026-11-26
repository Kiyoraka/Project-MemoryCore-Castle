//! Simple-font encodings (Standard, WinAnsi, MacRoman), glyph names to Unicode (the Latin part of the
//! Adobe Glyph List plus the `uniXXXX`/`uXXXXX` forms), and the CFF standard strings (the ISOAdobe
//! part).

/// ASCII 32..=126 as glyph names in StandardEncoding (39 is quoteright, 96 quoteleft).
const ASCII_NAMES: [&str; 95] = [
    "space", "exclam", "quotedbl", "numbersign", "dollar", "percent", "ampersand", "quoteright", "parenleft", "parenright", "asterisk",
    "plus", "comma", "hyphen", "period", "slash", "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "colon",
    "semicolon", "less", "equal", "greater", "question", "at", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O",
    "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "bracketleft", "backslash", "bracketright", "asciicircum", "underscore",
    "quoteleft", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x",
    "y", "z", "braceleft", "bar", "braceright", "asciitilde",
];

/// The CFF standard strings 0..=228 (ISOAdobe); `space`..`asciitilde` are ASCII_NAMES.
const CFF_EXTRA: [&str; 133] = [
    "exclamdown", "cent", "sterling", "fraction", "yen", "florin", "section", "currency", "quotesingle", "quotedblleft", "guillemotleft",
    "guilsinglleft", "guilsinglright", "fi", "fl", "endash", "dagger", "daggerdbl", "periodcentered", "paragraph", "bullet",
    "quotesinglbase", "quotedblbase", "quotedblright", "guillemotright", "ellipsis", "perthousand", "questiondown", "grave", "acute",
    "circumflex", "tilde", "macron", "breve", "dotaccent", "dieresis", "ring", "cedilla", "hungarumlaut", "ogonek", "caron", "emdash", "AE",
    "ordfeminine", "Lslash", "Oslash", "OE", "ordmasculine", "ae", "dotlessi", "lslash", "oslash", "oe", "germandbls", "onesuperior",
    "logicalnot", "mu", "trademark", "Eth", "onehalf", "plusminus", "Thorn", "onequarter", "divide", "brokenbar", "degree", "thorn",
    "threequarters", "twosuperior", "registered", "minus", "eth", "multiply", "threesuperior", "copyright", "Aacute", "Acircumflex",
    "Adieresis", "Agrave", "Aring", "Atilde", "Ccedilla", "Eacute", "Ecircumflex", "Edieresis", "Egrave", "Iacute", "Icircumflex",
    "Idieresis", "Igrave", "Ntilde", "Oacute", "Ocircumflex", "Odieresis", "Ograve", "Otilde", "Scaron", "Uacute", "Ucircumflex",
    "Udieresis", "Ugrave", "Yacute", "Ydieresis", "Zcaron", "aacute", "acircumflex", "adieresis", "agrave", "aring", "atilde", "ccedilla",
    "eacute", "ecircumflex", "edieresis", "egrave", "iacute", "icircumflex", "idieresis", "igrave", "ntilde", "oacute", "ocircumflex",
    "odieresis", "ograve", "otilde", "scaron", "uacute", "ucircumflex", "udieresis", "ugrave", "yacute", "ydieresis", "zcaron",
];

/// The CFF standard string `sid` (ISOAdobe range only; others are None).
pub fn cff_standard_string(sid: usize) -> Option<&'static str> {
    match sid {
        0 => Some(".notdef"),
        1..=95 => Some(ASCII_NAMES[sid - 1]),
        96..=228 => Some(CFF_EXTRA[sid - 96]),
        _ => None,
    }
}

/// Glyph name to Unicode for the Latin glyphs the encodings and fonts use.
const GLYPHS: &[(&str, u32)] = &[
    ("space", 0x20), ("exclam", 0x21), ("quotedbl", 0x22), ("numbersign", 0x23), ("dollar", 0x24), ("percent", 0x25),
    ("ampersand", 0x26), ("quotesingle", 0x27), ("parenleft", 0x28), ("parenright", 0x29), ("asterisk", 0x2A), ("plus", 0x2B),
    ("comma", 0x2C), ("hyphen", 0x2D), ("period", 0x2E), ("slash", 0x2F), ("zero", 0x30), ("one", 0x31), ("two", 0x32), ("three", 0x33),
    ("four", 0x34), ("five", 0x35), ("six", 0x36), ("seven", 0x37), ("eight", 0x38), ("nine", 0x39), ("colon", 0x3A), ("semicolon", 0x3B),
    ("less", 0x3C), ("equal", 0x3D), ("greater", 0x3E), ("question", 0x3F), ("at", 0x40), ("bracketleft", 0x5B), ("backslash", 0x5C),
    ("bracketright", 0x5D), ("asciicircum", 0x5E), ("underscore", 0x5F), ("grave", 0x60), ("braceleft", 0x7B), ("bar", 0x7C),
    ("braceright", 0x7D), ("asciitilde", 0x7E), ("quoteright", 0x2019), ("quoteleft", 0x2018), ("exclamdown", 0xA1), ("cent", 0xA2),
    ("sterling", 0xA3), ("fraction", 0x2044), ("yen", 0xA5), ("florin", 0x192), ("section", 0xA7), ("currency", 0xA4),
    ("quotedblleft", 0x201C), ("guillemotleft", 0xAB), ("guilsinglleft", 0x2039), ("guilsinglright", 0x203A), ("fi", 0xFB01),
    ("fl", 0xFB02), ("ff", 0xFB00), ("ffi", 0xFB03), ("ffl", 0xFB04), ("endash", 0x2013), ("dagger", 0x2020), ("daggerdbl", 0x2021),
    ("periodcentered", 0xB7), ("paragraph", 0xB6), ("bullet", 0x2022), ("quotesinglbase", 0x201A), ("quotedblbase", 0x201E),
    ("quotedblright", 0x201D), ("guillemotright", 0xBB), ("ellipsis", 0x2026), ("perthousand", 0x2030), ("questiondown", 0xBF),
    ("acute", 0xB4), ("circumflex", 0x2C6), ("tilde", 0x2DC), ("macron", 0xAF), ("breve", 0x2D8), ("dotaccent", 0x2D9),
    ("dieresis", 0xA8), ("ring", 0x2DA), ("cedilla", 0xB8), ("hungarumlaut", 0x2DD), ("ogonek", 0x2DB), ("caron", 0x2C7),
    ("emdash", 0x2014), ("AE", 0xC6), ("ordfeminine", 0xAA), ("Lslash", 0x141), ("Oslash", 0xD8), ("OE", 0x152), ("ordmasculine", 0xBA),
    ("ae", 0xE6), ("dotlessi", 0x131), ("lslash", 0x142), ("oslash", 0xF8), ("oe", 0x153), ("germandbls", 0xDF), ("onesuperior", 0xB9),
    ("logicalnot", 0xAC), ("mu", 0xB5), ("trademark", 0x2122), ("Eth", 0xD0), ("onehalf", 0xBD), ("plusminus", 0xB1), ("Thorn", 0xDE),
    ("onequarter", 0xBC), ("divide", 0xF7), ("brokenbar", 0xA6), ("degree", 0xB0), ("thorn", 0xFE), ("threequarters", 0xBE),
    ("twosuperior", 0xB2), ("registered", 0xAE), ("minus", 0x2212), ("eth", 0xF0), ("multiply", 0xD7), ("threesuperior", 0xB3),
    ("copyright", 0xA9), ("Aacute", 0xC1), ("Acircumflex", 0xC2), ("Adieresis", 0xC4), ("Agrave", 0xC0), ("Aring", 0xC5),
    ("Atilde", 0xC3), ("Ccedilla", 0xC7), ("Eacute", 0xC9), ("Ecircumflex", 0xCA), ("Edieresis", 0xCB), ("Egrave", 0xC8),
    ("Iacute", 0xCD), ("Icircumflex", 0xCE), ("Idieresis", 0xCF), ("Igrave", 0xCC), ("Ntilde", 0xD1), ("Oacute", 0xD3),
    ("Ocircumflex", 0xD4), ("Odieresis", 0xD6), ("Ograve", 0xD2), ("Otilde", 0xD5), ("Scaron", 0x160), ("Uacute", 0xDA),
    ("Ucircumflex", 0xDB), ("Udieresis", 0xDC), ("Ugrave", 0xD9), ("Yacute", 0xDD), ("Ydieresis", 0x178), ("Zcaron", 0x17D),
    ("aacute", 0xE1), ("acircumflex", 0xE2), ("adieresis", 0xE4), ("agrave", 0xE0), ("aring", 0xE5), ("atilde", 0xE3),
    ("ccedilla", 0xE7), ("eacute", 0xE9), ("ecircumflex", 0xEA), ("edieresis", 0xEB), ("egrave", 0xE8), ("iacute", 0xED),
    ("icircumflex", 0xEE), ("idieresis", 0xEF), ("igrave", 0xEC), ("ntilde", 0xF1), ("oacute", 0xF3), ("ocircumflex", 0xF4),
    ("odieresis", 0xF6), ("ograve", 0xF2), ("otilde", 0xF5), ("scaron", 0x161), ("uacute", 0xFA), ("ucircumflex", 0xFB),
    ("udieresis", 0xFC), ("ugrave", 0xF9), ("yacute", 0xFD), ("ydieresis", 0xFF), ("zcaron", 0x17E), ("Euro", 0x20AC),
    ("nbspace", 0xA0), ("sfthyphen", 0xAD), ("notequal", 0x2260), ("infinity", 0x221E), ("lessequal", 0x2264),
    ("greaterequal", 0x2265), ("partialdiff", 0x2202), ("summation", 0x2211), ("product", 0x220F), ("pi", 0x3C0),
    ("integral", 0x222B), ("Omega", 0x2126), ("radical", 0x221A), ("approxequal", 0x2248), ("Delta", 0x2206), ("lozenge", 0x25CA),
    ("apple", 0xF8FF), ("Abreve", 0x102), ("abreve", 0x103), ("Aogonek", 0x104), ("aogonek", 0x105), ("Cacute", 0x106),
    ("cacute", 0x107), ("Ccaron", 0x10C), ("ccaron", 0x10D), ("Dcaron", 0x10E), ("dcaron", 0x10F), ("Dcroat", 0x110),
    ("dcroat", 0x111), ("Eogonek", 0x118), ("eogonek", 0x119), ("Ecaron", 0x11A), ("ecaron", 0x11B), ("Gbreve", 0x11E),
    ("gbreve", 0x11F), ("Idotaccent", 0x130), ("Lacute", 0x139), ("lacute", 0x13A), ("Lcaron", 0x13D), ("lcaron", 0x13E),
    ("Nacute", 0x143), ("nacute", 0x144), ("Ncaron", 0x147), ("ncaron", 0x148), ("Ohungarumlaut", 0x150), ("ohungarumlaut", 0x151),
    ("Racute", 0x154), ("racute", 0x155), ("Rcaron", 0x158), ("rcaron", 0x159), ("Sacute", 0x15A), ("sacute", 0x15B),
    ("Scedilla", 0x15E), ("scedilla", 0x15F), ("Tcaron", 0x164), ("tcaron", 0x165), ("Uring", 0x16E), ("uring", 0x16F),
    ("Uhungarumlaut", 0x170), ("uhungarumlaut", 0x171), ("Zacute", 0x179), ("zacute", 0x17A), ("Zdotaccent", 0x17B),
    ("zdotaccent", 0x17C), ("Gamma", 0x393), ("Theta", 0x398), ("Phi", 0x3A6), ("alpha", 0x3B1), ("beta", 0x3B2), ("gamma", 0x3B3),
    ("delta", 0x3B4), ("epsilon", 0x3B5), ("sigma", 0x3C3), ("tau", 0x3C4), ("phi", 0x3C6), ("arrowleft", 0x2190),
    ("arrowup", 0x2191), ("arrowright", 0x2192), ("arrowdown", 0x2193), ("arrowboth", 0x2194), ("checkmark", 0x2713),
    ("uni00A0", 0xA0), (".notdef", 0),
];

/// A glyph name's Unicode text: the table above, `uniXXXX[XXXX...]`, `uXXXX[XX]`, `f_f_i` ligatures,
/// and `name.suffix` variants.
pub fn glyph_to_unicode(name: &str) -> Option<String> {
    if name.is_empty() || name == ".notdef" {
        return None;
    }
    if let Some(&(_, u)) = GLYPHS.iter().find(|(n, _)| *n == name) {
        return char::from_u32(u).map(|c| c.to_string());
    }
    if name.len() == 1 {
        let c = name.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(c.to_string());
        }
    }
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() >= 4 && hex.len() % 4 == 0 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            let units: Vec<u16> = (0..hex.len() / 4).filter_map(|i| u16::from_str_radix(&hex[i * 4..i * 4 + 4], 16).ok()).collect();
            let s: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
            return Some(s);
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(|c| c.to_string());
        }
    }
    if let Some(dot) = name.find('.') {
        if dot > 0 {
            return glyph_to_unicode(&name[..dot]);
        }
    }
    if name.contains('_') {
        let parts: Option<Vec<String>> = name.split('_').map(glyph_to_unicode).collect();
        return parts.map(|p| p.concat());
    }
    // "a123"-style names of some generators: the code as a character.
    None
}

/// A base encoding: 256 glyph names (None for unused codes).
pub type Encoding = [Option<&'static str>; 256];

fn ascii_base(standard: bool) -> Encoding {
    let mut e: Encoding = [None; 256];
    for (i, n) in ASCII_NAMES.iter().enumerate() {
        e[32 + i] = Some(n);
    }
    if !standard {
        e[39] = Some("quotesingle");
        e[96] = Some("grave");
    }
    e
}

pub fn standard_encoding() -> Encoding {
    let mut e = ascii_base(true);
    let hi: &[(usize, &str)] = &[
        (161, "exclamdown"), (162, "cent"), (163, "sterling"), (164, "fraction"), (165, "yen"), (166, "florin"), (167, "section"),
        (168, "currency"), (169, "quotesingle"), (170, "quotedblleft"), (171, "guillemotleft"), (172, "guilsinglleft"),
        (173, "guilsinglright"), (174, "fi"), (175, "fl"), (177, "endash"), (178, "dagger"), (179, "daggerdbl"), (180, "periodcentered"),
        (182, "paragraph"), (183, "bullet"), (184, "quotesinglbase"), (185, "quotedblbase"), (186, "quotedblright"),
        (187, "guillemotright"), (188, "ellipsis"), (189, "perthousand"), (191, "questiondown"), (193, "grave"), (194, "acute"),
        (195, "circumflex"), (196, "tilde"), (197, "macron"), (198, "breve"), (199, "dotaccent"), (200, "dieresis"), (202, "ring"),
        (203, "cedilla"), (205, "hungarumlaut"), (206, "ogonek"), (207, "caron"), (208, "emdash"), (225, "AE"), (227, "ordfeminine"),
        (232, "Lslash"), (233, "Oslash"), (234, "OE"), (235, "ordmasculine"), (241, "ae"), (245, "dotlessi"), (248, "lslash"),
        (249, "oslash"), (250, "oe"), (251, "germandbls"),
    ];
    for &(c, n) in hi {
        e[c] = Some(n);
    }
    e
}

const WIN_80: [&str; 32] = [
    "Euro", "", "quotesinglbase", "florin", "quotedblbase", "ellipsis", "dagger", "daggerdbl", "circumflex", "perthousand", "Scaron",
    "guilsinglleft", "OE", "", "Zcaron", "", "", "quoteleft", "quoteright", "quotedblleft", "quotedblright", "bullet", "endash", "emdash",
    "tilde", "trademark", "scaron", "guilsinglright", "oe", "", "zcaron", "Ydieresis",
];

const LATIN1_A0: [&str; 96] = [
    "space", "exclamdown", "cent", "sterling", "currency", "yen", "brokenbar", "section", "dieresis", "copyright", "ordfeminine",
    "guillemotleft", "logicalnot", "hyphen", "registered", "macron", "degree", "plusminus", "twosuperior", "threesuperior", "acute", "mu",
    "paragraph", "periodcentered", "cedilla", "onesuperior", "ordmasculine", "guillemotright", "onequarter", "onehalf", "threequarters",
    "questiondown", "Agrave", "Aacute", "Acircumflex", "Atilde", "Adieresis", "Aring", "AE", "Ccedilla", "Egrave", "Eacute", "Ecircumflex",
    "Edieresis", "Igrave", "Iacute", "Icircumflex", "Idieresis", "Eth", "Ntilde", "Ograve", "Oacute", "Ocircumflex", "Otilde", "Odieresis",
    "multiply", "Oslash", "Ugrave", "Uacute", "Ucircumflex", "Udieresis", "Yacute", "Thorn", "germandbls", "agrave", "aacute",
    "acircumflex", "atilde", "adieresis", "aring", "ae", "ccedilla", "egrave", "eacute", "ecircumflex", "edieresis", "igrave", "iacute",
    "icircumflex", "idieresis", "eth", "ntilde", "ograve", "oacute", "ocircumflex", "otilde", "odieresis", "divide", "oslash", "ugrave",
    "uacute", "ucircumflex", "udieresis", "yacute", "thorn", "ydieresis",
];

pub fn win_ansi_encoding() -> Encoding {
    let mut e = ascii_base(false);
    for (i, n) in WIN_80.iter().enumerate() {
        if !n.is_empty() {
            e[0x80 + i] = Some(n);
        }
    }
    for (i, n) in LATIN1_A0.iter().enumerate() {
        e[0xA0 + i] = Some(n);
    }
    // WinAnsi also shows bullets for the unused codes; the spec maps 0xAD to hyphen.
    e
}

const MAC_80: [&str; 128] = [
    "Adieresis", "Aring", "Ccedilla", "Eacute", "Ntilde", "Odieresis", "Udieresis", "aacute", "agrave", "acircumflex", "adieresis",
    "atilde", "aring", "ccedilla", "eacute", "egrave", "ecircumflex", "edieresis", "iacute", "igrave", "icircumflex", "idieresis", "ntilde",
    "oacute", "ograve", "ocircumflex", "odieresis", "otilde", "uacute", "ugrave", "ucircumflex", "udieresis", "dagger", "degree", "cent",
    "sterling", "section", "bullet", "paragraph", "germandbls", "registered", "copyright", "trademark", "acute", "dieresis", "notequal",
    "AE", "Oslash", "infinity", "plusminus", "lessequal", "greaterequal", "yen", "mu", "partialdiff", "summation", "product", "pi",
    "integral", "ordfeminine", "ordmasculine", "Omega", "ae", "oslash", "questiondown", "exclamdown", "logicalnot", "radical", "florin",
    "approxequal", "Delta", "guillemotleft", "guillemotright", "ellipsis", "space", "Agrave", "Atilde", "Otilde", "OE", "oe", "endash",
    "emdash", "quotedblleft", "quotedblright", "quoteleft", "quoteright", "divide", "lozenge", "ydieresis", "Ydieresis", "fraction",
    "currency", "guilsinglleft", "guilsinglright", "fi", "fl", "daggerdbl", "periodcentered", "quotesinglbase", "quotedblbase",
    "perthousand", "Acircumflex", "Ecircumflex", "Aacute", "Edieresis", "Egrave", "Iacute", "Icircumflex", "Idieresis", "Igrave", "Oacute",
    "Ocircumflex", "apple", "Ograve", "Uacute", "Ucircumflex", "Ugrave", "dotlessi", "circumflex", "tilde", "macron", "breve", "dotaccent",
    "ring", "cedilla", "hungarumlaut", "ogonek", "caron",
];

pub fn mac_roman_encoding() -> Encoding {
    let mut e = ascii_base(false);
    e[39] = Some("quotesingle");
    for (i, n) in MAC_80.iter().enumerate() {
        e[0x80 + i] = Some(n);
    }
    e
}

/// A base encoding by its PDF name.
pub fn named_encoding(name: &str) -> Option<Encoding> {
    match name {
        "WinAnsiEncoding" => Some(win_ansi_encoding()),
        "MacRomanEncoding" => Some(mac_roman_encoding()),
        "StandardEncoding" => Some(standard_encoding()),
        "PDFDocEncoding" => Some(win_ansi_encoding()),
        _ => None,
    }
}

/// For the standard-14 width tables: a glyph name's position among ASCII_NAMES.
pub fn ascii_index(name: &str) -> Option<usize> {
    ASCII_NAMES.iter().position(|n| *n == name)
}
