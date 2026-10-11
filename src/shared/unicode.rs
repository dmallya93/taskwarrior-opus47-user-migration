//! Rust translation of `libshared src/unicode.cpp` (everything declared in
//! `unicode.h`): code point classification helpers.
//!
//! `unicode_punctuation` and `unicode_alpha` call the C library's `iswpunct`
//! and `iswprint` directly, exactly as the C++ does, so their answers depend
//! on the process `LC_CTYPE` locale in the same way. Neither Taskwarrior nor
//! this translation calls `setlocale`, so both run in the "C" locale.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

// `<cwctype>` functions. The `libc` crate does not bind them; `wint_t` is a
// 32-bit integer on every supported platform.
extern "C" {
    fn iswpunct(wc: libc::c_uint) -> libc::c_int;
    fn iswprint(wc: libc::c_uint) -> libc::c_int;
}

/// C++ `iswpunct (c) != 0`.
fn isw_punct(c: u32) -> bool {
    // SAFETY: iswpunct is a pure classification function, defined for any
    // wint_t value.
    unsafe { iswpunct(c) != 0 }
}

/// C++ `iswprint (c) != 0`.
fn isw_print(c: u32) -> bool {
    // SAFETY: iswprint is a pure classification function, defined for any
    // wint_t value.
    unsafe { iswprint(c) != 0 }
}

/// C++ `unicodeWhitespace`: complete Unicode whitespace list (horizontal or
/// vertical).
pub fn unicode_whitespace(c: u32) -> bool {
    unicode_horizontal_whitespace(c) || unicode_vertical_whitespace(c)
}

/// C++ `unicodeHorizontalWhitespace`: complete Unicode horizontal whitespace
/// list.
///
/// <http://en.wikipedia.org/wiki/Whitespace_character>, updated 2015-09-13.
pub fn unicode_horizontal_whitespace(c: u32) -> bool {
    c == 0x0020 ||   // space Common  Separator, space
    c == 0x0009 ||   // Common  Other, control  HT, Horizontal Tab
    c == 0x00A0 ||   // no-break space  Common  Separator, space
    c == 0x1680 ||   // ogham space mark  Ogham Separator, space
    c == 0x180E ||   // mongolian vowel separator Mongolian Separator, space
    c == 0x2000 ||   // en quad Common  Separator, space
    c == 0x2001 ||   // em quad Common  Separator, space
    c == 0x2002 ||   // en space  Common  Separator, space
    c == 0x2003 ||   // em space  Common  Separator, space
    c == 0x2004 ||   // three-per-em space  Common  Separator, space
    c == 0x2005 ||   // four-per-em space Common  Separator, space
    c == 0x2006 ||   // six-per-em space  Common  Separator, space
    c == 0x2007 ||   // figure space  Common  Separator, space
    c == 0x2008 ||   // punctuation space Common  Separator, space
    c == 0x2009 ||   // thin space  Common  Separator, space
    c == 0x200A ||   // hair space  Common  Separator, space
    c == 0x200B ||   // zero width space
    c == 0x200C ||   // zero width non-joiner
    c == 0x200D ||   // zero width joiner
    c == 0x202F ||   // narrow no-break space Common  Separator, space
    c == 0x205F ||   // medium mathematical space Common  Separator, space
    c == 0x2060 ||   // word joiner
    c == 0x3000 // ideographic space Common  Separator, space
}

/// C++ `unicodeVerticalWhitespace`: complete Unicode vertical whitespace list.
///
/// <http://en.wikipedia.org/wiki/Whitespace_character>, updated 2015-09-13.
pub fn unicode_vertical_whitespace(c: u32) -> bool {
    c == 0x000A ||   // Common  Other, control  LF, Line feed
    c == 0x000B ||   // Common  Other, control  VT, Vertical Tab
    c == 0x000C ||   // Common  Other, control  FF, Form feed
    c == 0x000D ||   // Common  Other, control  CR, Carriage return
    c == 0x0085 ||   // Common  Other, control  NEL, Next line
    c == 0x2028 ||   // line separator  Common  Separator, line
    c == 0x2029 // paragraph separator Common  Separator, paragraph
}

/// C++ `unicodePunctuation`: `iswpunct (c) != 0` in the current locale.
pub fn unicode_punctuation(c: u32) -> bool {
    isw_punct(c)
}

/// C++ `unicodeAlpha`: printable, not punctuation and not whitespace, in the
/// current locale.
pub fn unicode_alpha(c: u32) -> bool {
    isw_print(c) && !isw_punct(c) && !unicode_whitespace(c)
}

/// C++ `unicodeLatinAlpha`: `A`-`Z` or `a`-`z`.
pub fn unicode_latin_alpha(c: u32) -> bool {
    ('A' as u32..='Z' as u32).contains(&c) || ('a' as u32..='z' as u32).contains(&c)
}

/// C++ `unicodeLatinDigit`: digits 0-9.
pub fn unicode_latin_digit(c: u32) -> bool {
    (0x30..=0x39).contains(&c)
}

