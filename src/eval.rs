//! Rust translation of `src/Eval.h` / `src/Eval.cpp`.
//!
//! [`Eval`] is the infix/postfix expression evaluator: a recursive-descent
//! syntax check that also rewrites unary `-`/`+` into `_neg_`/`_pos_`
//! (`infix_parse`), Dijkstra's shunting yard (`infix_to_postfix`)
//! driven by the C++ `operators[]` table ([`OPERATORS`]), and a postfix stack
//! evaluator over [`Variant`] (`evaluate_postfix_stack`).
//!
//! # Resolver and debug sink (milestone Design Decision 1, option 1)
//!
//! The C++ `Eval` stores a list of source callbacks (`_sources`, filled by the
//! constructor with `namedConstants` and by `addSource`), reads
//! `Context::getContext ().currentTask` for `~`, `!~`, `_hastag_` and
//! `_notag_`, and writes its debug output to `Context::getContext ().debug`.
//! The Rust `Eval` holds none of that state. Every method that may resolve an
//! identifier or emit debug output takes a caller-provided
//! `&mut dyn` [`DomResolver`], which bundles the three things C++ took from
//! globals:
//!
//! * [`DomResolver::resolve`]: the added sources (`addSource (fn)`), tried
//!   after the built-in named constants (`true`, `false`, `pi`), exactly as
//!   the C++ `_sources` vector starts with `namedConstants`;
//! * [`DomResolver::current_task`]: `Context::getContext ().currentTask`;
//! * [`DomResolver::debug`]: `Context::getContext ().debug (...)`, which
//!   receives byte-identical message texts (including the `\033[1;37;42m`
//!   `FILTER` prefixes).
//!
//! Because [`Eval`] owns only plain data (`_debug`, `_compiled`), a filter can
//! compile once and then, inside a loop over thousands of tasks, build a cheap
//! per-task resolver (for example a struct holding `&Context` / `&mut Context`
//! and `&Task`) and call [`Eval::evaluate_compiled_expression`] with it. No
//! borrow outlives a single call, so there are no lifetime conflicts with the
//! loop variable.
//!
//! C++ call patterns map as follows:
//!
//! | C++ | Rust |
//! |-----|------|
//! | `Eval e;` | [`Eval::new`] (named constants are built in) |
//! | `e.addSource (get);` | implement [`DomResolver::resolve`] (or use [`FnResolver`]) |
//! | `e.addSource (domSource);` | [`crate::dom::ContextResolver`] (or any resolver whose `resolve` calls [`dom_source`] with a DOM lookup) |
//! | no added source (`calc`) | [`NoDom`] |
//! | `e.evaluateInfixExpression (s, v);` | `let v = e.evaluate_infix_expression (s, &mut resolver)?;` |
//! | `e.compileExpression (tokens);` | `e.compile_expression (&tokens, &mut resolver)?;` |
//! | `e.evaluateCompiledExpression (v);` | `let v = e.evaluate_compiled_expression (&mut resolver)?;` |
//!
//! Note that the C++ `parsePrimitive` already consults the sources during
//! `compileExpression`, when `Filter` has not yet set a current task; the
//! resolver passed to [`Eval::compile_expression`] should therefore resolve
//! as C++ `domSource` does with a null `currentTask`.
//!
//! # Errors
//! C++ `throw std::string (...)` becomes `Err (TwError::Message (...))` with
//! the identical text; errors raised by the [`Variant`] operators and casts
//! propagate unchanged. The C++ out-parameter `Variant& v` becomes the
//! `Ok` value (C++ only assigns it on success, at the very end).

use std::collections::BTreeMap;

use crate::error::{Result, TwError};
use crate::lexer::{Lexer, Type};
use crate::shared::color::Color;
use crate::shared::format::format;
use crate::variant::{Type as VariantType, Variant, VariantTask};

/// One row of the C++ file-static `operators[]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operator {
    /// `std::string op`
    pub op: &'static str,
    /// `int precedence`
    pub precedence: u32,
    /// `char type`: `b`=binary, `u`=unary, `c`=circumfix.
    pub type_: char,
    /// `char associativity`: `l`=left, `r`=right, `_`=?
    pub associativity: char,
}

