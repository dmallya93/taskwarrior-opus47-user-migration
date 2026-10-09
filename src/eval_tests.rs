//! Unit tests for [`super::Eval`]: the complete port of Taskwarrior
//! `test/eval_test.cpp` (52 assertions, assertion-count checked through
//! [`UnitTest`]), plus direct tests of the debug output, precedence and
//! associativity, unary operators, error paths, the resolver API and the
//! operator tables.

use super::*;
use crate::test_util::env_lock;
use std::collections::BTreeMap;
use std::fmt::Debug;

/// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
struct UnitTest {
    counter: usize,
    failures: Vec<String>,
}

impl UnitTest {
    fn new() -> Self {
        UnitTest { counter: 0, failures: Vec::new() }
    }

    /// C++ `UnitTest::ok`.
    fn ok(&mut self, expression: bool, name: &str) {
        self.counter += 1;
        if !expression {
            self.failures.push(format!("not ok {} - {}", self.counter, name));
        }
    }

    /// C++ `UnitTest::notok`.
    fn notok(&mut self, expression: bool, name: &str) {
        self.ok(!expression, name);
    }

    /// C++ `UnitTest::is` (exact comparison overloads).
    fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: &str) {
        self.counter += 1;
        if actual != expected {
            self.failures.push(format!(
                "not ok {} - {}\n#   expected: {:?}\n#        got: {:?}",
                self.counter, name, expected, actual
            ));
        }
    }

    /// C++ `UnitTest::is (double actual, double expected, double tolerance, ...)`.
    fn is_tol(&mut self, actual: f64, expected: f64, tolerance: f64, name: &str) {
        self.counter += 1;
        if (actual - expected).abs() > tolerance {
            self.failures.push(format!("not ok {} - {}", self.counter, name));
        }
    }

    /// Asserts that every assertion passed and that `planned` ran.
    fn check(&self, planned: usize) {
        assert!(
            self.failures.is_empty(),
            "{} of {} assertions failed:\n{}",
            self.failures.len(),
            self.counter,
            self.failures.join("\n")
        );
        assert_eq!(self.counter, planned, "number of assertions executed");
    }
}

////////////////////////////////////////////////////////////////////////////////
// A few hard-coded symbols (C++ `get`).
fn get(name: &str) -> Option<Variant> {
    if name == "x" {
        Some(Variant::from_bool(true))
    } else {
        None
    }
}

