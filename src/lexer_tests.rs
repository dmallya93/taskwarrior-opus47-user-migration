//! Complete port of Taskwarrior `test/lexer_test.cpp` (PRODUCT_TASKWARRIOR,
//! 1291 assertions), with the C++ messages as assertion messages.
//!
//! Every test applies the same static setup as the C++ `main` (Datetime /
//! Duration flags as in `Context::staticInitialization`, plus the
//! `Lexer::attributes` entries) through [`Upstream`], which also holds
//! [`crate::test_util::env_lock`] with `TZ=UTC` (the date/duration lexing of
//! "today", "9th", ... goes through localtime). Everything is restored when
//! the guard drops, before the lock is released.

use super::*;
use crate::test_util::{env_lock, EnvGuard};
use std::sync::MutexGuard;

/// C++ `main` static setup; restores the statics, `TZ` and releases the lock on drop.
struct Upstream {
    attributes: BTreeMap<String, String>,
    iso: bool,
    standalone_date: bool,
    standalone_time: bool,
    standalone_seconds: bool,
    env: Option<EnvGuard>,
    _lock: MutexGuard<'static, ()>,
}

impl Upstream {
    fn setup() -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["TZ"]);
        std::env::set_var("TZ", "UTC");
        crate::shared::localtime::tzset();

        let guard = Upstream {
            attributes: Lexer::attributes(),
            iso: Datetime::iso_enabled(),
            standalone_date: Datetime::standalone_date_enabled(),
            standalone_time: Datetime::standalone_time_enabled(),
            standalone_seconds: Duration::standalone_seconds_enabled(),
            env: Some(env),
            _lock: lock,
        };

        // Use same Datetime/Duration configuration as Context::staticInitialization.
        Datetime::set_iso_enabled(true);
        Datetime::set_standalone_date_enabled(false);
        Datetime::set_standalone_time_enabled(false);
        Duration::set_standalone_seconds_enabled(false);

        // Feed in some attributes and types, so that the Lexer knows what a DOM
        // reference is.
        Lexer::insert_attribute("due", "date");
        Lexer::insert_attribute("tags", "string");
        Lexer::insert_attribute("description", "string");
        guard
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        Lexer::set_attributes(std::mem::take(&mut self.attributes));
        Datetime::set_iso_enabled(self.iso);
        Datetime::set_standalone_date_enabled(self.standalone_date);
        Datetime::set_standalone_time_enabled(self.standalone_time);
        Duration::set_standalone_seconds_enabled(self.standalone_seconds);
        drop(self.env.take());
        crate::shared::localtime::tzset();
        // `_lock` is released after this body returns.
    }
}

fn lex_all(text: &str) -> Vec<(String, Type)> {
    let mut lexer = Lexer::new(text);
    let mut token = String::new();
    let mut type_ = Type::word;
    let mut tokens = Vec::new();
    while lexer.token(&mut token, &mut type_) {
        tokens.push((token.clone(), type_));
    }
    tokens
}

fn c(ch: char) -> i32 {
    ch as i32
}

#[test]
fn is_boundary_was_quoted_dequote() {
    let _u = Upstream::setup();

    // static bool Lexer::isBoundary (int, int);
    assert!(Lexer::is_boundary(c(' '), c('a')), "' ' --> 'a' = isBoundary");
    assert!(Lexer::is_boundary(c('a'), c(' ')), "'a' --> ' ' = isBoundary");
    assert!(Lexer::is_boundary(c(' '), c('+')), "' ' --> '+' = isBoundary");
    assert!(Lexer::is_boundary(c(' '), c(',')), "' ' --> ',' = isBoundary");
    assert!(!Lexer::is_boundary(c('3'), c('4')), "'3' --> '4' = isBoundary");
    assert!(Lexer::is_boundary(c('('), c('(')), "'(' --> '(' = isBoundary");
    assert!(!Lexer::is_boundary(c('r'), c('d')), "'r' --> 'd' = isBoundary");

    // static bool Lexer::wasQuoted (const std::string&);
    assert!(!Lexer::was_quoted(""), "'' --> !wasQuoted");
    assert!(!Lexer::was_quoted("foo"), "'foo' --> !wasQuoted");
    assert!(Lexer::was_quoted("a b"), "'a b' --> wasQuoted");
    assert!(Lexer::was_quoted("(a)"), "'(a)' --> wasQuoted");

    // static bool Lexer::dequote (std::string&, const std::string& quotes = "'\"");
    let mut token = "foo".to_string();
    Lexer::dequote(&mut token);
    assert_eq!(token, "foo", "dequote foo --> foo");

    token = "'foo'".to_string();
    Lexer::dequote(&mut token);
    assert_eq!(token, "foo", "dequote 'foo' --> foo");

    token = "'o\\'clock'".to_string();
    Lexer::dequote(&mut token);
    assert_eq!(token, "o\\'clock", "dequote 'o\\'clock' --> o\\'clock");

    token = "abba".to_string();
    Lexer::dequote_with(&mut token, "a");
    assert_eq!(token, "bb", "dequote 'abba' (a) --> bb");
}