const fn opr(op: &'static str, precedence: u32, type_: char, associativity: char) -> Operator {
    Operator {
        op,
        precedence,
        type_,
        associativity,
    }
}

////////////////////////////////////////////////////////////////////////////////
/// Supported operators, borrowed from C++, particularly the precedence.
/// Note: table is sorted by length of operator string, so searches match
///       longest first.
pub const OPERATORS: [Operator; 26] = [
    // Operator   Precedence  Type  Associativity
    opr("^", 16, 'b', 'r'), // Exponent
    //
    opr("!", 15, 'u', 'r'),     // Unary not
    opr("_neg_", 15, 'u', 'r'), // Unary minus
    opr("_pos_", 15, 'u', 'r'), // Unary plus
    //
    opr("_hastag_", 14, 'b', 'l'), // +tag  [Pseudo-op]
    opr("_notag_", 14, 'b', 'l'),  // -tag  [Pseudo-op]
    //
    opr("*", 13, 'b', 'l'), // Multiplication
    opr("/", 13, 'b', 'l'), // Division
    opr("%", 13, 'b', 'l'), // Modulus
    //
    opr("+", 12, 'b', 'l'), // Addition
    opr("-", 12, 'b', 'l'), // Subtraction
    //
    opr("<=", 10, 'b', 'l'), // Less than or equal
    opr(">=", 10, 'b', 'l'), // Greater than or equal
    opr(">", 10, 'b', 'l'),  // Greater than
    opr("<", 10, 'b', 'l'),  // Less than
    //
    opr("=", 9, 'b', 'l'),   // Equal (partial)
    opr("==", 9, 'b', 'l'),  // Equal (exact)
    opr("!=", 9, 'b', 'l'),  // Inequal (partial)
    opr("!==", 9, 'b', 'l'), // Inequal (exact)
    //
    opr("~", 8, 'b', 'l'),  // Regex match
    opr("!~", 8, 'b', 'l'), // Regex non-match
    //
    opr("and", 5, 'b', 'l'), // Conjunction
    opr("or", 4, 'b', 'l'),  // Disjunction
    opr("xor", 3, 'b', 'l'), // Disjunction
    //
    opr("(", 0, 'c', '_'), // Precedence start
    opr(")", 0, 'c', '_'), // Precedence end
];

/// C++ `#define NUM_OPERATORS (sizeof (operators) / sizeof (operators[0]))`.
pub const NUM_OPERATORS: usize = OPERATORS.len();

/// The C++ token vector element `std::pair<std::string, Lexer::Type>`.
pub type Token = (String, Type);

////////////////////////////////////////////////////////////////////////////////
/// Everything `Eval` took from globals in C++: the added identifier sources,
/// the current task and the debug sink. See the module documentation.
pub trait DomResolver {
    /// The added sources (`addSource`): `Some (value)` when `name` is known,
    /// as a C++ source returning `true` after assigning `value`.
    ///
    /// `Err` is a C++ exception thrown from inside the source (for example
    /// `getDOM` loading a referenced task from TDB2, or computing urgency); as
    /// in C++ it aborts the evaluation (and the syntax check) unchanged.
    fn resolve(&mut self, name: &str) -> Result<Option<Variant>>;

    /// C++ `Context::getContext ().currentTask` (null -> `None`), used by
    /// `~`, `!~`, `_hastag_` and `_notag_`.
    fn current_task(&self) -> Option<&dyn VariantTask>;

    /// C++ `Context::getContext ().debug (message)`.
    fn debug(&mut self, message: &str);
}

/// A resolver without DOM access and without a current task, collecting the
/// debug messages as C++ `Context::debug` does (empties dropped). This is the
/// `calc` binary's setup (its `get` source never resolves anything).
#[derive(Debug, Clone, Default)]
pub struct NoDom {
    /// C++ `Context::debugMessages`.
    pub debug_messages: Vec<String>,
}

impl NoDom {
    /// An empty collector.
    pub fn new() -> Self {
        NoDom::default()
    }
}