/// Port of `test/eval_test.cpp`.
#[test]
#[allow(clippy::approx_constant)] // The C++ literal 3.141592 is kept.
fn eval_test_cpp() {
    // The Lexer's date/duration recognition reads the time zone.
    let _lock = env_lock();
    let mut t = UnitTest::new();

    // Test the source independently.
    t.notok(get("-").is_some(), "true <-- get(-)");

    let v = get("x");
    t.ok(v.is_some(), "true <-- get(x)");
    let v = v.unwrap_or_default();
    t.is(v.type_(), VariantType::Boolean, "get(x) --> boolean");
    t.is(v.get_bool(), true, "get(x) --> true");

    let e = Eval::new();
    let mut r = FnResolver::new(get);
    let result = e.evaluate_postfix_expression("x", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix 'x' --> boolean");
    t.is(result.get_bool(), true, "postfix 'x' --> true");

    let result = e.evaluate_postfix_expression("pi", &mut r).unwrap();
    t.is(result.type_(), VariantType::Real, "postfix 'pi' --> real");
    t.is_tol(result.get_real(), 3.141592, 0.00001, "postfix 'pi' --> 3.14159265");

    let result = e.evaluate_postfix_expression("foo", &mut r).unwrap();
    t.is(result.type_(), VariantType::String, "postfix 'foo' --> string");
    t.is(result.get_string(), "foo", "postfix 'foo' --> 'foo'");

    // Simple infix arithmetic.
    let result = e.evaluate_infix_expression("1+2", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "infix '1 + 2' --> integer");
    t.is(result.get_integer(), 3, "infix '1 + 2' --> 3");

    // Simple postfix arithmetic.
    let result = e.evaluate_postfix_expression("1 2 +", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "postfix '1 2 +' --> integer");
    t.is(result.get_integer(), 3, "postfix '1 2 +' --> 3");

    let result = e.evaluate_postfix_expression("1 2 -", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "postfix '1 2 -' --> integer");
    t.is(result.get_integer(), -1, "postfix '1 2 -' --> -1");

    let result = e.evaluate_postfix_expression("2 3 *", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "postfix '2 3 *' --> integer");
    t.is(result.get_integer(), 6, "postfix '2 3 *' --> 6");

    let result = e.evaluate_postfix_expression("5 2 /", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "postfix '5 2 /' --> integer");
    t.is(result.get_integer(), 2, "postfix '5 2 /' --> 2");

    let result = e.evaluate_postfix_expression("5 2 /", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "postfix '5 2 *' --> integer");
    t.is(result.get_integer(), 2, "postfix '5 2 *' --> 2");

    // Simple postfix unary operator.
    let result = e.evaluate_postfix_expression("0 !", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix '0 !' --> boolean");
    t.is(result.get_bool(), true, "postfix '0 !' --> true");

    let result = e.evaluate_postfix_expression("1 !", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix '1 !' --> boolean");
    t.is(result.get_bool(), false, "postfix '1 !' --> false");

    // Type promotion simple postfix arithmetic.
    let result = e.evaluate_postfix_expression("1 2.3 +", &mut r).unwrap();
    t.is(result.type_(), VariantType::Real, "postfix '1 2.3 +' --> real");
    t.is(result.get_real(), 3.3, "postfix '1 2.3 +' --> 3.3");

    let result = e.evaluate_postfix_expression("5 2.0 /", &mut r).unwrap();
    t.is(result.type_(), VariantType::Real, "postfix '5 2.0 /' --> integer");
    t.is(result.get_real(), 2.5, "postfix '5 2.0 /' --> 2.5");

    // Simple logic.
    let result = e.evaluate_postfix_expression("0 0 ||", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix '0 0 ||' --> boolean");
    t.is(result.get_bool(), false, "postfix '0 0 ||' --> false");

    let result = e.evaluate_postfix_expression("0 1 ||", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix '0 1 ||' --> boolean");
    t.is(result.get_bool(), true, "postfix '0 1 ||' --> true");

    let result = e.evaluate_postfix_expression("1 0 ||", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix '1 0 ||' --> boolean");
    t.is(result.get_bool(), true, "postfix '1 0 ||' --> true");

    let result = e.evaluate_postfix_expression("1 1 ||", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "postfix '1 1 ||' --> boolean");
    t.is(result.get_bool(), true, "postfix '1 1 ||' --> true");

    let result = e.evaluate_infix_expression("2*3+1", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "infix '2*3+1' --> integer");
    t.is(result.get_integer(), 7, "infix '2*3+1' --> 7");

    // TW-1254 - Unary minus support.
    let result = e.evaluate_infix_expression("2- -3", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "infix '2- -3' --> integer");
    t.is(result.get_integer(), 5, "infix '2- -3' --> 5");

    // e.debug ();
    let result = e.evaluate_infix_expression("!false", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "infix '!false' --> boolean");
    t.is(result.get_bool(), true, "infix '!false' --> true");

    let result = e.evaluate_infix_expression("!true", &mut r).unwrap();
    t.is(result.type_(), VariantType::Boolean, "infix '!true' --> boolean");
    t.is(result.get_bool(), false, "infix '!true' --> false");

    // _neg_
    let result = e.evaluate_infix_expression("- 1", &mut r).unwrap();
    t.is(result.type_(), VariantType::Integer, "infix '- 1' --> integer");
    t.is(result.get_integer(), -1, "infix '- 1' --> -1");

    let result = e.evaluate_infix_expression("- 1.2", &mut r).unwrap();
    t.is(result.type_(), VariantType::Real, "infix '- 1.2' --> real");
    t.is(result.get_real(), -1.2, "infix '- 1.2' --> -1.2");

    let result = e.evaluate_infix_expression("- 2days", &mut r).unwrap();
    t.is(result.type_(), VariantType::Duration, "infix '- 2days' --> duration");
    t.is(result.get_duration(), -86400 * 2, "infix '- 2days' --> -86400 * 2");

    t.check(52);
}

////////////////////////////////////////////////////////////////////////////////
// Helpers for the direct tests.

fn infix(expression: &str) -> Result<Variant> {
    Eval::new().evaluate_infix_expression(expression, &mut NoDom::new())
}

fn postfix(expression: &str) -> Result<Variant> {
    Eval::new().evaluate_postfix_expression(expression, &mut NoDom::new())
}

fn message(result: Result<Variant>) -> String {
    match result {
        Err(TwError::Message(m)) => m,
        other => panic!("expected a message error, got {other:?}"),
    }
}

/// A [`VariantTask`] with a fixed tag set and annotations.
struct TagTask {
    tags: Vec<&'static str>,
    annotations: BTreeMap<String, String>,
}

impl VariantTask for TagTask {
    fn get_annotations(&self) -> BTreeMap<String, String> {
        self.annotations.clone()
    }

    fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag)
    }
}

fn tag_task(tags: &[&'static str]) -> TagTask {
    TagTask { tags: tags.to_vec(), annotations: BTreeMap::new() }
}

/// The C++ `Lexer` token vector for `expression`.
fn lex(expression: &str) -> Vec<Token> {
    Eval::tokenize(expression)
}

fn texts(tokens: &[Token]) -> Vec<&str> {
    tokens.iter().map(|t| t.0.as_str()).collect()
}

////////////////////////////////////////////////////////////////////////////////
// Operator tables.

#[test]
fn operator_tables() {
    let all = Eval::get_operators();
    assert_eq!(all.len(), NUM_OPERATORS);
    assert_eq!(
        all,
        [
            "^", "!", "_neg_", "_pos_", "_hastag_", "_notag_", "*", "/", "%", "+", "-", "<=", ">=", ">", "<", "=",
            "==", "!=", "!==", "~", "!~", "and", "or", "xor", "(", ")"
        ]
    );
    assert_eq!(
        Eval::get_binary_operators(),
        [
            "^", "_hastag_", "_notag_", "*", "/", "%", "+", "-", "<=", ">=", ">", "<", "=", "==", "!=", "!==", "~",
            "!~", "and", "or", "xor"
        ]
    );

    let e = Eval::new();
    assert_eq!(e.identify_operator("^"), Some(('b', 16, 'r')));
    assert_eq!(e.identify_operator("_neg_"), Some(('u', 15, 'r')));
    assert_eq!(e.identify_operator("_notag_"), Some(('b', 14, 'l')));
    assert_eq!(e.identify_operator("!=="), Some(('b', 9, 'l')));
    assert_eq!(e.identify_operator("xor"), Some(('b', 3, 'l')));
    assert_eq!(e.identify_operator("("), Some(('c', 0, '_')));
    assert_eq!(e.identify_operator("&&"), None);
    assert_eq!(e.identify_operator("-"), Some(('b', 12, 'l')));
}

////////////////////////////////////////////////////////////////////////////////
// Precedence, associativity, parentheses.

#[test]
fn precedence_and_associativity() {
    let _lock = env_lock();
    assert_eq!(infix("2+3*4").unwrap().get_integer(), 14);
    assert_eq!(infix("(2+3)*4").unwrap().get_integer(), 20);
    // Left associative.
    assert_eq!(infix("10 - 4 - 3").unwrap().get_integer(), 3);
    assert_eq!(infix("8 % 5 % 2").unwrap().get_integer(), 1);
    // Right associative exponent: 2^(3^2).
    assert_eq!(infix("2^3^2").unwrap().get_integer(), 512);
    assert_eq!(infix("(2^3)^2").unwrap().get_integer(), 64);
    // Modulus binds like multiplication.
    assert_eq!(infix("1+7%4").unwrap().get_integer(), 4);
    // Comparison below arithmetic, logic below comparison.
    assert!(infix("1+1 == 2").unwrap().get_bool());
    assert!(infix("1 < 2 and 3 > 2").unwrap().get_bool());
    assert!(!infix("1 > 2 or 3 < 2").unwrap().get_bool());
    assert!(infix("1 < 2 xor 3 < 2").unwrap().get_bool());
    assert!(infix("2 <= 2 and 2 >= 3 or true").unwrap().get_bool());
    assert!(infix("1 != 2").unwrap().get_bool());
    assert!(infix("1 !== 2").unwrap().get_bool());
    assert!(infix("2 = 2").unwrap().get_bool());

    // The postfix form produced by the shunting yard.
    let e = Eval::new();
    let mut tokens = lex("1 + 2 * 3 ^ 2 ^ 2 - 4");
    e.infix_parse(&mut tokens, &mut NoDom::new());
    e.infix_to_postfix(&mut tokens).unwrap();
    assert_eq!(texts(&tokens), ["1", "2", "3", "2", "2", "^", "^", "*", "+", "4", "-"]);

    let mut tokens = lex("( 1 + 2 ) * 3");
    e.infix_to_postfix(&mut tokens).unwrap();
    assert_eq!(texts(&tokens), ["1", "2", "+", "3", "*"]);

    // Single token short circuit, even for an operator.
    let mut tokens = lex("(");
    e.infix_to_postfix(&mut tokens).unwrap();
    assert_eq!(texts(&tokens), ["("]);

    // Tokens that are not in the operator table are operands to the shunting yard.
    let mut tokens = lex("1 && 2");
    e.infix_to_postfix(&mut tokens).unwrap();
    assert_eq!(texts(&tokens), ["1", "&&", "2"]);
}

////////////////////////////////////////////////////////////////////////////////
// Unary operators.

#[test]
fn unary_operators() {
    let _lock = env_lock();
    assert_eq!(infix("-(2+3)").unwrap().get_integer(), -5);
    assert_eq!(infix("+3").unwrap().get_integer(), 3);
    assert_eq!(infix("2 * -3").unwrap().get_integer(), -6);
    assert_eq!(infix("2 - +3").unwrap().get_integer(), -1);
    // Unary minus binds tighter than exponent... in the table (15 < 16), so
    // -2^2 is -(2^2).
    assert_eq!(infix("-2^2").unwrap().get_integer(), -4);
    assert!(!infix("!1").unwrap().get_bool());
    assert!(infix("!0 and true").unwrap().get_bool());

    // parseUnary rewrites the operators in place.
    let e = Eval::new();
    let mut tokens = lex("- 1 + + 2 - - 3");
    e.infix_parse(&mut tokens, &mut NoDom::new());
    assert_eq!(texts(&tokens), ["_neg_", "1", "+", "_pos_", "2", "-", "_neg_", "3"]);
    e.infix_to_postfix(&mut tokens).unwrap();
    assert_eq!(texts(&tokens), ["1", "_neg_", "2", "_pos_", "+", "3", "_neg_", "-"]);

    // Postfix spellings.
    assert_eq!(postfix("4 _neg_").unwrap().get_integer(), -4);
    assert_eq!(postfix("4 _pos_").unwrap().get_integer(), 4);
}

////////////////////////////////////////////////////////////////////////////////
// Errors and ambiguity.

#[test]
fn errors() {
    let _lock = env_lock();
    assert_eq!(message(postfix("")), "No expression to evaluate.");
    assert_eq!(message(infix("")), "No expression to evaluate.");
    assert_eq!(message(postfix("!")), "The expression could not be evaluated.");
    assert_eq!(message(postfix("_neg_")), "The expression could not be evaluated.");
    assert_eq!(message(postfix("1 +")), "The expression could not be evaluated.");
    assert_eq!(message(postfix("1 2")), "The value is not an expression.");
    assert_eq!(message(infix("(1+2")), "Mismatched parentheses in expression");
    assert_eq!(message(infix("1+2)")), "Mismatched parentheses in expression");
    assert_eq!(message(infix("1 2")), "The value is not an expression.");
    // Without a current task the task operators are unsupported.
    assert_eq!(message(postfix("a b ~")), "Unsupported operator '~'.");
    assert_eq!(message(postfix("a b !~")), "Unsupported operator '!~'.");
    assert_eq!(message(postfix("a b _hastag_")), "Unsupported operator '_hastag_'.");
    assert_eq!(message(postfix("a b _notag_")), "Unsupported operator '_notag_'.");
    // Not in the table: '(' reaches the binary evaluator.
    assert_eq!(message(postfix("1 2 (")), "Unsupported operator '('.");
    // `_pos_` is a NOP even on an empty stack, leaving nothing to return.
    assert_eq!(message(postfix("_pos_")), "The value is not an expression.");
}

#[test]
fn ambiguity_minus_is_binary_after_operand() {
    let _lock = env_lock();
    // "2 - 3" is binary subtraction, "2 - -3" has one unary minus. ("2-3"
    // without spaces is a Lexer set token, not an expression.)
    assert_eq!(infix("2 - 3").unwrap().get_integer(), -1);
    assert_eq!(infix("2-3").unwrap().get_string(), "2-3");
    let e = Eval::new();
    let mut tokens = lex("2 - -3");
    e.infix_parse(&mut tokens, &mut NoDom::new());
    assert_eq!(texts(&tokens), ["2", "-", "_neg_", "3"]);
}

////////////////////////////////////////////////////////////////////////////////
// Resolver.

#[test]
fn resolver_lookup_order_and_dom_types() {
    let _lock = env_lock();
    let e = Eval::new();
    let mut calls: Vec<String> = Vec::new();
    {
        let mut r = FnResolver::new(|name: &str| {
            calls.push(name.to_string());
            match name {
                "pi" => Some(Variant::from_int(4)),
                "due" => Some(Variant::from_int(10)),
                _ => None,
            }
        });

        // Named constants win over the added source, which is not consulted.
        let v = e.evaluate_postfix_expression("pi", &mut r).unwrap();
        assert_eq!(v.type_(), VariantType::Real);

        let v = e.evaluate_infix_expression("due + 1", &mut r).unwrap();
        assert_eq!(v.get_integer(), 11);

        // An unresolved identifier is a string.
        let v = e.evaluate_infix_expression("nosuch", &mut r).unwrap();
        assert_eq!(v.get_string(), "nosuch");
    }
    // infixParse consults the sources for every primitive (numbers too), then
    // the postfix evaluator for identifiers only.
    assert_eq!(calls, ["due", "1", "due", "nosuch", "nosuch"]);
}

#[test]
fn dom_source_sets_the_source() {
    let found = dom_source("rc.foo", |name| {
        assert_eq!(name, "rc.foo");
        Some(Variant::from_string("bar"))
    })
    .unwrap();
    assert_eq!(found.get_string(), "bar");
    assert_eq!(found.source(), "rc.foo");
    assert!(dom_source("nope", |_| None).is_none());
}

#[test]
fn task_operators_use_the_current_task() {
    let _lock = env_lock();
    let e = Eval::new();
    let task = tag_task(&["home", "next"]);
    let mut r = FnResolver::with_task(get, &task);

    assert!(e.evaluate_postfix_expression("tags home _hastag_", &mut r).unwrap().get_bool());
    assert!(!e.evaluate_postfix_expression("tags work _hastag_", &mut r).unwrap().get_bool());
    assert!(e.evaluate_postfix_expression("tags work _notag_", &mut r).unwrap().get_bool());
    assert!(!e.evaluate_postfix_expression("tags next _notag_", &mut r).unwrap().get_bool());
    // Infix: _hastag_ binds tighter than `and`.
    assert!(e.evaluate_infix_expression("tags _hastag_ home and tags _notag_ work", &mut r).unwrap().get_bool());

    // Regex match through the current task.
    assert!(e.evaluate_infix_expression("'foobar' ~ 'oba'", &mut r).unwrap().get_bool());
    assert!(!e.evaluate_infix_expression("'foobar' !~ 'oba'", &mut r).unwrap().get_bool());
}

#[test]
fn compiled_expression_in_a_task_loop() {
    let _lock = env_lock();
    let precompiled = lex("tags _hastag_ home or tags _hastag_ next");
    let mut e = Eval::new();
    e.compile_expression(&precompiled, &mut NoDom::new()).unwrap();
    assert_eq!(texts(&e._compiled), ["tags", "home", "_hastag_", "tags", "next", "_hastag_", "or"]);

    let tasks = [tag_task(&["home"]), tag_task(&["work"]), tag_task(&[]), tag_task(&["next", "x"])];
    let mut selected = Vec::new();
    for (index, task) in tasks.iter().enumerate() {
        let mut r = FnResolver::with_task(|_: &str| None, task);
        if e.evaluate_compiled_expression(&mut r).unwrap().get_bool() {
            selected.push(index);
        }
    }
    assert_eq!(selected, [0, 3]);

    // Compilation errors propagate.
    let mut bad = Eval::new();
    assert!(bad.compile_expression(&lex("( 1"), &mut NoDom::new()).is_err());
}

////////////////////////////////////////////////////////////////////////////////
// Debug output.

/// `Color ("<fg> on gray6").colorize (text)` for a 256-color index `fg`.
fn col(fg: u32, text: &str) -> String {
    format!("\x1b[38;5;{fg};48;5;238m{text}\x1b[0m")
}

#[test]
fn dump_colours_every_token_type() {
    let e = Eval::new();
    let tokens: Vec<Token> = vec![
        ("+".into(), Type::op),
        ("1".into(), Type::number),
        ("0x1".into(), Type::hex),
        ("'s'".into(), Type::string),
        ("due".into(), Type::dom),
        ("foo".into(), Type::identifier),
        ("2024-01-01".into(), Type::date),
        ("2d".into(), Type::duration),
        ("w".into(), Type::word),
    ];
    let expected = [
        col(246, "+"),
        col(214, "1"),
        col(127, "0x1"),
        col(226, "'s'"),
        col(45, "due"),
        col(39, "foo"),
        col(82, "2024-01-01"),
        col(215, "2d"),
        col(16, "w"),
    ]
    .join(" ");
    assert_eq!(e.dump(&tokens).unwrap(), expected);
    assert_eq!(e.dump(&[]).unwrap(), "");
}

#[test]
fn debug_messages_infix() {
    let _lock = env_lock();
    let mut e = Eval::new();
    e.debug(true);
    let mut r = FnResolver::new(get);
    let v = e.evaluate_infix_expression("x and -2 < 1 + foo", &mut r).unwrap();
    assert!(v.get_bool());

    let id = |s: &str| col(39, s);
    let op = |s: &str| col(246, s);
    let num = |s: &str| col(214, s);
    let expected = vec![
        format!(
            "\x1b[1;37;42mFILTER\x1b[0m Infix        {} {} {} {} {} {} {} {}",
            id("x"),
            op("and"),
            op("-"),
            num("2"),
            op("<"),
            num("1"),
            op("+"),
            id("foo")
        ),
        format!(
            "\x1b[1;37;42mFILTER\x1b[0m Infix parsed {} {} {} {} {} {} {} {}",
            id("x"),
            op("and"),
            op("_neg_"),
            num("2"),
            op("<"),
            num("1"),
            op("+"),
            id("foo")
        ),
        format!(
            "\x1b[1;37;42mFILTER\x1b[0m Postfix      {} {} {} {} {} {} {} {}",
            id("x"),
            num("2"),
            op("_neg_"),
            num("1"),
            id("foo"),
            op("+"),
            op("<"),
            op("and")
        ),
        "Eval identifier source 'x' → ↑'true'".to_string(),
        "Eval literal number ↑'2'".to_string(),
        "Eval _neg_ ↓'2' → ↑'-2'".to_string(),
        "Eval literal number ↑'1'".to_string(),
        "Eval identifier source failed 'foo'".to_string(),
        "Eval ↓'1' + ↓'foo' → ↑'1foo'".to_string(),
        "Eval ↓'-2' < ↓'1foo' → ↑'true'".to_string(),
        "Eval ↓'true' and ↓'true' → ↑'true'".to_string(),
    ];
    assert_eq!(r.debug_messages, expected);
}

#[test]
fn debug_messages_postfix_literals() {
    let _lock = env_lock();
    let mut e = Eval::new();
    e.debug(true);
    let mut r = NoDom::new();
    e.evaluate_postfix_expression("1.5 'a' ! _pos_ ==", &mut r).unwrap();
    assert_eq!(
        r.debug_messages,
        [
            format!(
                "\x1b[1;37;42mFILTER\x1b[0m Postfix      {} {} {} {} {}",
                col(214, "1.5"),
                col(226, "'a'"),
                col(246, "!"),
                col(246, "_pos_"),
                col(246, "==")
            ),
            "Eval literal decimal ↑'1.5'".to_string(),
            "Eval literal string ↑''a''".to_string(),
            "Eval ! ↓''a'' → ↑'false'".to_string(),
            "[2] eval op _pos_ NOP".to_string(),
            "Eval ↓'1.5' == ↓'false' → ↑'false'".to_string(),
        ]
    );

    // Turning debug off silences everything.
    e.debug(false);
    let mut r = NoDom::new();
    e.evaluate_infix_expression("1+2", &mut r).unwrap();
    assert!(r.debug_messages.is_empty());
}

#[test]
fn debug_messages_compile() {
    let _lock = env_lock();
    let mut e = Eval::new();
    e.debug(true);
    let mut r = NoDom::new();
    e.compile_expression(&lex("1 - 2"), &mut r).unwrap();
    assert_eq!(
        r.debug_messages,
        [
            format!("\x1b[1;37;42mFILTER\x1b[0m Infix        {} {} {}", col(214, "1"), col(246, "-"), col(214, "2")),
            format!("\x1b[1;37;42mFILTER\x1b[0m Infix parsed {} {} {}", col(214, "1"), col(246, "-"), col(214, "2")),
            format!("\x1b[1;37;42mFILTER\x1b[0m Postfix      {} {} {}", col(214, "1"), col(214, "2"), col(246, "-")),
        ]
    );

    let mut r = NoDom::new();
    assert_eq!(e.evaluate_compiled_expression(&mut r).unwrap().get_integer(), -1);
    assert_eq!(r.debug_messages.len(), 3);
    assert_eq!(r.debug_messages[2], "Eval ↓'1' - ↓'2' → ↑'-1'");
}

#[test]
fn literal_dates_and_durations() {
    let _lock = env_lock();
    let v = infix("1d + 1d").unwrap();
    assert_eq!(v.type_(), VariantType::Duration);
    assert_eq!(v.get_duration(), 2 * 86400);
    let v = postfix("2024-01-01T00:00:00Z").unwrap();
    assert_eq!(v.type_(), VariantType::Date);
    assert_eq!(v.get_date(), 1704067200);
}