#[test]
fn no_tokens_and_mixed_tokens() {
    let _u = Upstream::setup();
    let mut token = String::new();
    let mut type_ = Type::word;

    // Should result in no tokens.
    let mut l0 = Lexer::new("");
    assert!(!l0.token(&mut token, &mut type_), "'' --> no tokens");

    // Should result in no tokens.
    let mut l1 = Lexer::new("       \t ");
    assert!(!l1.token(&mut token, &mut type_), "'       \\t ' --> no tokens");

    // € = Euro symbol.
    let tokens = lex_all(r" one 'two \'three\''+456-(1.3*2 - 0x12) 1.2e-3.4    foo.bar and '€'");
    let n = |i: usize| Lexer::type_name(&tokens[i].1);

    assert_eq!(tokens[0].0, "one", "tokens[0] = 'one'");
    assert_eq!(n(0), "identifier", "tokens[0] = identifier");
    assert_eq!(tokens[1].0, "'two 'three''", "tokens[1] = 'two 'three''");
    assert_eq!(n(1), "string", "tokens[1] = string");
    assert_eq!(tokens[2].0, "+", "tokens[2] = '+'");
    assert_eq!(n(2), "op", "tokens[2] = op");
    assert_eq!(tokens[3].0, "456", "tokens[3] = '456'");
    assert_eq!(n(3), "number", "tokens[3] = number");
    assert_eq!(tokens[4].0, "-", "tokens[4] = '-'");
    assert_eq!(n(4), "op", "tokens[4] = op");
    assert_eq!(tokens[5].0, "(", "tokens[5] = '('");
    assert_eq!(n(5), "op", "tokens[5] = op");
    assert_eq!(tokens[6].0, "1.3", "tokens[6] = '1.3'");
    assert_eq!(n(6), "number", "tokens[6] = number");
    assert_eq!(tokens[7].0, "*", "tokens[7] = '*'");
    assert_eq!(n(7), "op", "tokens[7] = op");
    assert_eq!(tokens[8].0, "2", "tokens[8] = '2'");
    assert_eq!(n(8), "number", "tokens[8] = number");
    assert_eq!(tokens[9].0, "-", "tokens[9] = '-'");
    assert_eq!(n(9), "op", "tokens[9] = op");
    assert_eq!(tokens[10].0, "0x12", "tokens[10] = '0x12'");
    assert_eq!(n(10), "hex", "tokens[10] = hex");
    assert_eq!(tokens[11].0, ")", "tokens[11] = ')'");
    assert_eq!(n(11), "op", "tokens[11] = op");
    assert_eq!(tokens[12].0, "1.2e-3.4", "tokens[12] = '1.2e-3.4'");
    assert_eq!(n(12), "number", "tokens[12] = number");
    assert_eq!(tokens[13].0, "foo.bar", "tokens[13] = 'foo.bar'");
    assert_eq!(n(13), "identifier", "tokens[13] = identifier");
    assert_eq!(tokens[14].0, "and", "tokens[14] = 'and'");
    assert_eq!(n(14), "op", "tokens[14] = op");
    assert_eq!(tokens[15].0, "'€'", "tokens[15] = \\u20ac --> ''€''");
    assert_eq!(n(15), "string", "tokens[15] = string");

    // Test for numbers that are no longer ISO-8601 dates.
    let tokens = lex_all("1 12 123 1234 12345 123456 1234567");
    assert_eq!(tokens.len(), 7, "7 tokens");
    for (i, expected) in ["1", "12", "123", "1234", "12345", "123456", "1234567"].iter().enumerate() {
        assert_eq!(tokens[i].0, *expected, "tokens[{i}] == '{expected}'");
        assert_eq!(tokens[i].1, Type::number, "tokens[{i}] == Type::number");
    }
}