impl DomResolver for NoDom {
    fn resolve(&mut self, _name: &str) -> Result<Option<Variant>> {
        Ok(None)
    }

    fn current_task(&self) -> Option<&dyn VariantTask> {
        None
    }

    fn debug(&mut self, message: &str) {
        if !message.is_empty() {
            self.debug_messages.push(message.to_string());
        }
    }
}

/// Adapts a closure (the C++ `bool (*) (const std::string&, Variant&)`
/// source passed to `addSource`) and an optional current task into a
/// [`DomResolver`], collecting debug messages like [`NoDom`].
pub struct FnResolver<'t, F: FnMut(&str) -> Option<Variant>> {
    /// The added source.
    pub source: F,
    /// The current task.
    pub task: Option<&'t dyn VariantTask>,
    /// C++ `Context::debugMessages`.
    pub debug_messages: Vec<String>,
}

impl<'t, F: FnMut(&str) -> Option<Variant>> FnResolver<'t, F> {
    /// A resolver over `source` with no current task.
    pub fn new(source: F) -> Self {
        FnResolver {
            source,
            task: None,
            debug_messages: Vec::new(),
        }
    }

    /// A resolver over `source` with `task` as the current task.
    pub fn with_task(source: F, task: &'t dyn VariantTask) -> Self {
        FnResolver {
            source,
            task: Some(task),
            debug_messages: Vec::new(),
        }
    }
}