/// C++ `unicodeHexDigit`: digits 0-9, a-f, A-F.
pub fn unicode_hex_digit(c: u32) -> bool {
    ('0' as u32..='9' as u32).contains(&c)
        || ('a' as u32..='f' as u32).contains(&c)
        || ('A' as u32..='F' as u32).contains(&c)
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/unicode.t.cpp`. Each C++ `t.ok`/`t.notok` is
    //! one assertion here, carrying the C++ test description as its message.

    use super::*;

    #[test]
    fn whitespace() {
        // White space detection.
        assert!(
            !unicode_whitespace(0x0041),
            "U+0041 (A) ! unicodeWhitespace"
        );
        assert!(unicode_whitespace(0x0020), "U+0020 unicodeWhitespace");
        assert!(unicode_whitespace(0x0009), "U+0009 unicodeWhitespace");
        assert!(unicode_whitespace(0x000A), "U+000A unicodeWhitespace");
        assert!(unicode_whitespace(0x000B), "U+000B unicodeWhitespace");
        assert!(unicode_whitespace(0x000C), "U+000C unicodeWhitespace");
        assert!(unicode_whitespace(0x000D), "U+000D unicodeWhitespace");
        assert!(unicode_whitespace(0x0085), "U+0085 unicodeWhitespace");
        assert!(unicode_whitespace(0x00A0), "U+00A0 unicodeWhitespace");
        assert!(unicode_whitespace(0x1680), "U+1680 unicodeWhitespace");
        assert!(unicode_whitespace(0x180E), "U+180E unicodeWhitespace");
        assert!(unicode_whitespace(0x2000), "U+2000 unicodeWhitespace");
        assert!(unicode_whitespace(0x2001), "U+2001 unicodeWhitespace");
        assert!(unicode_whitespace(0x2002), "U+2002 unicodeWhitespace");
        assert!(unicode_whitespace(0x2003), "U+2003 unicodeWhitespace");
        assert!(unicode_whitespace(0x2004), "U+2004 unicodeWhitespace");
        assert!(unicode_whitespace(0x2005), "U+2005 unicodeWhitespace");
        assert!(unicode_whitespace(0x2006), "U+2006 unicodeWhitespace");
        assert!(unicode_whitespace(0x2007), "U+2007 unicodeWhitespace");
        assert!(unicode_whitespace(0x2008), "U+2008 unicodeWhitespace");
        assert!(unicode_whitespace(0x2009), "U+2009 unicodeWhitespace");
        assert!(unicode_whitespace(0x200A), "U+200A unicodeWhitespace");
        assert!(unicode_whitespace(0x2028), "U+2028 unicodeWhitespace");
        assert!(unicode_whitespace(0x2029), "U+2029 unicodeWhitespace");
        assert!(unicode_whitespace(0x202F), "U+202F unicodeWhitespace");
        assert!(unicode_whitespace(0x205F), "U+205F unicodeWhitespace");
        assert!(unicode_whitespace(0x3000), "U+3000 unicodeWhitespace");
    }

    #[test]
    fn horizontal_whitespace() {
        // Horizontal white space detection.
        assert!(
            !unicode_horizontal_whitespace(0x0041),
            "U+0041 (A) ! unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x0020),
            "U+0020 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x0009),
            "U+0009 unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x000A),
            "U+000A ! unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x000B),
            "U+000B ! unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x000C),
            "U+000C ! unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x000D),
            "U+000D ! unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x0085),
            "U+0085 ! unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x00A0),
            "U+00A0 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x1680),
            "U+1680 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x180E),
            "U+180E unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2000),
            "U+2000 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2001),
            "U+2001 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2002),
            "U+2002 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2003),
            "U+2003 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2004),
            "U+2004 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2005),
            "U+2005 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2006),
            "U+2006 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2007),
            "U+2007 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2008),
            "U+2008 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x2009),
            "U+2009 unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x200A),
            "U+200A unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x2028),
            "U+2028 ! unicodeHorizontalWhitespace"
        );
        assert!(
            !unicode_horizontal_whitespace(0x2029),
            "U+2029 ! unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x202F),
            "U+202F unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x205F),
            "U+205F unicodeHorizontalWhitespace"
        );
        assert!(
            unicode_horizontal_whitespace(0x3000),
            "U+3000 unicodeHorizontalWhitespace"
        );
    }

    #[test]
    fn vertical_whitespace() {
        // Vertical white space detection.
        assert!(
            !unicode_vertical_whitespace(0x0041),
            "U+0041 (A) ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x0020),
            "U+0020 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x0009),
            "U+0009 ! unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x000A),
            "U+000A unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x000B),
            "U+000B unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x000C),
            "U+000C unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x000D),
            "U+000D unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x0085),
            "U+0085 unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x00A0),
            "U+00A0 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x1680),
            "U+1680 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x180E),
            "U+180E ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2000),
            "U+2000 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2001),
            "U+2001 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2002),
            "U+2002 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2003),
            "U+2003 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2004),
            "U+2004 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2005),
            "U+2005 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2006),
            "U+2006 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2007),
            "U+2007 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2008),
            "U+2008 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x2009),
            "U+2009 ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x200A),
            "U+200A ! unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x2028),
            "U+2028 unicodeVerticalWhitespace"
        );
        assert!(
            unicode_vertical_whitespace(0x2029),
            "U+2029 unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x202F),
            "U+202F ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x205F),
            "U+205F ! unicodeVerticalWhitespace"
        );
        assert!(
            !unicode_vertical_whitespace(0x3000),
            "U+3000 ! unicodeVerticalWhitespace"
        );
    }

    #[test]
    fn latin_alpha() {
        // Latin Alpha
        assert!(
            !unicode_latin_alpha(0x0033),
            "U+0033 (3) ! unicodeLatinAlpha"
        );
        assert!(unicode_latin_alpha(0x0041), "U+0041 (A) unicodeLatinAlpha");
        assert!(unicode_latin_alpha(0x005A), "U+005A (Z) unicodeLatinAlpha");
        assert!(unicode_latin_alpha(0x0061), "U+0061 (a) unicodeLatinAlpha");
        assert!(unicode_latin_alpha(0x007A), "U+007A (z) unicodeLatinAlpha");
    }

    #[test]
    fn latin_digit() {
        // Latin Digit
        assert!(
            !unicode_latin_digit(0x0041),
            "U+0041 (A) ! unicodeLatinDigit"
        );
        assert!(unicode_latin_digit(0x0030), "U+0030 (0) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0031), "U+0031 (1) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0032), "U+0032 (2) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0033), "U+0033 (3) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0034), "U+0034 (4) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0035), "U+0035 (5) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0036), "U+0036 (6) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0037), "U+0037 (7) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0038), "U+0038 (8) unicodeLatinDigit");
        assert!(unicode_latin_digit(0x0039), "U+0039 (9) unicodeLatinDigit");
    }

    #[test]
    fn hex_digit() {
        // Hex Digit
        assert!(!unicode_hex_digit(0x0047), "U+0047 (G) ! unicodeHexDigit");
        assert!(unicode_hex_digit(0x0030), "U+0030 (0) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0031), "U+0031 (1) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0032), "U+0032 (2) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0033), "U+0033 (3) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0034), "U+0034 (4) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0035), "U+0035 (5) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0036), "U+0036 (6) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0037), "U+0037 (7) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0038), "U+0038 (8) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0039), "U+0039 (9) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0041), "U+0041 (A) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0042), "U+0042 (B) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0043), "U+0043 (C) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0044), "U+0044 (D) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0045), "U+0045 (E) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0046), "U+0046 (F) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0061), "U+0061 (a) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0062), "U+0062 (b) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0063), "U+0063 (c) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0064), "U+0064 (d) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0065), "U+0065 (e) unicodeHexDigit");
        assert!(unicode_hex_digit(0x0066), "U+0066 (f) unicodeHexDigit");
    }

    #[test]
    fn punctuation() {
        // Punctuation
        assert!(
            !unicode_punctuation(0x0033),
            "U+0033 (3) ! unicodePunctuation"
        );
        assert!(unicode_punctuation(0x002C), "U+002C (,) unicodePunctuation");
        assert!(unicode_punctuation(0x002E), "U+002E (.) unicodePunctuation");
        assert!(unicode_punctuation(0x003A), "U+003A (:) unicodePunctuation");
        assert!(unicode_punctuation(0x0021), "U+0021 (!) unicodePunctuation");
        assert!(unicode_punctuation(0x003F), "U+003F (?) unicodePunctuation");
    }

    #[test]
    fn alpha() {
        // Alpha
        //
        // The C++ test first calls `setlocale (LC_ALL, "en_US.utf8")`. That is
        // process-global (it would leak into concurrently running Rust tests)
        // and the locale is not necessarily installed, so these assertions run
        // in the "C" locale Taskwarrior itself uses; both hold there too.
        assert!(unicode_alpha(0x0041), "U+0041 (A) unicodeAlpha");
        assert!(!unicode_alpha(0x3004), "U+3004 (〄) ! unicodeAlpha");
    }

    #[test]
    fn alpha_ascii_in_c_locale() {
        assert!(unicode_alpha('z' as u32));
        assert!(unicode_alpha('5' as u32));
        assert!(!unicode_alpha(' ' as u32));
        assert!(!unicode_alpha('.' as u32));
        assert!(!unicode_alpha('\n' as u32));
    }

    #[test]
    fn whitespace_zero_width_and_word_joiner() {
        // Listed in the C++ table but not exercised by unicode.t.cpp.
        for c in [0x200B, 0x200C, 0x200D, 0x2060] {
            assert!(unicode_horizontal_whitespace(c));
            assert!(unicode_whitespace(c));
            assert!(!unicode_vertical_whitespace(c));
        }
    }
}