#[test]
fn split() {
    let _u = Upstream::setup();

    // void split (std::vector<std::string>&, const std::string&);
    let items = Lexer::split(" ( A or B ) ");
    assert_eq!(items.len(), 5, "split ' ( A or B ) '");
    assert_eq!(items[0], "(", "split ' ( A or B ) ' -> [0] '('");
    assert_eq!(items[1], "A", "split ' ( A or B ) ' -> [1] 'A'");
    assert_eq!(items[2], "or", "split ' ( A or B ) ' -> [2] 'or'");
    assert_eq!(items[3], "B", "split ' ( A or B ) ' -> [3] 'B'");
    assert_eq!(items[4], ")", "split ' ( A or B ) ' -> [4] ')'");

    // Test simple mode with contrived tokens that ordinarily split.
    let items = Lexer::split("  +-* a+b 12.3e4 'c d'");
    assert_eq!(items.len(), 8, "split '  +-* a+b 12.3e4 'c d''");
    assert_eq!(items[0], "+", "split '  +-* a+b 12.3e4 'c d'' -> [0] '+'");
    assert_eq!(items[1], "-", "split '  +-* a+b 12.3e4 'c d'' -> [1] '-'");
    assert_eq!(items[2], "*", "split '  +-* a+b 12.3e4 'c d'' -> [2] '*'");
    assert_eq!(items[3], "a", "split '  +-* a+b 12.3e4 'c d'' -> [3] 'a'");
    assert_eq!(items[4], "+", "split '  +-* a+b 12.3e4 'c d'' -> [4] '+'");
    assert_eq!(items[5], "b", "split '  +-* a+b 12.3e4 'c d'' -> [5] 'b'");
    assert_eq!(items[6], "12.3e4", "split '  +-* a+b 12.3e4 'c d'' -> [6] '12.3e4'");
    assert_eq!(items[7], "'c d'", "split '  +-* a+b 12.3e4 'c d'' -> [7] ''c d''");
}

#[test]
fn decompose_pair() {
    let _u = Upstream::setup();

    // static bool decomposePair (...); 2 * 4 * 2 * 5 = 80 tests.
    let (mut out_name, mut out_mod, mut out_value, mut out_sep) =
        (String::new(), String::new(), String::new(), String::new());
    for name in ["name"] {
        for modifier in ["", "mod"] {
            for sep in [":", "=", "::", ":="] {
                for value in ["", "value", "a:b", "a::b", "a=b", "a:=b"] {
                    let input = format!(
                        "name{}{modifier}{sep}{value}",
                        if modifier.is_empty() { "" } else { "." }
                    );
                    assert!(
                        Lexer::decompose_pair(&input, &mut out_name, &mut out_mod, &mut out_sep, &mut out_value),
                        "decomposePair '{input}' --> true"
                    );
                    assert_eq!(name, out_name, "  '{input}' --> name '{name}'");
                    assert_eq!(modifier, out_mod, "  '{input}' --> mod '{modifier}'");
                    assert_eq!(value, out_value, "  '{input}' --> value '{value}'");
                    assert_eq!(sep, out_sep, "  '{input}' --> sep '{sep}'");
                }
            }
        }
    }
}