impl<F: FnMut(&str) -> Option<Variant>> DomResolver for FnResolver<'_, F> {
    fn resolve(&mut self, name: &str) -> Result<Option<Variant>> {
        Ok((self.source)(name))
    }

    fn current_task(&self) -> Option<&dyn VariantTask> {
        self.task
    }

    fn debug(&mut self, message: &str) {
        if !message.is_empty() {
            self.debug_messages.push(message.to_string());
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// Built-in support for some named constants (C++ file-static
/// `namedConstants`).
fn named_constants(name: &str) -> Option<Variant> {
    let value = if name == "true" {
        Variant::from_bool(true)
    } else if name == "false" {
        Variant::from_bool(false)
    } else if name == "pi" {
        Variant::from_real(3.14159165)
    } else {
        return None;
    };

    Some(value)
}

////////////////////////////////////////////////////////////////////////////////
/// Support for evaluating DOM references (C++ `domSource`, added with
/// `e.addSource (domSource)`).
///
/// Deviation: C++ calls `getDOM (identifier, Context::getContext
/// ().currentTask, value)` directly. The DOM lookup needs the context and the
/// current task explicitly, so it is passed in as `get_dom` (C++ `getDOM`
/// returning `true` -> `Some`). The Context-backed resolver,
/// [`crate::dom::ContextResolver`], performs the same steps itself, because
/// its `getDOM` is fallible. As in C++, a found value gets `identifier` as
/// its source.
pub fn dom_source(
    identifier: &str,
    get_dom: impl FnOnce(&str) -> Option<Variant>,
) -> Option<Variant> {
    if let Some(mut value) = get_dom(identifier) {
        value.set_source(identifier);
        return Some(value);
    }

    None
}

/// C++ `class Eval`.
#[derive(Debug, Clone, Default)]
pub struct Eval {
    /// C++ `bool _debug`.
    _debug: bool,
    /// C++ `std::vector<std::pair<std::string, Lexer::Type>> _compiled`.
    _compiled: Vec<Token>,
}

/// `throw std::string (message)`.
fn err<T>(message: &str) -> Result<T> {
    Err(TwError::Message(message.to_string()))
}

impl Eval {
    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Eval::Eval () { addSource (namedConstants); }`: the named
    /// constants are always consulted first (see `Eval::source`).
    pub fn new() -> Self {
        Eval::default()
    }

    /// The C++ `for (const auto& source : _sources)` lookup: `namedConstants`
    /// first, then the added sources (the resolver); the first hit wins.
    fn source(name: &str, resolver: &mut dyn DomResolver) -> Result<Option<Variant>> {
        match named_constants(name) {
            Some(value) => Ok(Some(value)),
            None => resolver.resolve(name),
        }
    }

    /// Reduce `e` to a vector of tokens.
    fn tokenize(e: &str) -> Vec<Token> {
        let mut l = Lexer::new(e);
        let mut tokens = Vec::new();
        let mut token = String::new();
        let mut type_ = Type::word;
        while l.token(&mut token, &mut type_) {
            tokens.push((token.clone(), type_));
        }
        tokens
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Eval::evaluateInfixExpression`.
    pub fn evaluate_infix_expression(
        &self,
        e: &str,
        resolver: &mut dyn DomResolver,
    ) -> Result<Variant> {
        // Reduce e to a vector of tokens.
        let mut tokens = Self::tokenize(e);

        // Parse for syntax checking and operator replacement.
        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Infix        {}",
                self.dump(&tokens)?
            ));
        }
        self.infix_parse(&mut tokens, resolver)?;
        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Infix parsed {}",
                self.dump(&tokens)?
            ));
        }

        // Convert infix --> postfix.
        self.infix_to_postfix(&mut tokens)?;
        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Postfix      {}",
                self.dump(&tokens)?
            ));
        }

        // Call the postfix evaluator.
        self.evaluate_postfix_stack(&tokens, resolver)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Eval::evaluatePostfixExpression`.
    pub fn evaluate_postfix_expression(
        &self,
        e: &str,
        resolver: &mut dyn DomResolver,
    ) -> Result<Variant> {
        // Reduce e to a vector of tokens.
        let tokens = Self::tokenize(e);

        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Postfix      {}",
                self.dump(&tokens)?
            ));
        }

        // Call the postfix evaluator.
        self.evaluate_postfix_stack(&tokens, resolver)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Eval::compileExpression`. The resolver is consulted by the syntax
    /// check (C++ `parsePrimitive`) and receives the debug output.
    pub fn compile_expression(
        &mut self,
        precompiled: &[Token],
        resolver: &mut dyn DomResolver,
    ) -> Result<()> {
        let mut compiled = precompiled.to_vec();

        // Parse for syntax checking and operator replacement.
        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Infix        {}",
                self.dump(&compiled)?
            ));
        }
        self.infix_parse(&mut compiled, resolver)?;
        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Infix parsed {}",
                self.dump(&compiled)?
            ));
        }

        // Convert infix --> postfix.
        //
        // C++ works on `_compiled` in place, and `infixToPostfix` only assigns
        // its result at the end, so a throw leaves `_compiled` parsed but
        // still infix. Assigning before the conversion keeps that state.
        self._compiled = compiled.clone();
        self.infix_to_postfix(&mut compiled)?;
        self._compiled = compiled;
        if self._debug {
            resolver.debug(&format!(
                "\x1b[1;37;42mFILTER\x1b[0m Postfix      {}",
                self.dump(&self._compiled)?
            ));
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Eval::evaluateCompiledExpression`.
    pub fn evaluate_compiled_expression(&self, resolver: &mut dyn DomResolver) -> Result<Variant> {
        // Call the postfix evaluator.
        self.evaluate_postfix_stack(&self._compiled, resolver)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Eval::debug (bool)`.
    pub fn debug(&mut self, value: bool) {
        self._debug = value;
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `Eval::getOperators`.
    pub fn get_operators() -> Vec<String> {
        let mut all = Vec::with_capacity(NUM_OPERATORS);
        for opr in &OPERATORS {
            all.push(opr.op.to_string());
        }

        all
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `Eval::getBinaryOperators`.
    pub fn get_binary_operators() -> Vec<String> {
        let mut all = Vec::new();
        for opr in &OPERATORS {
            if opr.type_ == 'b' {
                all.push(opr.op.to_string());
            }
        }

        all
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::evaluatePostfixStack`.
    fn evaluate_postfix_stack(
        &self,
        tokens: &[Token],
        resolver: &mut dyn DomResolver,
    ) -> Result<Variant> {
        if tokens.is_empty() {
            return err("No expression to evaluate.");
        }

        // This is stack used by the postfix evaluator.
        let mut values: Vec<Variant> = Vec::with_capacity(tokens.len());

        for token in tokens {
            // Unary operators.
            if token.1 == Type::op && token.0 == "!" {
                if values.is_empty() {
                    return err("The expression could not be evaluated.");
                }

                let right = values.pop().unwrap_or_default();
                let result = Variant::from_bool(right.op_not());
                if self._debug {
                    resolver.debug(&format(
                        "Eval {1} ↓'{2}' → ↑'{3}'",
                        &[&token.0, &right.to_string(), &result.to_string()],
                    ));
                }
                values.push(result);
            } else if token.1 == Type::op && token.0 == "_neg_" {
                if values.is_empty() {
                    return err("The expression could not be evaluated.");
                }

                let right = values.pop().unwrap_or_default();

                let mut result = Variant::from_int(0);
                result.op_sub_assign(&right)?;

                if self._debug {
                    resolver.debug(&format(
                        "Eval {1} ↓'{2}' → ↑'{3}'",
                        &[&token.0, &right.to_string(), &result.to_string()],
                    ));
                }
                values.push(result);
            } else if token.1 == Type::op && token.0 == "_pos_" {
                // The _pos_ operator is a NOP.
                if self._debug {
                    resolver.debug(&format("[{1}] eval op {2} NOP", &[&values.len(), &token.0]));
                }
            }
            // Binary operators.
            else if token.1 == Type::op {
                if values.len() < 2 {
                    return err("The expression could not be evaluated.");
                }

                let right = values.pop().unwrap_or_default();
                let left = values.pop().unwrap_or_default();

                let context_task = resolver.current_task();

                // Ordering these by anticipation frequency of use is a good idea.
                let op = token.0.as_str();
                let result = if op == "and" {
                    Variant::from_bool(left.op_and(&right))
                } else if op == "or" {
                    Variant::from_bool(left.op_or(&right))
                } else if op == "&&" {
                    Variant::from_bool(left.op_and(&right))
                } else if op == "||" {
                    Variant::from_bool(left.op_or(&right))
                } else if op == "<" {
                    Variant::from_bool(left.op_lt(&right)?)
                } else if op == "<=" {
                    Variant::from_bool(left.op_lte(&right)?)
                } else if op == ">" {
                    Variant::from_bool(left.op_gt(&right)?)
                } else if op == ">=" {
                    Variant::from_bool(left.op_gte(&right)?)
                } else if op == "==" {
                    Variant::from_bool(left.op_eq(&right)?)
                } else if op == "!==" {
                    Variant::from_bool(left.op_ne(&right)?)
                } else if op == "=" {
                    Variant::from_bool(left.op_partial(&right)?)
                } else if op == "!=" {
                    Variant::from_bool(left.op_nopartial(&right)?)
                } else if op == "+" {
                    left.op_add(&right)?
                } else if op == "-" {
                    left.op_sub(&right)?
                } else if op == "*" {
                    left.op_mul(&right)?
                } else if op == "/" {
                    left.op_div(&right)?
                } else if op == "^" {
                    left.op_exp(&right)?
                } else if op == "%" {
                    left.op_mod(&right)?
                } else if op == "xor" {
                    Variant::from_bool(left.op_xor(&right))
                } else if let Some(context_task) = context_task {
                    if op == "~" {
                        Variant::from_bool(left.op_match(&right, context_task)?)
                    } else if op == "!~" {
                        Variant::from_bool(left.op_nomatch(&right, context_task)?)
                    } else if op == "_hastag_" {
                        Variant::from_bool(left.op_hastag(&right, context_task)?)
                    } else if op == "_notag_" {
                        Variant::from_bool(left.op_notag(&right, context_task)?)
                    } else {
                        return Err(TwError::Message(format(
                            "Unsupported operator '{1}'.",
                            &[&token.0],
                        )));
                    }
                } else {
                    return Err(TwError::Message(format(
                        "Unsupported operator '{1}'.",
                        &[&token.0],
                    )));
                };

                if self._debug {
                    resolver.debug(&format(
                        "Eval ↓'{1}' {2} ↓'{3}' → ↑'{4}'",
                        &[
                            &left.to_string(),
                            &token.0,
                            &right.to_string(),
                            &result.to_string(),
                        ],
                    ));
                }
                values.push(result);
            }
            // Literals and identifiers.
            else {
                let mut v = Variant::from_string(&token.0);
                match token.1 {
                    Type::number => {
                        if Lexer::is_all_digits(&token.0) {
                            v.cast(VariantType::Integer)?;
                            if self._debug {
                                resolver.debug(&format(
                                    "Eval literal number ↑'{1}'",
                                    &[&v.to_string()],
                                ));
                            }
                        } else {
                            v.cast(VariantType::Real)?;
                            if self._debug {
                                resolver.debug(&format(
                                    "Eval literal decimal ↑'{1}'",
                                    &[&v.to_string()],
                                ));
                            }
                        }
                    }

                    Type::op => {
                        // Unreachable, as in C++: every op token took a branch above.
                        return err("Operator expected.");
                    }

                    Type::dom | Type::identifier => {
                        let mut found = false;
                        if let Some(value) = Self::source(&token.0, resolver)? {
                            v = value;
                            if self._debug {
                                resolver.debug(&format(
                                    "Eval identifier source '{1}' → ↑'{2}'",
                                    &[&token.0, &v.to_string()],
                                ));
                            }
                            found = true;
                        }

                        // An identifier that fails lookup is a string.
                        if !found {
                            v.cast(VariantType::String)?;
                            if self._debug {
                                resolver.debug(&format(
                                    "Eval identifier source failed '{1}'",
                                    &[&token.0],
                                ));
                            }
                        }
                    }

                    Type::date => {
                        v.cast(VariantType::Date)?;
                        if self._debug {
                            resolver.debug(&format("Eval literal date ↑'{1}'", &[&v.to_string()]));
                        }
                    }

                    Type::duration => {
                        v.cast(VariantType::Duration)?;
                        if self._debug {
                            resolver
                                .debug(&format("Eval literal duration ↑'{1}'", &[&v.to_string()]));
                        }
                    }

                    // Nothing to do.
                    _ => {
                        if self._debug {
                            resolver
                                .debug(&format("Eval literal string ↑'{1}'", &[&v.to_string()]));
                        }
                    }
                }

                values.push(v);
            }
        }

        // If there is more than one variant left on the stack, then the original
        // expression was not valid.
        if values.len() != 1 {
            return err("The value is not an expression.");
        }

        Ok(values.swap_remove(0))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::infixParse`.
    ///
    /// Grammar:
    /// ```text
    ///   Logical     --> Regex {( "and" | "or" | "xor" ) Regex}
    ///   Regex       --> Equality {( "~" | "!~" ) Equality}
    ///   Equality    --> Comparative {( "==" | "=" | "!=" ) Comparative}
    ///   Comparative --> Arithmetic {( "<=" | "<" | ">=" | ">" ) Arithmetic}
    ///   Arithmetic  --> Geometric {( "+" | "-" ) Geometric}
    ///   Geometric   --> Tag {( "*" | "/" | "%" ) Tag}
    ///   Tag         --> Unary {( "_hastag_" | "_notag_" ) Unary}
    ///   Unary       --> [( "-" | "+" | "!" )] Exponent
    ///   Exponent    --> Primitive ["^" Primitive]
    ///   Primitive   --> "(" Logical ")" | Variant
    /// ```
    /// As in C++, the result of the parse is ignored: it only rewrites unary
    /// `-` / `+`.
    ///
    /// `Err` only when the resolver throws (see [`DomResolver::resolve`]).
    fn infix_parse(&self, infix: &mut [Token], resolver: &mut dyn DomResolver) -> Result<()> {
        let mut i: usize = 0;
        self.parse_logical(infix, &mut i, resolver)?;
        Ok(())
    }

    /// `infix[i]` is an operator token equal to one of `ops`.
    fn is_op_at(infix: &[Token], i: usize, ops: &[&str]) -> bool {
        i < infix.len() && infix[i].1 == Type::op && ops.contains(&infix[i].0.as_str())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseLogical`:
    /// `Logical --> Regex {( "and" | "or" | "xor" ) Regex}`.
    fn parse_logical(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_regex(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["and", "or", "xor"]) {
                *i += 1;
                if !self.parse_regex(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseRegex`:
    /// `Regex --> Equality {( "~" | "!~" ) Equality}`.
    fn parse_regex(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_equality(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["~", "!~"]) {
                *i += 1;
                if !self.parse_equality(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseEquality`:
    /// `Equality --> Comparative {( "==" | "=" | "!==" | "!=" ) Comparative}`.
    fn parse_equality(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_comparative(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["==", "=", "!==", "!="]) {
                *i += 1;
                if !self.parse_comparative(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseComparative`:
    /// `Comparative --> Arithmetic {( "<=" | "<" | ">=" | ">" ) Arithmetic}`.
    fn parse_comparative(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_arithmetic(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["<=", "<", ">=", ">"]) {
                *i += 1;
                if !self.parse_arithmetic(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseArithmetic`:
    /// `Arithmetic --> Geometric {( "+" | "-" ) Geometric}`.
    fn parse_arithmetic(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_geometric(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["+", "-"]) {
                *i += 1;
                if !self.parse_geometric(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseGeometric`:
    /// `Geometric --> Tag {( "*" | "/" | "%" ) Tag}`.
    fn parse_geometric(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_tag(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["*", "/", "%"]) {
                *i += 1;
                if !self.parse_tag(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseTag`:
    /// `Tag --> Unary {( "_hastag_" | "_notag_" ) Unary}`.
    fn parse_tag(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_unary(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["_hastag_", "_notag_"]) {
                *i += 1;
                if !self.parse_unary(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseUnary`:
    /// `Unary --> [( "-" | "+" | "!" )] Exponent`.
    fn parse_unary(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() {
            if infix[*i].0 == "-" {
                infix[*i].0 = "_neg_".to_string();
                *i += 1;
            } else if infix[*i].0 == "+" {
                infix[*i].0 = "_pos_".to_string();
                *i += 1;
            } else if infix[*i].0 == "!" {
                *i += 1;
            }
        }

        self.parse_exponent(infix, i, resolver)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parseExponent`:
    /// `Exponent --> Primitive ["^" Primitive]`.
    fn parse_exponent(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() && self.parse_primitive(infix, i, resolver)? {
            while Self::is_op_at(infix, *i, &["^"]) {
                *i += 1;
                if !self.parse_primitive(infix, i, resolver)? {
                    return Ok(false);
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::parsePrimitive`:
    /// `Primitive --> "(" Logical ")" | Variant`.
    fn parse_primitive(
        &self,
        infix: &mut [Token],
        i: &mut usize,
        resolver: &mut dyn DomResolver,
    ) -> Result<bool> {
        if *i < infix.len() {
            if infix[*i].0 == "(" {
                *i += 1;
                if *i < infix.len()
                    && self.parse_logical(infix, i, resolver)?
                    && *i < infix.len()
                    && infix[*i].0 == ")"
                {
                    *i += 1;
                    return Ok(true);
                }
            } else {
                let found = Self::source(&infix[*i].0, resolver)?.is_some();

                if found || infix[*i].1 != Type::op {
                    *i += 1;
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::infixToPostfix`: Dijkstra Shunting Algorithm.
    /// <https://en.wikipedia.org/wiki/Shunting-yard_algorithm>
    ///
    /// ```text
    ///   While there are tokens to be read:
    ///     Read a token.
    ///     If the token is an operator, o1, then:
    ///       while there is an operator token, o2, at the top of the stack, and
    ///             either o1 is left-associative and its precedence is less than or
    ///             equal to that of o2,
    ///             or o1 is right-associative and its precedence is less than that
    ///             of o2,
    ///         pop o2 off the stack, onto the output queue;
    ///       push o1 onto the stack.
    ///     If the token is a left parenthesis, then push it onto the stack.
    ///     If the token is a right parenthesis:
    ///       Until the token at the top of the stack is a left parenthesis, pop
    ///       operators off the stack onto the output queue.
    ///       Pop the left parenthesis from the stack, but not onto the output queue.
    ///       If the token at the top of the stack is a function token, pop it onto
    ///       the output queue.
    ///       If the stack runs out without finding a left parenthesis, then there
    ///       are mismatched parentheses.
    ///     If the token is a number, then add it to the output queue.
    ///
    ///   When there are no more tokens to read:
    ///     While there are still operator tokens in the stack:
    ///       If the operator token on the top of the stack is a parenthesis, then
    ///       there are mismatched parentheses.
    ///       Pop the operator onto the output queue.
    ///   Exit.
    /// ```
    fn infix_to_postfix(&self, infix: &mut Vec<Token>) -> Result<()> {
        // Short circuit.
        if infix.len() == 1 {
            return Ok(());
        }

        // Result.
        let mut postfix: Vec<Token> = Vec::new();

        // Shunting yard.
        let mut op_stack: Vec<Token> = Vec::new();

        for token in infix.iter() {
            if token.1 == Type::op && token.0 == "(" {
                op_stack.push(token.clone());
            } else if token.1 == Type::op && token.0 == ")" {
                while let Some(top) = op_stack.last() {
                    if top.0 == "(" {
                        break;
                    }
                    postfix.push(top.clone());
                    op_stack.pop();
                }

                if op_stack.pop().is_none() {
                    return err("Mismatched parentheses in expression");
                }
            } else if let (Type::op, Some((_type, precedence, associativity))) =
                (token.1, self.identify_operator(&token.0))
            {
                while let Some(top) = op_stack.last() {
                    let Some((_type2, precedence2, _associativity2)) =
                        self.identify_operator(&top.0)
                    else {
                        break;
                    };
                    if !((associativity == 'l' && precedence <= precedence2)
                        || (associativity == 'r' && precedence < precedence2))
                    {
                        break;
                    }
                    postfix.push(top.clone());
                    op_stack.pop();
                }

                op_stack.push(token.clone());
            } else {
                postfix.push(token.clone());
            }
        }

        while let Some(top) = op_stack.pop() {
            if top.0 == "(" || top.0 == ")" {
                return err("Mismatched parentheses in expression");
            }

            postfix.push(top);
        }

        *infix = postfix;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::identifyOperator`: the `(type, precedence,
    /// associativity)` of `op` (C++ out-parameters, `true` -> `Some`).
    fn identify_operator(&self, op: &str) -> Option<(char, u32, char)> {
        for opr in &OPERATORS {
            if opr.op == op {
                return Some((opr.type_, opr.precedence, opr.associativity));
            }
        }

        None
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Eval::dump`: the tokens colourised by type, separated by
    /// single spaces.
    ///
    /// Deviation: the C++ `Color (const std::string&)` constructor throws on
    /// a bad spec; [`Color::parse`] returns `Result`, so `dump` does too (the
    /// specs are constants that always parse).
    fn dump(&self, tokens: &[Token]) -> Result<String> {
        // Set up a color mapping.
        let mut color_map: BTreeMap<Type, Color> = BTreeMap::new();
        color_map.insert(Type::op, Color::parse("gray14 on gray6")?);
        color_map.insert(Type::number, Color::parse("rgb530 on gray6")?);
        color_map.insert(Type::hex, Color::parse("rgb303 on gray6")?);
        color_map.insert(Type::string, Color::parse("rgb550 on gray6")?);
        color_map.insert(Type::dom, Color::parse("rgb045 on gray6")?);
        color_map.insert(Type::identifier, Color::parse("rgb035 on gray6")?);
        color_map.insert(Type::date, Color::parse("rgb150 on gray6")?);
        color_map.insert(Type::duration, Color::parse("rgb531 on gray6")?);

        let mut output = String::new();
        for (index, token) in tokens.iter().enumerate() {
            if index != 0 {
                output.push(' ');
            }

            // C++ `color_map[i->second]` default-constructs a trivial Color
            // for unmapped types.
            let mapped = color_map.get(&token.1).copied().unwrap_or_default();
            let c = if mapped.nontrivial() {
                mapped
            } else {
                Color::parse("rgb000 on gray6")?
            };

            output.push_str(&c.colorize(&token.0));
        }

        Ok(output)
    }
}

#[cfg(test)]
#[path = "eval_tests.rs"]
mod tests;
