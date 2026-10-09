//! Rust translation of `libshared src/format.cpp` (everything declared in
//! `format.h`): positional `{N}` formatting, justification, number and size
//! formatting, and text sanitizing.
//!
//! The C++ `format` overload set maps onto Rust as follows:
//!
//! | C++                                             | Rust                         |
//! |-------------------------------------------------|------------------------------|
//! | `format (T)`, `format (std::string&)`, `format (const char*)`, `format (double)` | [`format_value`] / [`FormatArg`] |
//! | `format (fmt, args...)` (variadic template)     | [`format`]`(fmt, &[&a, &b])` |
//! | `format (float, int, int)`                      | [`format_float`]             |
//! | `format (double, int, int)`                     | [`format_double`]            |
//! | `leftJustify (int, int)` / `(string, int)`      | [`left_justify_int`] / [`left_justify`] |
//! | `rightJustify (int, int)` / `(string, int)`     | [`right_justify_int`] / [`right_justify`] |
//! | `printable (std::string)` / `printable (char)`  | [`printable`] / [`printable_char`] |
//!
//! `std::ostream` renders floating point through the C library's `printf`
//! conversions, which differ from Rust's `Display`. [`printf_e`],
//! [`printf_f`], [`printf_g`] and [`stream_double`] reproduce those
//! conversions byte for byte (glibc spelling of infinities and NaNs).
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

use crate::shared::utf8::{utf8_next_char, utf8_text_width};

////////////////////////////////////////////////////////////////////////////////
// C printf / iostream floating point conversions.

/// glibc spelling of non-finite values for `%e`/`%f`/`%g`, or `None` for a
/// finite value.
fn non_finite(value: f64) -> Option<String> {
    if value.is_nan() {
        Some(if value.is_sign_negative() { "-nan" } else { "nan" }.to_string())
    } else if value.is_infinite() {
        Some(if value < 0.0 { "-inf" } else { "inf" }.to_string())
    } else {
        None
    }
}

/// Splits Rust's `{:.*e}` rendering (`"-1.2345e-7"`) into its mantissa
/// (`"-1.2345"`) and decimal exponent (`-7`).
fn rust_exponential(value: f64, precision: usize) -> (String, i32) {
    let rendered = format!("{value:.precision$e}");
    match rendered.split_once('e') {
        Some((mantissa, exponent)) => (mantissa.to_string(), exponent.parse().unwrap_or(0)),
        None => (rendered, 0),
    }
}

/// C `printf ("%.*e", precision, value)`: one digit before the point,
/// `precision` digits after, and an exponent of at least two digits
/// (`1.234500e+03`). Rounding is exact on the binary value, ties to even,
/// as in glibc.
pub fn printf_e(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }

    let (mantissa, exponent) = rust_exponential(value, precision);
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
}

/// C `printf ("%.*f", precision, value)`: fixed notation with `precision`
/// digits after the point. Rounding is exact on the binary value, ties to
/// even, as in glibc.
pub fn printf_f(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }

    format!("{value:.precision$}")
}

/// C `printf ("%.*g", precision, value)`: `precision` significant digits
/// (0 is treated as 1), using `%e` style when the exponent is less than -4 or
/// not less than the precision and `%f` style otherwise, then removing
/// trailing zeros and a trailing decimal point.
pub fn printf_g(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text;
    }

    let p = precision.max(1);

    // The exponent X that %e would produce at precision P - 1 (after rounding).
    let (_, x) = rust_exponential(value, p - 1);

    if x < -4 || x >= p as i32 {
        let (mantissa, exponent) = rust_exponential(value, p - 1);
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", strip_fraction_zeros(&mantissa), exponent.unsigned_abs())
    } else {
        let decimals = (p as i32 - 1 - x) as usize;
        strip_fraction_zeros(&format!("{value:.decimals$}"))
    }
}