#[test]
fn read_word() {
    let _u = Upstream::setup();

    // static bool readWord (const std::string&, const std::string&, std::string::size_type&, std::string&);
    let mut cursor: usize = 0;
    let mut word = String::new();
    assert!(Lexer::read_word_quoted("'one two'", "'\"", &mut cursor, &mut word), "readWord ''one two'' --> true");
    assert_eq!(word, "'one two'", "  word '{word}'");
    assert_eq!(cursor, 9, "  cursor");

    // Unterminated quoted string is invalid.
    cursor = 0;
    assert!(!Lexer::read_word_quoted("'one", "'\"", &mut cursor, &mut word), "readWord ''one' --> false");

    // static bool readWord (const std::string&, std::string::size_type&, std::string&);
    cursor = 0;
    assert!(Lexer::read_word("input", &mut cursor, &mut word), "readWord 'input' --> true");
    assert_eq!(word, "input", "  word '{word}'");
    assert_eq!(cursor, 5, "  cursor");

    cursor = 0;
    assert!(Lexer::read_word("one\\ two", &mut cursor, &mut word), "readWord 'one\\ two' --> true");
    assert_eq!(word, "one two", "  word '{word}'");
    assert_eq!(cursor, 8, "  cursor");

    cursor = 0;
    assert!(Lexer::read_word("\\u20A43", &mut cursor, &mut word), "readWord '\\u20A43' --> true");
    assert_eq!(word, "₤3", "  word '{word}'");
    assert_eq!(cursor, 7, "  cursor");

    cursor = 0;
    assert!(Lexer::read_word("U+20AC4", &mut cursor, &mut word), "readWord '\\u20AC4' --> true");
    assert_eq!(word, "€4", "  word '{word}'");
    assert_eq!(cursor, 7, "  cursor");

    let text = "one 'two' three\\ four";
    cursor = 0;
    assert!(Lexer::read_word(text, &mut cursor, &mut word), r#"readWord "one 'two' three\ four" --> true"#);
    assert_eq!(word, "one", "  word '{word}'");
    cursor += 1;
    assert!(Lexer::read_word(text, &mut cursor, &mut word), r#"readWord "one 'two' three\ four" --> true"#);
    assert_eq!(word, "'two'", "  word '{word}'");
    cursor += 1;
    assert!(Lexer::read_word(text, &mut cursor, &mut word), r#"readWord "one 'two' three\ four" --> true"#);
    assert_eq!(word, "three four", "  word '{word}'");

    let text = "one     ";
    cursor = 0;
    assert!(Lexer::read_word(text, &mut cursor, &mut word), "readWord \"one     \" --> true");
    assert_eq!(word, "one", "  word '{word}'");
}

#[test]
fn is_literal_is_one_of_common_length() {
    let _u = Upstream::setup();

    // bool isLiteral (const std::string&, bool, bool);
    let mut l4 = Lexer::new("one.two");
    assert!(!l4.is_literal("zero", false, false), "isLiteral 'one.two' --> false");
    assert!(l4.is_literal("one", false, false), "isLiteral 'one.two' --> 'one'");
    assert!(l4.is_literal(".", false, false), "isLiteral 'one.two' --> '.'");
    assert!(l4.is_literal("two", false, true), "isLiteral 'one.two' --> 'two'");

    let mut l5 = Lexer::new("wonder");
    assert!(
        !l5.is_literal("wonderful", false, false),
        "isLiteral 'wonderful' != 'wonder' without abbreviation"
    );
    assert!(l5.is_literal("wonderful", true, false), "isLiteral 'wonderful' == 'wonder' with abbreviation");

    // bool isOneOf (const std::string&, bool, bool);
    let mut l6 = Lexer::new("Grumpy.");
    let dwarves = ["Sneezy", "Doc", "Bashful", "Grumpy", "Happy", "Sleepy", "Dopey"];
    assert!(!l6.is_one_of(&dwarves, false, true), "isOneof ('Grumpy', true) --> false");
    assert!(l6.is_one_of(&dwarves, false, false), "isOneOf ('Grumpy', false) --> true");

    // static std::string::size_type commonLength (const std::string&, const std::string&);
    assert_eq!(Lexer::common_length("", ""), 0, "commonLength '' : '' --> 0");
    assert_eq!(Lexer::common_length("a", "a"), 1, "commonLength 'a' : 'a' --> 1");
    assert_eq!(Lexer::common_length("abcde", "abcde"), 5, "commonLength 'abcde' : 'abcde' --> 5");
    assert_eq!(Lexer::common_length("abc", ""), 0, "commonLength 'abc' : '' --> 0");
    assert_eq!(Lexer::common_length("abc", "def"), 0, "commonLength 'abc' : 'def' --> 0");
    assert_eq!(Lexer::common_length("foobar", "foo"), 3, "commonLength 'foobar' : 'foo' --> 3");
    assert_eq!(Lexer::common_length("foo", "foobar"), 3, "commonLength 'foo' : 'foobar' --> 3");

    // static std::string::size_type commonLength (const std::string&, size_type, const std::string&, size_type);
    assert_eq!(
        Lexer::common_length_at("wonder", 0, "prowonderbread", 3),
        6,
        "'wonder'+0 : 'prowonderbread'+3 --> 6"
    );
}

/// One expected result: token, type, expfail_token, expfail_type.
type Expect = (&'static str, Type, bool, bool);

const fn r(token: &'static str, type_: Type) -> Expect {
    (token, type_, false, false)
}

/// The C++ `lexerTests` table (the `NO` placeholders are omitted: the C++
/// loop skips them).
fn lexer_tests() -> Vec<(&'static str, Vec<Expect>)> {
    use Type::*;
    vec![
        // Pattern
        ("/foo/", vec![r("/foo/", pattern)]),
        ("/a\\/b/", vec![r("/a\\/b/", pattern)]),
        ("/'/", vec![r("/'/", pattern)]),
        // Substitution
        ("/from/to/g", vec![r("/from/to/g", substitution)]),
        ("/from/to/", vec![r("/from/to/", substitution)]),
        // Tag
        ("+tag", vec![r("+tag", tag)]),
        ("-tag", vec![r("-tag", tag)]),
        ("+@tag", vec![r("+@tag", tag)]),
        // Path
        ("/long/path/to/file.txt", vec![r("/long/path/to/file.txt", path)]),
        // Word
        ("1.foo.bar", vec![r("1.foo.bar", word)]),
        // Identifier
        ("foo", vec![r("foo", identifier)]),
        ("Çirçös", vec![r("Çirçös", identifier)]),
        ("☺", vec![r("☺", identifier)]),
        ("name", vec![r("name", identifier)]),
        ("f1", vec![r("f1", identifier)]),
        ("foo.bar", vec![r("foo.bar", identifier)]),
        ("a1a1a1a1_a1a1_a1a1_a1a1_a1a1a1a1a1a1", vec![r("a1a1a1a1_a1a1_a1a1_a1a1_a1a1a1a1a1a1", identifier)]),
        // Word that starts wih 'or', which is an operator, but should be ignored.
        ("ordinary", vec![r("ordinary", identifier)]),
        // DOM
        ("due", vec![r("due", dom)]),
        ("123.tags", vec![r("123.tags", dom)]),
        ("123.tags.PENDING", vec![r("123.tags.PENDING", dom)]),
        ("123.description", vec![r("123.description", dom)]),
        ("123.annotations.1.description", vec![r("123.annotations.1.description", dom)]),
        ("123.annotations.1.entry", vec![r("123.annotations.1.entry", dom)]),
        ("123.annotations.1.entry.year", vec![r("123.annotations.1.entry.year", dom)]),
        ("a360fc44-315c-4366-b70c-ea7e7520b749.due", vec![r("a360fc44-315c-4366-b70c-ea7e7520b749.due", dom)]),
        ("12345678-1234-1234-1234-123456789012.due", vec![r("12345678-1234-1234-1234-123456789012.due", dom)]),
        ("system.os", vec![r("system.os", dom)]),
        ("rc.foo", vec![r("rc.foo", dom)]),
        ("rc.context.my-ctx", vec![r("rc.context.my-ctx", dom)]),
        ("rc.foo-", vec![r("rc.foo", dom), r("-", op)]),
        ("due-1d", vec![r("due", dom), r("-", op), r("1d", duration)]),
        // URL
        ("http://example.com", vec![r("http://example.com", url)]),
        ("https://foo.example.com", vec![r("https://foo.example.com", url)]),
        // String
        ("'one two'", vec![r("'one two'", string)]),
        ("\"three\"", vec![r("\"three\"", string)]),
        ("'\\''", vec![r("'''", string)]),
        (r#""\"""#, vec![r(r#"""""#, string)]),
        ("\"\tfoo\t\"", vec![r("\"\tfoo\t\"", string)]),
        (r#""₤3""#, vec![r("\"₤3\"", string)]),
        ("\"U+20AC4\"", vec![r("\"€4\"", string)]),
        // Number
        ("1", vec![r("1", number)]),
        ("3.14", vec![r("3.14", number)]),
        ("6.02217e23", vec![r("6.02217e23", number)]),
        ("1.2e-3.4", vec![r("1.2e-3.4", number)]),
        ("0x2f", vec![r("0x2f", hex)]),
        // Set (1,2,4-7,9)
        ("1,2", vec![r("1,2", set)]),
        ("1-2", vec![r("1-2", set)]),
        ("1-2,4", vec![r("1-2,4", set)]),
        ("1-2,4,6-8", vec![r("1-2,4,6-8", set)]),
        ("1-2,4,6-8,10-12", vec![r("1-2,4,6-8,10-12", set)]),
        // Pair
        ("name:value", vec![r("name:value", pair)]),
        ("name=value", vec![r("name=value", pair)]),
        ("name:=value", vec![r("name:=value", pair)]),
        ("name.mod:value", vec![r("name.mod:value", pair)]),
        ("name.mod=value", vec![r("name.mod=value", pair)]),
        ("name:", vec![r("name:", pair)]),
        ("name=", vec![r("name=", pair)]),
        ("name.mod:", vec![r("name.mod:", pair)]),
        ("name.mod=", vec![r("name.mod=", pair)]),
        ("pro:'P 1'", vec![r("pro:'P 1'", pair)]),
        ("rc:x", vec![r("rc:x", pair)]),
        ("rc.name:value", vec![r("rc.name:value", pair)]),
        ("rc.name=value", vec![r("rc.name=value", pair)]),
        ("rc.name:=value", vec![r("rc.name:=value", pair)]),
        ("due:='eow - 2d'", vec![r("due:='eow - 2d'", pair)]),
        ("name:'foo\nbar'", vec![r("name:'foo\nbar'", pair)]),
        // Operator - complete set
        ("^", vec![r("^", op)]),
        ("!", vec![r("!", op)]),
        ("_neg_", vec![r("_neg_", op)]),
        ("_pos_", vec![r("_pos_", op)]),
        ("_hastag_", vec![r("_hastag_", op)]),
        ("_notag_", vec![r("_notag_", op)]),
        ("*", vec![r("*", op)]),
        ("/", vec![r("/", op)]),
        ("%", vec![r("%", op)]),
        ("+", vec![r("+", op)]),
        ("-", vec![r("-", op)]),
        ("<=", vec![r("<=", op)]),
        (">=", vec![r(">=", op)]),
        (">", vec![r(">", op)]),
        ("<", vec![r("<", op)]),
        ("=", vec![r("=", op)]),
        ("==", vec![r("==", op)]),
        ("!=", vec![r("!=", op)]),
        ("!==", vec![r("!==", op)]),
        ("~", vec![r("~", op)]),
        ("!~", vec![r("!~", op)]),
        ("and", vec![r("and", op)]),
        ("or", vec![r("or", op)]),
        ("xor", vec![r("xor", op)]),
        ("(", vec![r("(", op)]),
        (")", vec![r(")", op)]),
        // UUID
        ("ffffffff-ffff-ffff-ffff-ffffffffffff", vec![r("ffffffff-ffff-ffff-ffff-ffffffffffff", uuid)]),
        (
            "0000000d-0000-0000-0000-000000000000",
            vec![("0000000d-0000-0000-0000-000000000000", uuid, true, true)],
        ),
        ("00000000-0000-0000-0000-0000000", vec![r("00000000-0000-0000-0000-0000000", uuid)]),
        ("00000000-0000-0000-0000", vec![r("00000000-0000-0000-0000", uuid)]),
        ("00000000-0000-0000", vec![r("00000000-0000-0000", uuid)]),
        ("00000000-0000", vec![r("00000000-0000", uuid)]),
        ("00000000", vec![r("00000000", uuid)]),
        ("a360fc44-315c-4366-b70c-ea7e7520b749", vec![r("a360fc44-315c-4366-b70c-ea7e7520b749", uuid)]),
        ("a360fc44-315c-4366-b70c-ea7e752", vec![r("a360fc44-315c-4366-b70c-ea7e752", uuid)]),
        ("a360fc44-315c-4366-b70c", vec![r("a360fc44-315c-4366-b70c", uuid)]),
        ("a360fc44-315c-4366", vec![r("a360fc44-315c-4366", uuid)]),
        ("a360fc44-315c", vec![r("a360fc44-315c", uuid)]),
        ("a360fc44", vec![r("a360fc44", uuid)]),
        // Date
        ("2015-W01", vec![r("2015-W01", date)]),
        ("2015-02-17", vec![r("2015-02-17", date)]),
        ("2013-11-29T22:58:00Z", vec![r("2013-11-29T22:58:00Z", date)]),
        ("20131129T225800Z", vec![r("20131129T225800Z", date)]),
        // PRODUCT_TASKWARRIOR
        ("9th", vec![r("9th", date)]),
        ("10th", vec![r("10th", date)]),
        ("today", vec![r("today", date)]),
        // Duration
        ("year", vec![r("year", duration)]),
        ("4weeks", vec![r("4weeks", duration)]),
        ("PT23H", vec![r("PT23H", duration)]),
        ("1second", vec![r("1second", duration)]),
        ("1s", vec![r("1s", duration)]),
        ("1minute", vec![r("1minute", duration)]),
        ("2hour", vec![r("2hour", duration)]),
        ("3 days", vec![r("3 days", duration)]),
        ("4w", vec![r("4w", duration)]),
        ("5mo", vec![r("5mo", duration)]),
        ("6 years", vec![r("6 years", duration)]),
        ("P1Y", vec![r("P1Y", duration)]),
        ("PT1H", vec![r("PT1H", duration)]),
        ("P1Y1M1DT1H1M1S", vec![r("P1Y1M1DT1H1M1S", duration)]),
        // Misc
        ("--", vec![r("--", separator)]),
        // Expression
        ("(1+2)", vec![r("(", op), r("1", number), r("+", op), r("2", number), r(")", op)]),
        ("description~pattern", vec![r("description", dom), r("~", op), r("pattern", identifier)]),
        ("(+tag)", vec![r("(", op), r("+tag", tag), r(")", op)]),
        ("(name:value)", vec![r("(", op), r("name:value", pair), r(")", op)]),
    ]
}

/// C++ `t.is (..., expfail)`: with `expfail` the comparison is expected to
/// fail (a match is reported as "# FIXED" and counted as a failure).
/// Failures are collected so one run reports every divergence.
fn check_results(lexer: &mut Lexer, input: &str, results: &[Expect], ok_message: &str, failures: &mut Vec<String>) {
    let mut token = String::new();
    let mut type_ = Type::word;
    for &(etoken, etype, expfail_token, expfail_type) in results {
        if !lexer.token(&mut token, &mut type_) {
            failures.push(format!("[{input:?}] {ok_message}: got false, expected {etoken:?}"));
            return;
        }
        if (token == etoken) == expfail_token {
            let fixed = if expfail_token { " # FIXED" } else { "" };
            failures.push(format!("[{input:?}]   token --> {token}{fixed}\n# expected: {etoken}\n#      got: {token}"));
        }
        if (type_ == etype) == expfail_type {
            let fixed = if expfail_type { " # FIXED" } else { "" };
            failures.push(format!(
                "[{input:?}]   type --> Lexer::Type::{}{fixed}\n# expected: {etype:?}\n#      got: {type_:?}",
                Lexer::type_to_string(type_)
            ));
        }
    }
}

#[test]
fn lexer_tests_table() {
    let _u = Upstream::setup();
    let mut failures = Vec::new();

    for (input, results) in lexer_tests() {
        // The isolated test puts the input string directly into the Lexer.
        let mut isolated = Lexer::new(input);
        check_results(&mut isolated, input, &results, "Isolated Lexer::token(...) --> true", &mut failures);

        // The embedded test surrounds the input string with a space.
        let mut embedded = Lexer::new(&format!(" {input} "));
        check_results(&mut embedded, input, &results, "Embedded Lexer::token(...) --> true", &mut failures);
    }

    assert!(failures.is_empty(), "{} lexerTests failures:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn type_names() {
    let _u = Upstream::setup();
    for (type_, name) in [
        (Type::uuid, "uuid"),
        (Type::number, "number"),
        (Type::hex, "hex"),
        (Type::string, "string"),
        (Type::url, "url"),
        (Type::pair, "pair"),
        (Type::set, "set"),
        (Type::separator, "separator"),
        (Type::tag, "tag"),
        (Type::path, "path"),
        (Type::substitution, "substitution"),
        (Type::pattern, "pattern"),
        (Type::op, "op"),
        (Type::dom, "dom"),
        (Type::identifier, "identifier"),
        (Type::word, "word"),
        (Type::date, "date"),
        (Type::duration, "duration"),
    ] {
        assert_eq!(Lexer::type_name(&type_), name, "Lexer::typeName (Lexer::Type::{name})");
    }
}

#[test]
fn lower_case_commify_trim() {
    let _u = Upstream::setup();

    // std::string Lexer::lowerCase (const std::string& input)
    assert_eq!(Lexer::lower_case(""), "", "Lexer::lowerCase '' -> ''");
    assert_eq!(Lexer::lower_case("pre01_:POST"), "pre01_:post", "Lexer::lowerCase 'pre01_:POST' -> 'pre01_:post'");

    // std::string Lexer::commify (const std::string& data)
    for (input, expected) in [
        ("", ""),
        ("1", "1"),
        ("12", "12"),
        ("123", "123"),
        ("1234", "1,234"),
        ("12345", "12,345"),
        ("123456", "123,456"),
        ("1234567", "1,234,567"),
        ("12345678", "12,345,678"),
        ("123456789", "123,456,789"),
        ("1234567890", "1,234,567,890"),
        ("1.0", "1.0"),
        ("12.0", "12.0"),
        ("123.0", "123.0"),
        ("1234.0", "1,234.0"),
        ("12345.0", "12,345.0"),
        ("123456.0", "123,456.0"),
        ("1234567.0", "1,234,567.0"),
        ("12345678.0", "12,345,678.0"),
        ("123456789.0", "123,456,789.0"),
        ("1234567890.0", "1,234,567,890.0"),
        ("pre", "pre"),
        ("pre1234", "pre1,234"),
        ("1234post", "1,234post"),
        ("pre1234post", "pre1,234post"),
    ] {
        assert_eq!(Lexer::commify(input), expected, "Lexer::commify '{input}' -> '{expected}'");
    }

    // std::string Lexer::trimLeft (const std::string& in, const std::string& t /*= " "*/)
    assert_eq!(Lexer::trim_left(""), "", "Lexer::trimLeft '' -> ''");
    assert_eq!(Lexer::trim_left("   "), "", "Lexer::trimLeft '   ' -> ''");
    assert_eq!(Lexer::trim_left_with("", " \t"), "", "Lexer::trimLeft '' -> ''");
    assert_eq!(Lexer::trim_left("xxx"), "xxx", "Lexer::trimLeft 'xxx' -> 'xxx'");
    assert_eq!(Lexer::trim_left_with("xxx", " \t"), "xxx", "Lexer::trimLeft 'xxx' -> 'xxx'");
    assert_eq!(
        Lexer::trim_left("  \t xxx \t  "),
        "\t xxx \t  ",
        r"Lexer::trimLeft '  \t xxx \t  ' -> '\t xxx \t  '"
    );
    assert_eq!(
        Lexer::trim_left_with("  \t xxx \t  ", " \t"),
        "xxx \t  ",
        r"Lexer::trimLeft '  \t xxx \t  ' -> 'xxx \t  '"
    );

    // std::string Lexer::trimRight (const std::string& in, const std::string& t /*= " "*/)
    assert_eq!(Lexer::trim_right(""), "", "Lexer::trimRight '' -> ''");
    assert_eq!(Lexer::trim_right("   "), "", "Lexer::trimRight '   ' -> ''");
    assert_eq!(Lexer::trim_right_with("", " \t"), "", "Lexer::trimRight '' -> ''");
    assert_eq!(Lexer::trim_right("xxx"), "xxx", "Lexer::trimRight 'xxx' -> 'xxx'");
    assert_eq!(Lexer::trim_right_with("xxx", " \t"), "xxx", "Lexer::trimRight 'xxx' -> 'xxx'");
    assert_eq!(
        Lexer::trim_right("  \t xxx \t  "),
        "  \t xxx \t",
        r"Lexer::trimRight '  \t xxx \t  ' -> '  \t xxx \t'"
    );
    assert_eq!(
        Lexer::trim_right_with("  \t xxx \t  ", " \t"),
        "  \t xxx",
        r"Lexer::trimRight '  \t xxx \t  ' -> '  \t xxx'"
    );

    // std::string Lexer::trim (const std::string& in, const std::string& t /*= " "*/)
    assert_eq!(Lexer::trim(""), "", "Lexer::trim '' -> ''");
    assert_eq!(Lexer::trim("   "), "", "Lexer::trim '   ' -> ''");
    assert_eq!(Lexer::trim_with("", " \t"), "", "Lexer::trim '' -> ''");
    assert_eq!(Lexer::trim("xxx"), "xxx", "Lexer::trim 'xxx' -> 'xxx'");
    assert_eq!(Lexer::trim_with("xxx", " \t"), "xxx", "Lexer::trim 'xxx' -> 'xxx'");
    assert_eq!(Lexer::trim("  \t xxx \t  "), "\t xxx \t", r"Lexer::trim '  \t xxx \t  ' -> '\t xxx \t'");
    assert_eq!(Lexer::trim_with("  \t xxx \t  ", " \t"), "xxx", "Lexer::trim '  \\t xxx \\t  ' -> 'xxx'");
}
