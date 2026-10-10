//! Rust translation of `src/commands/CmdCalc.cpp`.
//!
//! The struct is the C++ class from `CmdCalc.cpp`, carrying the exact
//! metadata of its C++ constructor.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dom::ContextResolver;
use crate::error::Result;
use crate::eval::Eval;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCalc` (`calc`).
pub struct CmdCalc {
    meta: CommandMeta,
}

impl CmdCalc {
    /// C++ `CmdCalc::CmdCalc`.
    pub fn new() -> Self {
        CmdCalc {
            meta: CommandMeta {
                keyword: "calc".into(),
                usage: "task          calc <expression>".into(),
                description: "Calculator".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCalc {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCalc {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCalc::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Configurable infix/postfix
        let mut infix = true;
        if context.config.get("expressions") == "postfix" {
            infix = false;
        }

        // Create an evaluator with DOM access.
        let mut e = Eval::new();
        e.debug(context.config.get_boolean("debug"));

        // Compile all the args into one expression.
        let mut expression = String::new();
        for word in context.cli2.get_words(&context.config, &mut context.debug_messages) {
            expression += &word;
            expression.push(' ');
        }

        // Evaluate according to preference. The resolver is C++ `domSource`:
        // `getDOM (identifier, Context::getContext ().currentTask, value)`,
        // with Eval's debug output going to `Context::debug`.
        let mut resolver = ContextResolver::new(context);
        let result = if infix {
            e.evaluate_infix_expression(&expression, &mut resolver)?
        } else {
            e.evaluate_postfix_expression(&expression, &mut resolver)?
        };

        *output = format!("{result}\n");
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::datetime::Datetime;
    use crate::shared::duration::Duration;

    /// A Context whose CLI2 holds `args` (as `Context::initialize` would
    /// add them) and whose configuration holds the `config` settings.
    /// Also applies the `Context::staticInitialization` lexer settings
    /// (thread-local, so they only affect this test's thread).
    fn context_with(args: &[&str], config: &[(&str, &str)]) -> Context {
        Datetime::set_standalone_date_enabled(false);
        Datetime::set_standalone_time_enabled(false);
        Duration::set_standalone_seconds_enabled(false);
        let mut context = Context::new();
        for (name, value) in config {
            context.config.set(name, *value);
        }
        crate::commands::command::register(&mut context.commands, std::rc::Rc::new(CmdCalc::new()));
        context.cli2.entity("cmd", "calc");
        for a in args {
            context.cli2.add(a);
        }
        crate::cli2::with_cli2(&mut context, |cli2, ctx| cli2.analyze(ctx)).unwrap();
        context
    }

    fn run(args: &[&str], config: &[(&str, &str)]) -> (Result<i32>, String, Vec<String>) {
        let mut context = context_with(args, config);
        let mut output = String::new();
        let rc = CmdCalc::new().execute(&mut context, &mut output);
        (rc, output, context.debug_messages)
    }

    #[test]
    fn infix_by_default() {
        let (rc, out, debug) = run(&["task", "calc", "12", "*", "3600", "+", "34*60+56"], &[]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "45296\n");
        assert!(debug.is_empty());
    }

    #[test]
    fn postfix_by_configuration() {
        let (rc, out, _) = run(&["task", "calc", "12 3600 * 34 60 * 56 + +"], &[("expressions", "postfix")]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "45296\n");
    }

    #[test]
    fn debug_goes_to_the_context_sink() {
        let (rc, out, debug) = run(&["task", "calc", "2- -3"], &[("debug", "1")]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "5\n");
        assert!(debug.iter().any(|m| m == "Eval literal number ↑'2'"), "{debug:?}");
        assert!(debug.iter().any(|m| m == "Eval _neg_ ↓'3' → ↑'-3'"), "{debug:?}");
        assert!(debug.iter().any(|m| m.starts_with("\x1b[1;37;42mFILTER\x1b[0m Infix ")), "{debug:?}");
    }

    #[test]
    fn errors_propagate() {
        let (rc, out, _) = run(&["task", "calc", "1 +"], &[]);
        assert!(rc.is_err());
        assert_eq!(out, "");
    }

    #[test]
    fn dom_references_resolve() {
        // C++ `domSource` with a null currentTask: context-free references.
        let (rc, out, _) = run(&["task", "calc", "rc.foo"], &[("foo", "bar")]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "bar\n");

        // rc values are strings: string * integer repeats (C++ Variant).
        let (rc, out, _) = run(&["task", "calc", "rc.width", "*", "2"], &[("width", "40")]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "4040\n");

        let (_, out, _) = run(&["task", "calc", "system.version"], &[]);
        assert_eq!(out, format!("{}\n", crate::version::VERSION));

        // An unknown reference is a string, as in C++.
        let (_, out, _) = run(&["task", "calc", "rc.missing"], &[]);
        assert_eq!(out, "rc.missing\n");
    }
}