/// `%g` post-processing: removes trailing zeros from the fractional part, and
/// the decimal point itself if no fraction remains.
fn strip_fraction_zeros(number: &str) -> String {
    if !number.contains('.') {
        return number.to_string();
    }

    number.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `std::ostream << double` in the default float field: `%.*g` at the stream
/// precision. A negative precision means the default of 6 (libstdc++).
fn stream_general(value: f64, precision: i32) -> String {
    let precision = if precision < 0 { 6 } else { precision as usize };
    printf_g(value, precision)
}

/// `std::stringstream s; s << value;` for a `double` (or a `float`, which the
/// stream widens to `double`) with default flags: `%g` at precision 6.
pub fn stream_double(value: f64) -> String {
    stream_general(value, 6)
}

/// `std::setw (width)` with the default right adjustment: pads `text` on the
/// left with `fill` to `width` characters. Negative or small widths add
/// nothing.
fn pad_left(text: &str, width: i32, fill: char) -> String {
    let length = text.chars().count();
    let width = width.max(0) as usize;
    if width <= length {
        return text.to_string();
    }

    let mut padded: String = std::iter::repeat_n(fill, width - length).collect();
    padded.push_str(text);
    padded
}

/// `count` spaces, as C++ `std::string (std::max<int> (count, 0), ' ')`.
fn spaces(count: i32) -> String {
    " ".repeat(count.max(0) as usize)
}

////////////////////////////////////////////////////////////////////////////////
// format

/// A value that can be substituted into a [`format`] pattern: what the C++
/// `format (value)` overload set produces for the value's type.
///
/// - strings and `char` are copied as-is (`format (std::string&)`,
///   `format (const char*)`, `stream << char`);
/// - integers are written in decimal (`stream << int`);
/// - `bool` is written as `1`/`0` (`stream << bool` without `boolalpha`);
/// - `f64` uses the non-template `format (double)`, i.e. `std::fixed` with
///   precision 6 (`1.500000`), because C++ overload resolution prefers it over
///   the template for `double` arguments;
/// - `f32` uses the template, i.e. the default stream conversion `%g` at
///   precision 6 (`1.5`).
///
/// Deliberately not implemented for `i8`/`u8`: C++ `char`, `signed char` and
/// `unsigned char` all stream as characters, while a Rust `u8` usually holds a
/// number. Pass a `char` or cast to a wider integer to say which is meant.
pub trait FormatArg {
    /// The C++ `format (value)` text for this value.
    fn format_arg(&self) -> String;
}

impl<T: FormatArg + ?Sized> FormatArg for &T {
    fn format_arg(&self) -> String {
        (**self).format_arg()
    }
}

impl FormatArg for str {
    fn format_arg(&self) -> String {
        self.to_string()
    }
}

impl FormatArg for String {
    fn format_arg(&self) -> String {
        self.clone()
    }
}

impl FormatArg for char {
    fn format_arg(&self) -> String {
        self.to_string()
    }
}

impl FormatArg for bool {
    fn format_arg(&self) -> String {
        if *self { "1" } else { "0" }.to_string()
    }
}

macro_rules! format_arg_decimal {
    ($($type:ty),*) => {
        $(
            impl FormatArg for $type {
                fn format_arg(&self) -> String {
                    self.to_string()
                }
            }
        )*
    };
}

format_arg_decimal!(i16, u16, i32, u32, i64, u64, i128, u128, isize, usize);

impl FormatArg for f64 {
    fn format_arg(&self) -> String {
        // C++ `format (double)`: s << std::fixed << value;
        printf_f(*self, 6)
    }
}

impl FormatArg for f32 {
    fn format_arg(&self) -> String {
        // C++ template `format (T)` with T = float: s << value;
        stream_double(*self as f64)
    }
}

/// C++ `format (T value)` and its non-template overloads `format
/// (std::string&)`, `format (const char*)` and `format (double)`: the text of
/// a single value (see [`FormatArg`] for the per-type rules).
pub fn format_value<T: FormatArg + ?Sized>(value: &T) -> String {
    value.format_arg()
}

/// C++ variadic `format (fmt, args...)`: replaces every `{1}` in `fmt` with
/// the first argument, then every `{2}` with the second, and so on.
///
/// As in C++, the substitutions happen one argument at a time over the
/// partially formatted text, so a `{2}` that appears inside the first
/// argument's text is itself replaced by the second argument. With no
/// arguments the pattern is returned unchanged (C++ `format (const char*)`).
///
/// ```
/// use taskwarrior::shared::format::format;
/// assert_eq!(format("pre {2}{1} post", &[&"one", &2]), "pre 2one post");
/// ```
pub fn format(fmt: &str, args: &[&dyn FormatArg]) -> String {
    let mut output = fmt.to_string();
    for (index, arg) in args.iter().enumerate() {
        let fmt_num = index as i32 + 1;
        replace_positional(&mut output, &format!("{{{fmt_num}}}"), &arg.format_arg());
    }

    output
}

/// C++ `formatHex (int)`: lower-case hexadecimal. Negative values are shown
/// as their 32-bit two's complement, as the C++ stream does.
pub fn format_hex(value: i32) -> String {
    format!("{value:x}")
}

/// C++ `format (float value, int width, int precision)`: `%g` at `precision`
/// significant digits, right-justified in `width`. Values strictly between 0
/// and 1 are first rounded (half away from zero) to `width - 2` decimal
/// places, the digits that fit after `0.`.
pub fn format_float(value: f32, width: i32, precision: i32) -> String {
    let mut value = value;
    if 0.0 < value && value < 1.0 {
        // For value close to zero, width - 2 (2 accounts for the first zero and
        // the dot) is the number of digits after zero that are significant
        let factor = near_zero_factor(width);
        // roundf (value * factor) / factor, with value * factor computed in
        // double, narrowed to float for roundf, and divided in double.
        value = (((value as f64 * factor) as f32).round() as f64 / factor) as f32;
    }

    pad_left(&stream_general(value as f64, precision), width, ' ')
}

/// C++ `format (double value, int width, int precision)`: `%g` at
/// `precision` significant digits, right-justified in `width`. Values
/// strictly between 0 and 1 are first rounded (half away from zero) to
/// `width - 2` decimal places, the digits that fit after `0.`.
pub fn format_double(value: f64, width: i32, precision: i32) -> String {
    let mut value = value;
    if 0.0 < value && value < 1.0 {
        // For value close to zero, width - 2 (2 accounts for the first zero and
        // the dot) is the number of digits after zero that are significant
        let factor = near_zero_factor(width);
        value = (value * factor).round() / factor;
    }

    pad_left(&stream_general(value, precision), width, ' ')
}

/// `10^(width - 2)` built by repeated multiplication, as in C++.
fn near_zero_factor(width: i32) -> f64 {
    let mut factor = 1.0;
    for _ in 2..width {
        factor *= 10.0;
    }
    factor
}

/// C++ `replace_positional`: replaces every occurrence of `from` in `fmt`
/// with `to`, scanning forward past each replacement (so text inserted by a
/// replacement is never rescanned).
///
/// Deviation: C++ loops forever for an empty `from`; here an empty `from`
/// leaves `fmt` unchanged.
pub fn replace_positional(fmt: &mut String, from: &str, to: &str) {
    if from.is_empty() {
        return;
    }

    let mut pos = 0;
    while let Some(found) = fmt[pos..].find(from) {
        let at = pos + found;
        fmt.replace_range(at..at + from.len(), to);
        pos = at + to.len();
    }
}

////////////////////////////////////////////////////////////////////////////////
// Justification

/// C++ `leftJustify (const int, const int)`: the decimal number padded on the
/// right with spaces to `width` characters.
pub fn left_justify_int(input: i32, width: i32) -> String {
    let output = input.to_string();
    let pad = spaces(width.wrapping_sub(output.len() as i32));
    output + &pad
}

/// C++ `leftJustify (const std::string&, const int)`: `input` padded on the
/// right with spaces to `width` display cells (color codes have no width).
pub fn left_justify(input: &str, width: i32) -> String {
    input.to_string() + &spaces(width.wrapping_sub(utf8_text_width(input) as i32))
}

/// C++ `rightJustifyZero (const int, const int)`: the decimal number padded on
/// the left with `0` to `width` characters. As with `std::setw` +
/// `std::setfill ('0')`, the fill goes before a minus sign (`-5`, 4 -> `00-5`).
pub fn right_justify_zero(input: i32, width: i32) -> String {
    pad_left(&input.to_string(), width, '0')
}

/// C++ `rightJustify (const int, const int)`: the decimal number padded on the
/// left with spaces to `width` characters.
pub fn right_justify_int(input: i32, width: i32) -> String {
    pad_left(&input.to_string(), width, ' ')
}

/// C++ `rightJustify (const std::string&, const int)`: `input` padded on the
/// left with spaces to `width` display cells (color codes have no width).
pub fn right_justify(input: &str, width: i32) -> String {
    spaces(width.wrapping_sub(utf8_text_width(input) as i32)) + input
}

////////////////////////////////////////////////////////////////////////////////
// Numbers, sizes and times

/// C++ `commify`: inserts thousands separators into the run of digits before
/// the (last) decimal point, or into the last run of digits when there is no
/// decimal point. Other text is preserved.
pub fn commify(data: &str) -> String {
    let data = data.as_bytes();
    let length = data.len() as isize;
    let at = |i: isize| data[i as usize];

    // First scan for decimal point and end of digits.
    let mut decimal_point: isize = -1;
    let mut end: isize = -1;
    for (i, &byte) in data.iter().enumerate() {
        if byte.is_ascii_digit() {
            end = i as isize;
        }

        if byte == b'.' {
            decimal_point = i as isize;
        }
    }

    let mut result: Vec<u8> = Vec::with_capacity(data.len() + data.len() / 3);
    let mut i = length - 1;
    if decimal_point != -1 {
        // In reverse order, transfer all digits up to, and including the decimal
        // point.
        while i >= decimal_point {
            result.push(at(i));
            i -= 1;
        }
    } else {
        // In reverse order, transfer all digits up to, but not including the last
        // digit.
        while i > end {
            result.push(at(i));
            i -= 1;
        }
    }

    let mut consecutive_digits = 0;
    while i >= 0 {
        let byte = at(i);
        result.push(byte);
        if byte.is_ascii_digit() {
            consecutive_digits += 1;
            if consecutive_digits == 3 && i != 0 && at(i - 1).is_ascii_digit() {
                result.push(b',');
                consecutive_digits = 0;
            }
        }
        i -= 1;
    }

    // reverse result into data.
    result.reverse();
    String::from_utf8_lossy(&result).into_owned()
}

/// C++ `formatBytes`: converts a quantity in bytes to a more readable format
/// (`994 B`, `1.0 KiB`, `1.0 MiB`, `1.0 GiB`), commified.
pub fn format_bytes(bytes: usize) -> String {
    let formatted = if bytes >= 995_000_000 {
        format!("{} GiB", printf_f(bytes as f64 / 1_000_000_000.0, 1))
    } else if bytes >= 995_000 {
        format!("{} MiB", printf_f(bytes as f64 / 1_000_000.0, 1))
    } else if bytes >= 995 {
        format!("{} KiB", printf_f(bytes as f64 / 1000.0, 1))
    } else {
        format!("{} B", bytes as i32)
    };

    commify(&formatted)
}

/// C++ `formatTime`: converts a quantity in seconds to a more readable format
/// (`1.1 y`, `2 mo`, `1 wk`, `3 d`, `5 h`, `10 m`, `30 s`, or `-` for less
/// than one second). The C++ `float` intermediate arithmetic is reproduced.
pub fn format_time(seconds: i64) -> String {
    // float days = (float) seconds / 86400.0;
    let days = ((seconds as f32) as f64 / 86400.0) as f32;

    if seconds >= 86400 * 365 {
        format!("{} y", printf_f(days as f64 / 365.0, 1))
    } else if seconds >= 86400 * 84 {
        format!("{} mo", (days / 30.0) as i32)
    } else if seconds >= 86400 * 13 {
        format!("{} wk", ((days as f64 / 7.0) as f32) as i32)
    } else if seconds >= 86400 {
        format!("{} d", days as i32)
    } else if seconds >= 3600 {
        format!("{} h", (seconds / 3600) as i32)
    } else if seconds >= 60 {
        format!("{} m", (seconds / 60) as i32)
    } else if seconds >= 1 {
        format!("{} s", seconds as i32)
    } else {
        "-".to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Text

/// C++ `printable (const std::string&)`: replaces CR, LF, FF, TAB and VT with
/// their backslash escapes (`\r`, `\n`, `\f`, `\t`, `\v`).
pub fn printable(input: &str) -> String {
    input
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\x0c', "\\f")
        .replace('\t', "\\t")
        .replace('\x0b', "\\v")
}

/// C++ `printable (char)`: the backslash escape for CR, LF, FF, TAB and VT,
/// otherwise the character itself.
pub fn printable_char(input: char) -> String {
    match input {
        '\r' => "\\r".to_string(),
        '\n' => "\\n".to_string(),
        '\x0c' => "\\f".to_string(),
        '\t' => "\\t".to_string(),
        '\x0b' => "\\v".to_string(),
        _ => input.to_string(),
    }
}

/// C++ `obfuscateText`: converts every character except spaces to `x`, but
/// leaves color codes (from ESC up to and including the next `m`) intact.
/// Stops at an embedded NUL, as the C++ `utf8_next_char` loop does.
pub fn obfuscate_text(input: &str) -> String {
    let mut output: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut inside = false;

    loop {
        let character = utf8_next_char(input, &mut i);
        if character == 0 {
            break;
        }

        if inside {
            // C++ `output << (char) character`.
            output.push(character as u8);

            if character == 'm' as u32 {
                inside = false;
            }
        } else {
            if character == 0o33 {
                inside = true;
            }

            if inside || character == ' ' as u32 {
                output.push(character as u8);
            } else {
                output.push(b'x');
            }
        }
    }

    String::from_utf8_lossy(&output).into_owned()
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/format.t.cpp`. Each C++ `t.is` is one assertion
    //! here, carrying the C++ test description as its message. Rust-only tests
    //! for the printf/iostream helpers and the functions format.t.cpp does not
    //! cover follow.

    use super::*;

    #[test]
    fn format_single_values() {
        // std::string format (char);
        assert_eq!(format_value(&'A'), "A", "format ('A') -> A");

        // std::string format (int);
        assert_eq!(format_value(&0), "0", "format (0) -> 0");
        assert_eq!(format_value(&-1), "-1", "format (-1) -> -1");
    }

    #[test]
    fn format_hex_values() {
        // std::string formatHex (int);
        assert_eq!(format_hex(0), "0", "formatHex (0) -> 0");
        assert_eq!(format_hex(10), "a", "formatHex (10) -> a");
        assert_eq!(format_hex(123), "7b", "formatHex (123) -> 7b");
    }

    #[test]
    fn format_width_precision() {
        // std::string format (float, int, int);
        // (The C++ literals are doubles, so these exercise format (double, int, int).)
        assert_eq!(format_double(0.12345678, 8, 4), "  0.1235", "format (0.12345678,    8,   4) -> __0.1235");

        assert_eq!(format_double(1.23456789, 8, 1), "       1", "format (1.23456789,    8,   1) -> _______1");
        assert_eq!(format_double(1.23456789, 8, 2), "     1.2", "format (1.23456789,    8,   2) -> _____1.2");
        assert_eq!(format_double(1.23456789, 8, 3), "    1.23", "format (1.23456789,    8,   3) -> ____1.23");
        assert_eq!(format_double(1.23456789, 8, 4), "   1.235", "format (1.23456789,    8,   4) -> ___1.235");
        assert_eq!(format_double(1.23456789, 8, 5), "  1.2346", "format (1.23456789,    8,   5) -> __1.2346");
        assert_eq!(format_double(1.23456789, 8, 6), " 1.23457", "format (1.23456789,    8,   6) ->  1.23457");
        assert_eq!(format_double(1.23456789, 8, 7), "1.234568", "format (1.23456789,    8,   7) -> 1.234568");
        assert_eq!(format_double(1.23456789, 8, 8), "1.2345679", "format (1.23456789,    8,   8) -> 1.2345679");
        assert_eq!(
            format_double(2444238.56789, 12, 11),
            "2444238.5679",
            "format (2444238.56789, 12, 11) -> 2444238.5679"
        );
    }

    #[test]
    fn format_positional() {
        assert_eq!(format("pre {1} post", &[&"mid"]), "pre mid post", "format 1a");
        assert_eq!(format("pre {1}{1} post", &[&"mid"]), "pre midmid post", "format 1b");
        assert_eq!(format("pre {1} post", &[&0]), "pre 0 post", "format 1c");
        assert_eq!(format("pre {1}{1} post", &[&0]), "pre 00 post", "format 1d");

        assert_eq!(format("pre {1}{2} post", &[&"one", &"two"]), "pre onetwo post", "format 2a");
        assert_eq!(format("pre {2}{1} post", &[&"one", &"two"]), "pre twoone post", "format 2b");
        assert_eq!(format("pre {1}{2} post", &[&"one", &2]), "pre one2 post", "format 2c");
        assert_eq!(format("pre {1}{2} post", &[&1, &"two"]), "pre 1two post", "format 2d");
        assert_eq!(format("pre {1}{2} post", &[&1, &2]), "pre 12 post", "format 2e");

        assert_eq!(
            format("pre {1}{2}{3} post", &[&"one", &"two", &"three"]),
            "pre onetwothree post",
            "format 3a"
        );
        assert_eq!(
            format("pre {3}{1}{2} post", &[&"one", &"two", &"three"]),
            "pre threeonetwo post",
            "format 3b"
        );
    }

    #[test]
    fn justify() {
        // std::string leftJustify (const std::string&, const int);
        assert_eq!(left_justify_int(123, 3), "123", "leftJustify 123,3 -> '123'");
        assert_eq!(left_justify_int(123, 4), "123 ", "leftJustify 123,4 -> '123 '");
        assert_eq!(left_justify_int(123, 5), "123  ", "leftJustify 123,5 -> '123  '");

        // std::string leftJustify (const std::string&, const int);
        assert_eq!(left_justify("foo", 3), "foo", "leftJustify foo,3 -> 'foo'");
        assert_eq!(left_justify("foo", 4), "foo ", "leftJustify foo,4 -> 'foo '");
        assert_eq!(left_justify("foo", 5), "foo  ", "leftJustify foo,5 -> 'foo  '");
        assert_eq!(left_justify("föo", 5), "föo  ", "leftJustify föo,5 -> 'föo  '");

        // std::string rightJustify (const std::string&, const int);
        assert_eq!(right_justify_int(123, 3), "123", "rightJustify 123,3 -> '123'");
        assert_eq!(right_justify_int(123, 4), " 123", "rightJustify 123,4 -> ' 123'");
        assert_eq!(right_justify_int(123, 5), "  123", "rightJustify 123,5 -> '  123'");

        // std::string rightJustify (const std::string&, const int);
        assert_eq!(right_justify("foo", 3), "foo", "rightJustify foo,3 -> 'foo'");
        assert_eq!(right_justify("foo", 4), " foo", "rightJustify foo,4 -> ' foo'");
        assert_eq!(right_justify("foo", 5), "  foo", "rightJustify foo,5 -> '  foo'");
        assert_eq!(right_justify("föo", 5), "  föo", "rightJustify föo,5 -> '  föo'");
    }

    #[test]
    fn commify_cases() {
        // std::string commify (const std::string& data)
        assert_eq!(commify(""), "", "commify '' -> ''");
        assert_eq!(commify("1"), "1", "commify '1' -> '1'");
        assert_eq!(commify("12"), "12", "commify '12' -> '12'");
        assert_eq!(commify("123"), "123", "commify '123' -> '123'");
        assert_eq!(commify("1234"), "1,234", "commify '1234' -> '1,234'");
        assert_eq!(commify("12345"), "12,345", "commify '12345' -> '12,345'");
        assert_eq!(commify("123456"), "123,456", "commify '123456' -> '123,456'");
        assert_eq!(commify("1234567"), "1,234,567", "commify '1234567' -> '1,234,567'");
        assert_eq!(commify("12345678"), "12,345,678", "commify '12345678' -> '12,345,678'");
        assert_eq!(commify("123456789"), "123,456,789", "commify '123456789' -> '123,456,789'");
        assert_eq!(commify("1234567890"), "1,234,567,890", "commify '1234567890' -> '1,234,567,890'");

        assert_eq!(commify("pre"), "pre", "commify 'pre' -> 'pre'");
        assert_eq!(commify("pre1234"), "pre1,234", "commify 'pre1234' -> 'pre1,234'");
        assert_eq!(commify("1234post"), "1,234post", "commify '1234post' -> '1,234post'");
        assert_eq!(commify("pre1234post"), "pre1,234post", "commify 'pre1234post' -> 'pre1,234post'");
    }

    #[test]
    fn format_bytes_cases() {
        // std::string formatBytes (size_t);
        assert_eq!(format_bytes(0), "0 B", "0 -> 0 B");

        assert_eq!(format_bytes(994), "994 B", "994 -> 994 B");
        assert_eq!(format_bytes(995), "1.0 KiB", "995 -> 1.0 KiB");
        assert_eq!(format_bytes(999), "1.0 KiB", "999 -> 1.0 KiB");
        assert_eq!(format_bytes(1000), "1.0 KiB", "1000 -> 1.0 KiB");
        assert_eq!(format_bytes(1001), "1.0 KiB", "1001 -> 1.0 KiB");

        assert_eq!(format_bytes(999999), "1.0 MiB", "999999 -> 1.0 MiB");
        assert_eq!(format_bytes(1000000), "1.0 MiB", "1000000 -> 1.0 MiB");
        assert_eq!(format_bytes(1000001), "1.0 MiB", "1000001 -> 1.0 MiB");

        assert_eq!(format_bytes(999999999), "1.0 GiB", "999999999 -> 1.0 GiB");
        assert_eq!(format_bytes(1000000000), "1.0 GiB", "1000000000 -> 1.0 GiB");
        assert_eq!(format_bytes(1000000001), "1.0 GiB", "1000000001 -> 1.0 GiB");
    }

    #[test]
    fn printable_cases() {
        // std::string printable (const std::string&);
        assert_eq!(
            printable("f\ro\no\tb\x0ba\x0br"),
            "f\\ro\\no\\tb\\va\\vr",
            "printable f\\ro\\no\\tb\\va\\vr --> f\\\\ro\\\\no\\\\tb\\\\va\\\\vr"
        );
        assert_eq!(printable("foobar"), "foobar", "printable foobar --> foobar");

        // std::string printable (char);
        assert_eq!(printable_char('\r'), "\\r", "printable \\r --> \\\\r");
        assert_eq!(printable_char('\n'), "\\n", "printable \\n --> \\\\n");
        assert_eq!(printable_char('\x0c'), "\\f", "printable \\f --> \\\\f");
        assert_eq!(printable_char('\t'), "\\t", "printable \\t --> \\\\t");
        assert_eq!(printable_char('\x0b'), "\\v", "printable \\v --> \\\\v");
        assert_eq!(printable_char('x'), "x", "printable x --> x");
    }

    #[test]
    fn obfuscate_text_cases() {
        // std::string obfuscateText (const std::string&);
        assert_eq!(obfuscate_text("foo"), "xxx", "obfuscateText 'foo' --> 'xxx'");
        assert_eq!(
            obfuscate_text("\x1b[33mfoo\x1b[0m"),
            "\x1b[33mxxx\x1b[0m",
            "obfuscateText '<red>foo</red>' --> '<red>xxx</red>'"
        );
        assert_eq!(
            obfuscate_text("one\x1b[33mtwo\x1b[0mthree"),
            "xxx\x1b[33mxxx\x1b[0mxxxxx",
            "obfuscateText 'one<red>two</red>three' --> 'xxx<red>xxx</red>xxxxx'"
        );
    }

    // ---------------------------------------------------------------------
    // Rust-only tests.

    #[test]
    fn printf_e_matches_c() {
        assert_eq!(printf_e(1234.5, 6), "1.234500e+03");
        assert_eq!(printf_e(0.000123, 2), "1.23e-04");
        assert_eq!(printf_e(-1.5e-300, 1), "-1.5e-300");
        assert_eq!(printf_e(0.0, 3), "0.000e+00");
        assert_eq!(printf_e(9.99, 1), "1.0e+01");
        assert_eq!(printf_e(1e100, 0), "1e+100");
    }

    #[test]
    fn printf_f_matches_c() {
        assert_eq!(printf_f(1.5, 6), "1.500000");
        assert_eq!(printf_f(-0.0, 2), "-0.00");
        assert_eq!(printf_f(1e21, 0), "1000000000000000000000");
        // Exact binary ties round to even, as glibc does.
        assert_eq!(printf_f(0.5, 0), "0");
        assert_eq!(printf_f(1.5, 0), "2");
        assert_eq!(printf_f(2.5, 0), "2");
        assert_eq!(printf_f(0.125, 2), "0.12");
        assert_eq!(printf_f(0.375, 2), "0.38");
        // Not a tie in binary: 0.15 is slightly below 0.15.
        assert_eq!(printf_f(0.15, 1), "0.1");
        assert_eq!(printf_f(1.05, 1), "1.1");
    }

    #[test]
    fn printf_g_matches_c() {
        assert_eq!(printf_g(0.0, 6), "0");
        assert_eq!(printf_g(-0.0, 6), "-0");
        assert_eq!(printf_g(1.0, 6), "1");
        assert_eq!(printf_g(1.5, 6), "1.5");
        assert_eq!(printf_g(100.0, 6), "100");
        assert_eq!(printf_g(123456.0, 6), "123456");
        assert_eq!(printf_g(1234567.0, 6), "1.23457e+06");
        assert_eq!(printf_g(999999.5, 6), "1e+06");
        assert_eq!(printf_g(0.0001, 6), "0.0001");
        assert_eq!(printf_g(0.00001, 6), "1e-05");
        assert_eq!(printf_g(0.000123456789, 6), "0.000123457");
        assert_eq!(printf_g(1e100, 6), "1e+100");
        assert_eq!(printf_g(-2.5e-10, 6), "-2.5e-10");
        assert_eq!(printf_g(6.02214076, 3), "6.02");
        assert_eq!(printf_g(2.5, 1), "2");
        assert_eq!(printf_g(25.0, 1), "2e+01");
        // Precision 0 is treated as 1.
        assert_eq!(printf_g(1.23456789, 0), "1");
        assert_eq!(printf_g(0.1 + 0.2, 17), "0.30000000000000004");
        assert_eq!(printf_g(f64::INFINITY, 6), "inf");
        assert_eq!(printf_g(f64::NEG_INFINITY, 6), "-inf");
        assert_eq!(printf_g(f64::NAN, 6), "nan");
        assert_eq!(printf_g(-f64::NAN, 6), "-nan");
    }

    #[test]
    fn stream_double_default_precision() {
        assert_eq!(stream_double(3.0), "3");
        assert_eq!(stream_double(0.1), "0.1");
        assert_eq!(stream_double(2.0 / 3.0), "0.666667");
        assert_eq!(stream_double(1e6), "1e+06");
        assert_eq!(stream_double(-123.456), "-123.456");
        assert_eq!(stream_double(1.5f32 as f64), "1.5");
    }

    #[test]
    fn format_arg_types() {
        // double -> format (double): std::fixed.
        assert_eq!(format("{1}", &[&1.5]), "1.500000");
        assert_eq!(format_value(&2.0f64), "2.000000");
        // float -> template format (T): default stream conversion.
        assert_eq!(format("{1}", &[&1.5f32]), "1.5");
        assert_eq!(format("{1}", &[&0.1f32]), "0.1");
        // bool streams as 1/0.
        assert_eq!(format("{1}{2}", &[&true, &false]), "10");
        // Other integer widths and owned strings.
        assert_eq!(format("{1} {2} {3}", &[&u64::MAX, &-5i64, &String::from("s")]), "18446744073709551615 -5 s");
        assert_eq!(format_value("text"), "text");
        assert_eq!(format_value(&&"ref"), "ref");
    }

    #[test]
    fn format_positional_edge_cases() {
        // No arguments: unchanged.
        assert_eq!(format("pre {1} post", &[]), "pre {1} post");
        // Unused placeholders stay.
        assert_eq!(format("{1}{3}", &[&"a", &"b"]), "a{3}");
        // An argument containing a later placeholder is substituted again, as in C++.
        assert_eq!(format("{1}", &[&"<{2}>", &"x"]), "<x>");
        // But a replacement is never rescanned for its own placeholder.
        assert_eq!(format("{1}{1}", &[&"{1}"]), "{1}{1}");
        // Multi-digit placeholders.
        let args: Vec<i32> = (1..=11).collect();
        let refs: Vec<&dyn FormatArg> = args.iter().map(|a| a as &dyn FormatArg).collect();
        assert_eq!(format("{11}-{1}", &refs), "11-1");
        // UTF-8 around placeholders.
        assert_eq!(format("ö{1}ö", &[&"☺"]), "ö☺ö");
    }

    #[test]
    fn replace_positional_cases() {
        let mut text = String::from("a-b-c");
        replace_positional(&mut text, "-", "--");
        assert_eq!(text, "a--b--c");

        let mut text = String::from("abc");
        replace_positional(&mut text, "", "x");
        assert_eq!(text, "abc");

        let mut text = String::from("xx");
        replace_positional(&mut text, "x", "");
        assert_eq!(text, "");
    }

    #[test]
    fn format_hex_negative() {
        assert_eq!(format_hex(-1), "ffffffff");
        assert_eq!(format_hex(255), "ff");
    }

    #[test]
    fn format_width_precision_more() {
        // Width smaller than the number: no truncation.
        assert_eq!(format_double(1234.5, 2, 6), "1234.5");
        // Negative width and default precision.
        assert_eq!(format_double(1.5, -1, -1), "1.5");
        // Exponent form at low precision.
        assert_eq!(format_double(123456.0, 8, 2), " 1.2e+05");
        // Near-zero rounding to width - 2 decimals before %g.
        assert_eq!(format_double(0.000049, 6, 4), "     0");
        assert_eq!(format_double(0.00006, 6, 4), "0.0001");
        // Zero and negatives are not pre-rounded.
        assert_eq!(format_double(0.0, 4, 2), "   0");
        assert_eq!(format_double(-0.123456, 10, 3), "    -0.123");

        // format (float, int, int), with the double -> float argument
        // conversion a C++ caller passing a double literal would get.
        let (small, one): (f64, f64) = (0.12345678, 1.23456789);
        assert_eq!(format_float(small as f32, 8, 4), "  0.1235");
        assert_eq!(format_float(one as f32, 8, 6), " 1.23457");
        assert_eq!(format_float(2.5, 4, 6), " 2.5");
        // float precision shows through at high precision.
        assert_eq!(format_float(0.1, 12, 10), "0.1000000015");
    }

    #[test]
    fn justify_more() {
        assert_eq!(left_justify_int(-12, 5), "-12  ");
        assert_eq!(left_justify_int(12345, 2), "12345");
        assert_eq!(right_justify_int(-12, 5), "  -12");
        assert_eq!(right_justify_zero(5, 3), "005");
        assert_eq!(right_justify_zero(123, 2), "123");
        assert_eq!(right_justify_zero(-5, 4), "00-5");
        // Wide characters and color codes are measured in display cells.
        assert_eq!(left_justify("改", 4), "改  ");
        assert_eq!(right_justify("改", 4), "  改");
        assert_eq!(left_justify("\x1b[1mab\x1b[0m", 4), "\x1b[1mab\x1b[0m  ");
        assert_eq!(right_justify("abcdef", 3), "abcdef");
    }

    #[test]
    fn commify_more() {
        assert_eq!(commify("1234.5678"), "1,234.5678");
        assert_eq!(commify("1234567.1"), "1,234,567.1");
        assert_eq!(commify("-1234"), "-1,234");
        // As in C++, the digit counter is not reset by non-digits.
        assert_eq!(commify("1234 and 5678"), "12,34 and 5,678");
        // The last decimal point decides.
        assert_eq!(commify("1234.5678.9"), "12,34.5,678.9");
        assert_eq!(commify("ö1234"), "ö1,234");
    }

    #[test]
    fn format_bytes_more() {
        assert_eq!(format_bytes(1500), "1.5 KiB");
        assert_eq!(format_bytes(994999), "995.0 KiB");
        assert_eq!(format_bytes(1234567890123), "1,234.6 GiB");
    }

    #[test]
    fn format_time_cases() {
        assert_eq!(format_time(-5), "-");
        assert_eq!(format_time(0), "-");
        assert_eq!(format_time(1), "1 s");
        assert_eq!(format_time(59), "59 s");
        assert_eq!(format_time(60), "1 m");
        assert_eq!(format_time(3599), "59 m");
        assert_eq!(format_time(3600), "1 h");
        assert_eq!(format_time(86399), "23 h");
        assert_eq!(format_time(86400), "1 d");
        assert_eq!(format_time(86400 * 13 - 1), "12 d");
        assert_eq!(format_time(86400 * 13), "1 wk");
        assert_eq!(format_time(86400 * 84 - 1), "11 wk");
        assert_eq!(format_time(86400 * 84), "2 mo");
        assert_eq!(format_time(86400 * 365 - 1), "12 mo");
        assert_eq!(format_time(86400 * 365), "1.0 y");
        assert_eq!(format_time(86400 * 400), "1.1 y");
    }

    #[test]
    fn printable_more() {
        assert_eq!(printable("a\x0cb"), "a\\fb");
        assert_eq!(printable(""), "");
        assert_eq!(printable_char('\0'), "\0");
        assert_eq!(printable_char('é'), "é");
    }

    #[test]
    fn obfuscate_text_more() {
        assert_eq!(obfuscate_text("a b"), "x x");
        // A multi-byte character becomes a single x.
        assert_eq!(obfuscate_text("föo ☺"), "xxx x");
        assert_eq!(obfuscate_text(""), "");
        // Stops at an embedded NUL.
        assert_eq!(obfuscate_text("ab\0cd"), "xx");
    }
}
